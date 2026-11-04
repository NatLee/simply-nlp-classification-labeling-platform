//! 整合測試：真的啟動伺服器，用 HTTP 呼叫 API。AI 服務換成本機的假伺服器（Claude 和 OpenAI 相容各一個）。

use std::{
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use reqwest::{multipart, StatusCode};
use serde_json::{json, Value};
use simply_nlp_labeling::{
    app, build_state,
    config::{Config, LlmSettings, ProviderConfig},
};
use tempfile::TempDir;
use tokio::net::TcpListener;

// ───────────────────────────── 測試用的伺服器 ─────────────────────────────

struct Server {
    base: String,
    http: reqwest::Client,
    dir: TempDir,
}

fn config(dir: &Path, provider: Option<ProviderConfig>) -> Config {
    Config {
        bind: "127.0.0.1:0".into(),
        data_dir: dir.to_path_buf(),
        database_path: dir.join("labels.db"),
        static_dir: dir.join("web"),
        max_upload_bytes: 20 * 1024 * 1024,
        llm: LlmSettings {
            disabled_reason: provider.is_none().then(|| "沒有設定 LLM_PROVIDER".to_string()),
            provider,
            timeout: Duration::from_secs(10),
            concurrency: 2,
        },
        // 登入的測試在 tests/auth.rs
        auth: None,
    }
}

async fn serve(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

impl Server {
    async fn start(provider: Option<ProviderConfig>) -> Self {
        let dir = TempDir::new().unwrap();
        let state = build_state(config(dir.path(), provider)).await.unwrap();
        let base = serve(app(state)).await;
        Self {
            base,
            http: reqwest::Client::new(),
            dir,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api{path}", self.base)
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let r = self.http.get(self.url(path)).send().await.unwrap();
        let s = r.status();
        (s, r.json().await.unwrap_or(Value::Null))
    }

    async fn get_bytes(&self, path: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        let r = self.http.get(self.url(path)).send().await.unwrap();
        let (s, h) = (r.status(), r.headers().clone());
        (s, h, r.bytes().await.unwrap().to_vec())
    }

    async fn send(&self, method: reqwest::Method, path: &str, body: Value) -> (StatusCode, Value) {
        let r = self
            .http
            .request(method, self.url(path))
            .json(&body)
            .send()
            .await
            .unwrap();
        let s = r.status();
        (s, r.json().await.unwrap_or(Value::Null))
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(reqwest::Method::POST, path, body).await
    }

    async fn put(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(reqwest::Method::PUT, path, body).await
    }

    async fn upload(&self, path: &str, filename: &str, bytes: Vec<u8>, fields: &[(&str, &str)]) -> (StatusCode, Value) {
        let mut form =
            multipart::Form::new().part("file", multipart::Part::bytes(bytes).file_name(filename.to_string()));
        for (k, v) in fields {
            form = form.text(k.to_string(), v.to_string());
        }
        let r = self.http.post(self.url(path)).multipart(form).send().await.unwrap();
        let s = r.status();
        (s, r.json().await.unwrap_or(Value::Null))
    }

    async fn project(&self, template: &str) -> i64 {
        let (s, p) = self
            .post(
                "/projects",
                json!({"name": format!("測試 {template}"), "templateId": template}),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{p}");
        p["id"].as_i64().unwrap()
    }

    async fn annotator(&self, name: &str) -> i64 {
        let (s, a) = self.post("/annotators", json!({"name": name})).await;
        assert_eq!(s, StatusCode::CREATED, "{a}");
        a["id"].as_i64().unwrap()
    }

    async fn item_ids(&self, project: i64) -> Vec<i64> {
        let (_, v) = self.get(&format!("/projects/{project}/items?perPage=200")).await;
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_i64().unwrap())
            .collect()
    }

    async fn label(&self, item: i64, annotator: i64, values: Value) -> (StatusCode, Value) {
        self.put(
            &format!("/items/{item}/annotation"),
            json!({"annotatorId": annotator, "values": values}),
        )
        .await
    }
}

fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn zip_of(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// 匯出的 JSONL 去掉 id 和時間，只留內容，拿來比較兩個專案是否一樣。
fn comparable(jsonl: &[u8]) -> Vec<Value> {
    let mut rows: Vec<Value> = String::from_utf8(jsonl.to_vec())
        .unwrap()
        .lines()
        .map(|l| {
            let mut v: Value = serde_json::from_str(l).unwrap();
            v.as_object_mut().unwrap().remove("id");
            for a in v["annotations"].as_array_mut().unwrap() {
                let a = a.as_object_mut().unwrap();
                a.remove("created_at");
                a.remove("source");
            }
            v
        })
        .collect();
    rows.sort_by_key(|v| v["text"].as_str().unwrap_or_default().to_string());
    rows
}

// ───────────────────────────── 專案、標注結構、標注 ─────────────────────────────

#[tokio::test]
async fn labeling_flow_with_schema_validation() {
    let s = Server::start(None).await;
    let (status, meta) = s.get("/meta").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(meta["ai"]["enabled"], false);

    let (status, templates) = s.get("/templates").await;
    assert_eq!(status, StatusCode::OK);
    assert!(templates
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == "legacy-emotion"));

    let (status, bad) = s
        .post(
            "/projects",
            json!({"name": "壞", "schema": {"fields": [{"id": "X", "label": "", "type": "single", "options": []}]}}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(bad["details"].as_array().unwrap().len() >= 3, "{bad}");

    let p = s.project("legacy-emotion").await;
    let amy = s.annotator("Amy").await;
    let bob = s.annotator("Bob").await;
    let (status, _) = s.post("/annotators", json!({"name": "amy"})).await;
    assert_eq!(status, StatusCode::CONFLICT, "names are case-insensitive");
    let (status, _) = s.post("/annotators", json!({"name": "AI · fake"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 舊版的 Text 範本：一行一筆，重複的略過
    let csv = "Text\n第一句,有逗號\n第二句\n第一句,有逗號\n".as_bytes().to_vec();
    let (status, r) = s
        .upload(
            &format!("/projects/{p}/import"),
            "textDataExample.csv",
            csv.clone(),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!((r["created"].as_u64(), r["format"].as_str()), (Some(2), Some("csv")));
    let (_, r) = s.upload(&format!("/projects/{p}/import"), "again.csv", csv, &[]).await;
    assert_eq!(
        (r["created"].as_u64(), r["duplicates"].as_u64()),
        (Some(0), Some(3)),
        "every row of the second upload is a duplicate"
    );

    // 佇列：Amy 標完兩筆之後就沒有了
    let mut seen = Vec::new();
    for _ in 0..2 {
        let (_, next) = s.get(&format!("/projects/{p}/next?annotator={amy}")).await;
        let id = next["itemId"].as_i64().expect("an item");
        assert!(!seen.contains(&id));
        let (status, a) = s
            .label(
                id,
                amy,
                json!({"score": 4, "tag": "happy", "type": "閒聊", "tag_opt": ["政治"]}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{a}");
        assert_eq!(a["source"], "human");
        seen.push(id);
    }
    let (_, next) = s.get(&format!("/projects/{p}/next?annotator={amy}")).await;
    assert!(next["itemId"].is_null());
    // Bob 還沒標，跳過的下一筆不會馬上又出現（除非只剩那一筆）
    let (_, next) = s
        .get(&format!("/projects/{p}/next?annotator={bob}&exclude={}", seen[0]))
        .await;
    assert_eq!(next["itemId"].as_i64(), Some(seen[1]));

    // 標注驗證：錯的值回 400 和每個問題
    let (status, err) = s.label(seen[0], bob, json!({"score": 9, "tag": "joy"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["details"].as_array().unwrap().len(), 2, "{err}");

    // 同一人再送一次是更新，不會多一筆
    let (_, again) = s
        .label(seen[0], amy, json!({"score": 2, "tag": "sad", "type": "閒聊"}))
        .await;
    let (_, detail) = s.get(&format!("/items/{}?annotator={amy}", seen[0])).await;
    assert_eq!(detail["annotations"].as_array().unwrap().len(), 1);
    assert_eq!(detail["mine"]["id"], again["id"]);
    assert_eq!(detail["mine"]["values"]["score"], 2.0);
    // 舊版的「這筆文字以前用過的自訂標籤」：另一筆用過的「政治」出現在常用標籤裡
    assert_eq!(detail["tagHints"]["tag_opt"], json!(["政治"]));

    // 改結構：拿掉已經用過的選項會被擋下，加選項可以
    let (_, project) = s.get(&format!("/projects/{p}")).await;
    let mut schema = project["schema"].clone();
    let tag_options = schema["fields"][1]["options"].as_array_mut().unwrap();
    tag_options.retain(|o| o["value"] != "sad");
    let (status, err) = s.put(&format!("/projects/{p}"), json!({"schema": schema})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{err}");
    let mut schema = project["schema"].clone();
    schema["fields"][1]["options"]
        .as_array_mut()
        .unwrap()
        .push(json!({"value": "calm", "label": "calm", "emoji": "😌"}));
    let (status, updated) = s
        .put(&format!("/projects/{p}"), json!({"schema": schema, "name": "改名了"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "改名了");

    // 統計：和舊版儀表板一樣的三個數字
    let (_, stats) = s.get(&format!("/projects/{p}/stats?annotator={amy}")).await;
    assert_eq!(stats["items"]["coverRate"], 1.0);
    assert_eq!(stats["items"]["meanLabels"], 1.0);
    assert_eq!(stats["items"]["stdLabels"], 0.0);
    assert_eq!(stats["mine"], 2);

    // 隱藏的資料不出現在佇列
    s.send(
        reqwest::Method::PATCH,
        &format!("/items/{}", seen[1]),
        json!({"hidden": true}),
    )
    .await;
    let (_, next) = s.get(&format!("/projects/{p}/next?annotator={bob}")).await;
    assert_eq!(next["itemId"].as_i64(), Some(seen[0]));

    // 有標注的標注者不能刪
    let (status, _) = s
        .send(reqwest::Method::DELETE, &format!("/annotators/{amy}"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = s
        .send(reqwest::Method::DELETE, &format!("/annotators/{bob}"), json!({}))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ───────────────────────────── 舊版 emoLabel.db 遷移 ─────────────────────────────

/// 舊版 Flask 程式建立的資料表（從 repo 裡原本那個 emoLabel.db 的 sqlite_master 抄下來，連換行都一樣）。
const LEGACY_DDL: &[&str] = &[
    "CREATE TABLE \"emotion\" (\r\n\t\"id\"\tINTEGER NOT NULL PRIMARY KEY AUTOINCREMENT UNIQUE,\r\n\t\"text\"\tINTEGER NOT NULL UNIQUE,\r\n\t\"date\"\tTEXT NOT NULL,\r\n\t\"flag\"\tINTEGER NOT NULL\r\n)",
    "CREATE TABLE \"label\" (\r\n\t\"id\"\tINTEGER NOT NULL PRIMARY KEY AUTOINCREMENT UNIQUE,\r\n\t\"emotionId\"\tINTEGER NOT NULL,\r\n\t\"score\"\tREAL NOT NULL,\r\n\t\"tag\"\tTEXT NOT NULL,\r\n\t\"type\"\tTEXT NOT NULL,\r\n\t\"tag_opt\"\tTEXT NOT NULL,\r\n\t\"updateDate\"\tTEXT NOT NULL\r\n)",
];

/// 照舊版 `EmoLabel.insertText`／`insertLabel` 的寫法塞資料。
async fn make_legacy_db(path: &Path) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    for ddl in LEGACY_DDL {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    let texts = [("今天的晚餐超好吃", 1), ("2019", 1), ("不給看的", 0), ("沒人標的", 1)];
    for (text, flag) in texts {
        sqlx::query("insert or ignore into emotion (text, date, flag) values (?, ?, ?)")
            .bind(text)
            .bind("2019-05-24 13:05:01")
            .bind(flag.to_string())
            .execute(&pool)
            .await
            .unwrap();
    }
    // tag_opt 是 Python 的 repr(list)，score 是 str(int)
    let labels = [
        (1, "4", "happy", "閒聊", "['美食', \"it's\"]", "2019-05-24 13:10:00"),
        (1, "5", "happy", "其它", "[]", "2019-05-25 09:00:00"),
        (2, "3", "wow", "問答", "['數字']", "2019-05-25 09:01:00"),
        (3, "1", "angry", "人工客服", "[]", "2019-05-25 09:02:00"),
        (99, "2", "sad", "其它", "[]", "2019-05-25 09:03:00"), // 對不到文字
    ];
    for (emotion_id, score, tag, kind, tag_opt, date) in labels {
        sqlx::query("insert into label (emotionId, score, tag, type, tag_opt, updateDate) values (?, ?, ?, ?, ?, ?)")
            .bind(emotion_id.to_string())
            .bind(score)
            .bind(tag)
            .bind(kind)
            .bind(tag_opt)
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;
}

#[tokio::test]
async fn legacy_database_is_migrated() {
    let s = Server::start(None).await;
    let old = s.dir.path().join("emoLabel.db");
    make_legacy_db(&old).await;
    let bytes = std::fs::read(&old).unwrap();

    let (status, r) = s
        .upload(
            "/import/legacy-db",
            "emoLabel.db",
            bytes.clone(),
            &[("name", "舊資料"), ("utcOffsetMinutes", "480")],
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{r}");
    assert_eq!(r["items"], 4);
    assert_eq!(r["hiddenItems"], 1);
    assert_eq!(r["annotations"], 4);
    assert_eq!(r["orphanLabels"], 1);
    let p = r["projectId"].as_i64().unwrap();

    let (_, project) = s.get(&format!("/projects/{p}")).await;
    assert_eq!(project["name"], "舊資料");
    let ids: Vec<&str> = project["schema"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["score", "tag", "type", "tag_opt"]);

    let (_, list) = s.get(&format!("/projects/{p}/items?perPage=10")).await;
    let items = list["items"].as_array().unwrap();
    let texts: Vec<&str> = items.iter().map(|i| i["text"].as_str().unwrap()).collect();
    // 純數字的文字在舊資料庫裡是整數，要轉回字串
    assert_eq!(texts, ["今天的晚餐超好吃", "2019", "不給看的", "沒人標的"]);
    assert_eq!(items[2]["hidden"], true);
    assert_eq!(items[0]["meta"]["legacy_id"], 1);
    // 2019-05-24 13:05:01（UTC+8）
    assert_eq!(items[0]["createdAt"], 1_558_674_301);

    let (_, detail) = s.get(&format!("/items/{}", items[0]["id"])).await;
    let anns = detail["annotations"].as_array().unwrap();
    assert_eq!(anns.len(), 2);
    assert_eq!(
        anns[0]["values"],
        json!({"score": 4.0, "tag": "happy", "type": "閒聊", "tag_opt": ["美食", "it's"]})
    );
    assert_eq!(anns[0]["source"], "legacy");
    assert!(anns[0]["annotatorId"].is_null());
    assert_eq!(anns[1]["values"], json!({"score": 5.0, "tag": "happy", "type": "其它"}));

    // 舊版儀表板（只算 flag=1 的 3 筆）：被標次數 2、1、0 → 覆蓋率 2/3、平均 1、標準差 sqrt(2/3)
    let (_, stats) = s.get(&format!("/projects/{p}/stats")).await;
    let close = |v: &Value, want: f64| assert!((v.as_f64().unwrap() - want).abs() < 1e-9, "{v} != {want}");
    close(&stats["items"]["coverRate"], 2.0 / 3.0);
    close(&stats["items"]["meanLabels"], 1.0);
    close(&stats["items"]["stdLabels"], (2.0f64 / 3.0).sqrt());
    assert_eq!(stats["anonymous"], 4);

    // 舊檔沒被改動
    assert_eq!(std::fs::read(&old).unwrap(), bytes);
    // 不是舊版資料庫的檔案會被拒絕
    let (status, _) = s.upload("/import/legacy-db", "x.db", b"not sqlite".to_vec(), &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ───────────────────────────── 匯入匯出往返 ─────────────────────────────

#[tokio::test]
async fn import_export_round_trip() {
    let s = Server::start(None).await;
    let p = s.project("image-text").await;
    let manifest = [
        r#"{"text": "海邊", "image": "images/a.png", "annotations": [{"annotator": "Amy", "values": {"match": "yes", "topics": ["travel"]}}, {"annotator": "Bob", "values": {"match": "yes", "tags": ["夏天"]}}]}"#,
        r#"{"text": "拉麵", "image": "images/b.png", "match": "partly", "source_id": "x-2"}"#,
        r#"{"text": "只有文字", "meta": {"lang": "zh"}}"#,
        r#"{"text": "", "image": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="}"#,
    ]
    .join("\n");
    let zip = zip_of(&[
        ("data/items.jsonl", manifest.into_bytes()),
        ("data/images/a.png", png(30, 20, [10, 120, 200])),
        ("data/images/b.png", png(20, 20, [250, 160, 10])),
        ("__MACOSX/data/._a.png", b"junk".to_vec()),
    ]);
    let (status, r) = s.upload(&format!("/projects/{p}/import"), "set.zip", zip, &[]).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(
        (r["created"].as_u64(), r["annotations"].as_u64(), r["media"].as_u64()),
        (Some(4), Some(3), Some(3))
    );
    assert_eq!(r["format"], "zip+items.jsonl");

    // 圖片可以讀，而且支援 Range
    let (_, list) = s.get(&format!("/projects/{p}/items")).await;
    let media_id = list["items"][0]["media"]["id"].as_i64().unwrap();
    let (status, headers, bytes) = s.get_bytes(&format!("/media/{media_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "image/png");
    assert!(image::load_from_memory(&bytes).is_ok());
    let r = s
        .http
        .get(s.url(&format!("/media/{media_id}")))
        .header("range", "bytes=0-3")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(r.bytes().await.unwrap().as_ref(), b"\x89PNG");

    // zip（JSONL＋圖片）匯出再匯入另一個專案，內容一模一樣
    let (status, _, exported_zip) = s.get_bytes(&format!("/projects/{p}/export?format=zip")).await;
    assert_eq!(status, StatusCode::OK);
    let q = s.project("image-text").await;
    let (status, r) = s
        .upload(&format!("/projects/{q}/import"), "export.zip", exported_zip, &[])
        .await;
    assert_eq!(status, StatusCode::OK, "{r}");
    let (_, _, a) = s.get_bytes(&format!("/projects/{p}/export?format=jsonl")).await;
    let (_, _, b) = s.get_bytes(&format!("/projects/{q}/export?format=jsonl")).await;
    assert_eq!(comparable(&a), comparable(&b));
    let rows = comparable(&a);
    let first = rows.iter().find(|r| r["text"] == "海邊").unwrap();
    assert_eq!(
        first["consensus"],
        json!({"match": "yes"}),
        "topics 和 tags 都只有一人選，沒有過半"
    );
    assert_eq!(first["media"]["mime"], "image/png");

    // CSV（一筆標注一列）匯出再匯入另一個專案：文字、標注者和標注值都對得上
    let c = s.project("sentiment").await;
    let lines = [
        r#"{"text": "好吃", "annotations": [{"annotator": "Amy", "values": {"polarity": "positive", "intensity": 0.8, "note": "有逗號, 也有\"引號\""}}, {"annotator": null, "values": {"polarity": "positive"}}]}"#,
        r#"{"text": "難吃", "annotator": "Bob", "polarity": "negative", "intensity": 0.3}"#,
    ]
    .join("\n");
    let (status, r) = s
        .upload(&format!("/projects/{c}/import"), "x.jsonl", lines.into_bytes(), &[])
        .await;
    assert_eq!(status, StatusCode::OK, "{r}");
    let (_, _, csv) = s.get_bytes(&format!("/projects/{c}/export?format=csv")).await;
    assert!(csv.starts_with(b"\xEF\xBB\xBF"), "CSV starts with a BOM for Excel");
    assert!(String::from_utf8(csv.clone()).unwrap().contains("Amy,import,"));
    let d = s.project("sentiment").await;
    let (status, r) = s.upload(&format!("/projects/{d}/import"), "export.csv", csv, &[]).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!((r["created"].as_u64(), r["annotations"].as_u64()), (Some(2), Some(3)));
    let (_, _, a) = s.get_bytes(&format!("/projects/{c}/export?format=jsonl")).await;
    let (_, _, b) = s.get_bytes(&format!("/projects/{d}/export?format=jsonl")).await;
    assert_eq!(comparable(&a), comparable(&b));
    let t = s.project("image-text").await;
    // 共識匯出（一筆資料一列）
    let (_, _, items_csv) = s
        .get_bytes(&format!("/projects/{p}/export?format=csv&mode=items"))
        .await;
    let items_csv = String::from_utf8(items_csv).unwrap();
    assert!(items_csv
        .lines()
        .next()
        .unwrap()
        .ends_with("n_annotations,match,topics,tags"));

    // 微調／評估用的聊天格式：有共識的資料才會匯出，圖片放 data URL
    let (status, _, chat) = s.get_bytes(&format!("/projects/{p}/export?format=chat-jsonl")).await;
    assert_eq!(status, StatusCode::OK);
    let lines: Vec<Value> = String::from_utf8(chat)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    let msgs = &lines[0]["messages"];
    assert_eq!(msgs[0]["role"], "system");
    assert!(msgs[1]["content"][0]["image_url"]["url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    let answer: Value = serde_json::from_str(msgs[2]["content"].as_str().unwrap()).unwrap();
    assert_eq!(answer, json!({"values": {"match": "yes"}}));

    // 範本下載和舊範例檔
    let (status, _, template) = s.get_bytes(&format!("/projects/{p}/import-template.csv")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(template)
        .unwrap()
        .starts_with("text,match,topics,tags"));
    let (status, _, sample) = s.get_bytes("/samples/labelDataExample.csv").await;
    assert_eq!(status, StatusCode::OK);
    assert!(sample.starts_with(b"Level,Classes,Custom classes,Text"));

    // 匯入錯誤：整批不匯入，列出每一列
    let bad = "text,match\n好,yes\n壞,maybe\n".as_bytes().to_vec();
    let (status, err) = s.upload(&format!("/projects/{t}/import"), "bad.csv", bad, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["details"][0].as_str().unwrap().starts_with("第 2 列"), "{err}");
    let (_, list) = s.get(&format!("/projects/{t}/items?q=好")).await;
    assert_eq!(list["total"], 0);
}

#[tokio::test]
async fn bundled_samples_import_cleanly() {
    let s = Server::start(None).await;
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for (template, file, expect_items, expect_labels) in [
        ("legacy-emotion", "textDataExample.csv", 3, 0),
        ("legacy-emotion", "labelDataExample.csv", 4, 4),
        ("sentiment", "sentiment-sample.jsonl", 12, 0),
        ("image-text", "image-text-sample.zip", 6, 0),
    ] {
        let p = s.project(template).await;
        let bytes = std::fs::read(samples.join(file)).unwrap();
        let (status, r) = s.upload(&format!("/projects/{p}/import"), file, bytes, &[]).await;
        assert_eq!(status, StatusCode::OK, "{file}: {r}");
        assert_eq!(r["created"], expect_items, "{file}");
        assert_eq!(r["annotations"], expect_labels, "{file}");
    }
}

// ───────────────────────────── 一致性 ─────────────────────────────

#[tokio::test]
async fn agreement_statistics() {
    let s = Server::start(None).await;
    let p = s.project("sentiment").await;
    let lines: String = (1..=4).map(|i| format!("{{\"text\": \"句子 {i}\"}}\n")).collect();
    s.upload(&format!("/projects/{p}/import"), "x.jsonl", lines.into_bytes(), &[])
        .await;
    let items = s.item_ids(p).await;
    let (a, b, c) = (s.annotator("A").await, s.annotator("B").await, s.annotator("C").await);
    // A 和 B 在 4 筆裡有 3 筆一樣；C 只標前兩筆
    let pa = ["positive", "negative", "neutral", "positive"];
    let pb = ["positive", "negative", "neutral", "negative"];
    let ia = [0.2, 0.8, 0.5, 0.9];
    let ib = [0.3, 0.8, 0.4, 0.9];
    for i in 0..4 {
        s.label(items[i], a, json!({"polarity": pa[i], "intensity": ia[i]}))
            .await;
        s.label(items[i], b, json!({"polarity": pb[i], "intensity": ib[i]}))
            .await;
    }
    s.label(items[0], c, json!({"polarity": "positive"})).await;
    s.label(items[1], c, json!({"polarity": "neutral"})).await;

    let (_, stats) = s.get(&format!("/projects/{p}/stats")).await;
    let polarity = &stats["fields"][0];
    assert_eq!(polarity["id"], "polarity");
    let ag = &polarity["agreement"];
    assert_eq!(ag["itemsUsed"], 4);
    // 手算：A/B 的 Cohen's kappa。類別順序 positive, neutral, negative
    // A = (2, 1, 1)，B = (1, 1, 2)，一致 3/4；pe = (2·1 + 1·1 + 1·2)/16 = 5/16；κ = (12/16 − 5/16)/(11/16) = 7/11
    let ab = ag["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["a"] == "A" && x["b"] == "B")
        .unwrap();
    assert_eq!(ab["n"], 4);
    assert!((ab["cohenKappa"].as_f64().unwrap() - 7.0 / 11.0).abs() < 1e-9, "{ab}");
    assert_eq!(ab["percent"], 0.75);
    // Fleiss：每筆 [pos, neu, neg] 的人數 = [3,0,0] [0,1,2] [0,2,0] [1,0,1]
    // P_i = 1, 1/3, 1, 0 → P̄ = 7/12；p = (4, 3, 3)/10 → Pe = 0.34；κ = (7/12 − 0.34)/0.66
    let fleiss = ag["fleissKappa"].as_f64().unwrap();
    assert!((fleiss - (7.0 / 12.0 - 0.34) / 0.66).abs() < 1e-9, "{fleiss}");
    let intensity = &stats["fields"][1];
    assert!(intensity["agreement"]["krippendorffAlpha"].as_f64().unwrap() > 0.8);
    assert_eq!(stats["annotators"].as_array().unwrap().len(), 3);
}

// ───────────────────────────── AI 輔助：假的 Claude 和 OpenAI 相容服務 ─────────────────────────────

#[derive(Default)]
struct FakeLlm {
    requests: Mutex<Vec<(HeaderMap, Value)>>,
    calls: AtomicUsize,
    /// 前幾次回 500，測重試
    fail_first: usize,
}

/// 依資料內容決定回答：文字裡有「不確定」就給低信心。
fn fake_answer(prompt: &str) -> Value {
    let unsure = prompt.contains("不確定");
    json!({
        "values": {"score": if unsure { 7.4 } else { 4.0 }, "tag": if unsure { "nope" } else { "happy" }, "type": "閒聊", "tag_opt": ["AI"]},
        "confidence": if unsure { 0.4 } else { 0.95 },
        "rationale": "語氣很開心。"
    })
}

fn user_text(body: &Value, path: &str) -> String {
    body.pointer(path).map(|v| v.to_string()).unwrap_or_default()
}

async fn fake_anthropic(
    State(f): State<Arc<FakeLlm>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    let n = f.calls.fetch_add(1, Ordering::SeqCst);
    f.requests.lock().unwrap().push((headers, body.clone()));
    if n < f.fail_first {
        return (
            axum::http::StatusCode::from_u16(529).unwrap(),
            Json(json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})),
        );
    }
    let answer = fake_answer(&user_text(&body, "/messages/0/content"));
    (
        axum::http::StatusCode::OK,
        Json(json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": body["model"],
            "stop_reason": "end_turn",
            "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": answer.to_string()}],
        })),
    )
}

async fn fake_openai(
    State(f): State<Arc<FakeLlm>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    let n = f.calls.fetch_add(1, Ordering::SeqCst);
    f.requests.lock().unwrap().push((headers, body.clone()));
    if n < f.fail_first {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": "boom"}})),
        );
    }
    let answer = fake_answer(&user_text(&body, "/messages/1/content"));
    // 小模型常見的回法：把 JSON 包在 ```json 裡
    let content = format!("好的：\n```json\n{answer}\n```");
    (
        axum::http::StatusCode::OK,
        Json(
            json!({"model": body["model"], "choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": content}}]}),
        ),
    )
}

async fn start_fake(fail_first: usize) -> (Arc<FakeLlm>, String) {
    let fake = Arc::new(FakeLlm {
        fail_first,
        ..Default::default()
    });
    let router = Router::new()
        .route("/v1/messages", post(fake_anthropic))
        .route("/v1/chat/completions", post(fake_openai))
        .with_state(fake.clone());
    (fake, serve(router).await)
}

/// 兩種服務都跑同一套流程：要建議 → 接受 → 批次預標，信心夠高的自動接受。
async fn ai_flow(s: &Server, fake: &FakeLlm) {
    let p = s.project("legacy-emotion").await;
    s.put(
        &format!("/projects/{p}"),
        json!({"ai": {"instructions": "依情緒評分", "autoAcceptThreshold": 0.9}}),
    )
    .await;
    let zip = zip_of(&[
        (
            "items.csv",
            "text,image\n好開心,a.png\n這句不確定,\n第三句,\n第四句不確定,\n"
                .as_bytes()
                .to_vec(),
        ),
        ("a.png", png(2000, 1000, [200, 30, 30])),
    ]);
    let (status, r) = s.upload(&format!("/projects/{p}/import"), "x.zip", zip, &[]).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    let items = s.item_ids(p).await;
    let amy = s.annotator("Amy").await;

    let (status, sug) = s.post(&format!("/items/{}/suggest", items[0]), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sug}");
    assert_eq!(
        sug["values"],
        json!({"score": 4.0, "tag": "happy", "type": "閒聊", "tag_opt": ["AI"]})
    );
    assert_eq!(sug["confidence"], 0.95);
    assert_eq!(sug["rationale"], "語氣很開心。");
    assert_eq!(sug["status"], "pending");

    // 接受建議：來源記成 ai_assisted，建議標成 accepted
    let (_, a) = s
        .put(
            &format!("/items/{}/annotation", items[0]),
            json!({"annotatorId": amy, "values": sug["values"], "suggestionId": sug["id"]}),
        )
        .await;
    assert_eq!(a["source"], "ai_assisted");
    let (_, detail) = s.get(&format!("/items/{}", items[0])).await;
    assert_eq!(detail["suggestion"]["status"], "accepted");

    // 低信心、值不合法的建議：分數夾回範圍內，不認得的選項丟掉
    let (_, sug2) = s.post(&format!("/items/{}/suggest", items[1]), json!({})).await;
    assert_eq!(sug2["values"]["score"], 5.0);
    assert!(sug2["values"].get("tag").is_none());
    // 修改後送出：建議標成 modified
    let (_, _) = s
        .put(&format!("/items/{}/annotation", items[1]), json!({"annotatorId": amy, "values": {"score": 2, "tag": "sad", "type": "其它"}, "suggestionId": sug2["id"]}))
        .await;
    let (_, detail) = s.get(&format!("/items/{}", items[1])).await;
    assert_eq!(detail["suggestion"]["status"], "modified");

    // 批次預標：只處理沒人標過、也還沒有建議的兩筆；高信心的那筆自動接受
    let (status, job) = s.post(&format!("/projects/{p}/ai/batch"), json!({"limit": 10})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["total"], 2);
    assert_eq!(job["threshold"], 0.9);
    let mut status_now = Value::Null;
    for _ in 0..100 {
        let (_, st) = s.get(&format!("/projects/{p}/ai/batch")).await;
        status_now = st["job"].clone();
        if status_now["running"] == false {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status_now["done"], 2, "{status_now}");
    assert_eq!(status_now["autoAccepted"], 1, "{status_now}");
    let (_, stats) = s.get(&format!("/projects/{p}/stats")).await;
    assert_eq!(stats["sources"]["ai_auto"], 1);
    let ai_annotator = stats["annotators"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "ai")
        .unwrap();
    assert!(ai_annotator["name"].as_str().unwrap().starts_with("AI · "));
    let (_, pending) = s.get(&format!("/projects/{p}/items?filter=pending")).await;
    assert_eq!(pending["total"], 1, "the low-confidence one waits for a human");

    // 圖片有送給模型，而且大圖先縮小了
    let requests = fake.requests.lock().unwrap();
    assert!(requests.len() >= 4);
    assert!(requests.iter().all(|(_, b)| b.get("temperature").is_none()));
    drop(requests);
}

#[tokio::test]
async fn ai_assist_with_claude_api() {
    let (fake, base) = start_fake(1).await;
    let s = Server::start(Some(ProviderConfig::Anthropic {
        base_url: base,
        api_key: "test-key-123".into(),
        model: "claude-opus-5-5".into(),
        effort: Some("low".into()),
        server_fallbacks: true,
    }))
    .await;
    let (_, meta) = s.get("/meta").await;
    assert_eq!(
        meta["ai"],
        json!({"enabled": true, "provider": "anthropic", "model": "claude-opus-5-5"})
    );
    assert!(
        !meta.to_string().contains("test-key-123"),
        "API key must never reach the browser"
    );

    ai_flow(&s, &fake).await;

    let requests = fake.requests.lock().unwrap();
    let (headers, body) = &requests[1]; // 第一次是 529，重試後成功
    assert_eq!(headers["x-api-key"], "test-key-123");
    assert_eq!(headers["anthropic-version"], "2026-10-21");
    assert_eq!(headers["anthropic-beta"], "server-side-fallback-2026-07-01");
    assert_eq!(body["model"], "claude-opus-5-5");
    assert_eq!(body["fallbacks"], "default");
    assert_eq!(body["output_config"]["effort"], "low");
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    assert!(body["system"].as_str().unwrap().contains("依情緒評分"));
    let image = &body["messages"][0]["content"][0];
    assert_eq!(image["type"], "image");
    assert_eq!(
        image["source"]["media_type"], "image/jpeg",
        "the 2000px PNG is shrunk and re-encoded"
    );
    assert!(body.get("thinking").is_none());
    assert_eq!(requests[0].1, requests[1].1, "the retry sends the same request");
}

#[tokio::test]
async fn ai_assist_with_openai_compatible_api() {
    let (fake, base) = start_fake(1).await;
    let s = Server::start(Some(ProviderConfig::OpenAi {
        base_url: format!("{base}/v1"),
        api_key: None,
        model: "qwen3-vl:2b".into(),
    }))
    .await;
    ai_flow(&s, &fake).await;
    let requests = fake.requests.lock().unwrap();
    let (headers, body) = &requests[1];
    assert!(
        headers.get("authorization").is_none(),
        "Ollama needs no key, so none is sent"
    );
    assert_eq!(body["model"], "qwen3-vl:2b");
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert!(body["messages"][1]["content"][0]["image_url"]["url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/jpeg;base64,"));
}

#[tokio::test]
async fn ai_errors_are_reported_and_ai_off_by_default() {
    // 沒設定：AI 功能關閉，其他照常
    let s = Server::start(None).await;
    let p = s.project("legacy-emotion").await;
    s.upload(&format!("/projects/{p}/import"), "a.csv", b"Text\nhi\n".to_vec(), &[])
        .await;
    let item = s.item_ids(p).await[0];
    let (status, err) = s.post(&format!("/items/{item}/suggest"), json!({})).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(err["error"].as_str().unwrap().contains("LLM_PROVIDER"));
    let (status, _) = s.post(&format!("/projects/{p}/ai/batch"), json!({})).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    // 服務一直失敗：重試後回 502，訊息看得懂
    let (_, base) = start_fake(100).await;
    let s = Server::start(Some(ProviderConfig::OpenAi {
        base_url: format!("{base}/v1"),
        api_key: Some("sk-test".into()),
        model: "m".into(),
    }))
    .await;
    let p = s.project("legacy-emotion").await;
    s.upload(&format!("/projects/{p}/import"), "a.csv", b"Text\nhi\n".to_vec(), &[])
        .await;
    let item = s.item_ids(p).await[0];
    let (status, err) = s.post(&format!("/items/{item}/suggest"), json!({})).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(err["error"].as_str().unwrap().contains("500"), "{err}");
}
