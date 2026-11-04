//! 內建登入的整合測試：啟用後未登入回 401、角色權限、同源檢查、登入失敗限速、啟動時重設管理員密碼。
//! 沒啟用登入時的行為由 tests/api.rs 的所有測試涵蓋（它們都沒有登入）。

use std::{path::Path, time::Duration};

use reqwest::{header, multipart, Method, StatusCode};
use serde_json::{json, Value};
use simply_nlp_labeling::{
    app, build_state,
    config::{AuthConfig, Config, LlmSettings},
};
use tempfile::TempDir;
use tokio::net::TcpListener;

const ADMIN_PASSWORD: &str = "admin-password-123";

fn config(dir: &Path, admin_password: &str) -> Config {
    Config {
        bind: "127.0.0.1:0".into(),
        data_dir: dir.to_path_buf(),
        database_path: dir.join("labels.db"),
        static_dir: dir.join("web"),
        max_upload_bytes: 5 * 1024 * 1024,
        llm: LlmSettings {
            provider: None,
            disabled_reason: Some("沒有設定 LLM_PROVIDER".into()),
            timeout: Duration::from_secs(5),
            concurrency: 1,
        },
        auth: Some(AuthConfig {
            admin_username: "admin".into(),
            admin_password: admin_password.into(),
            public_origin: Some("https://label.example.com".into()),
            cookie_secure: false,
        }),
    }
}

async fn serve(config: Config) -> String {
    let state = build_state(config).await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    format!("http://{addr}/api")
}

/// 一個瀏覽器：記得自己的 session cookie。
struct Browser {
    base: String,
    http: reqwest::Client,
    cookie: Option<String>,
}

impl Browser {
    fn new(base: &str) -> Self {
        Self {
            base: base.into(),
            http: reqwest::Client::new(),
            cookie: None,
        }
    }

    async fn call(&mut self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, c);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        self.remember(resp.headers());
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    fn remember(&mut self, headers: &header::HeaderMap) {
        if let Some(set) = headers.get(header::SET_COOKIE).and_then(|v| v.to_str().ok()) {
            let pair = set.split(';').next().unwrap().to_string();
            self.cookie = if pair.ends_with('=') { None } else { Some(pair) };
        }
    }

    async fn get(&mut self, path: &str) -> (StatusCode, Value) {
        self.call(Method::GET, path, None).await
    }
    async fn post(&mut self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, path, Some(body)).await
    }
    async fn put(&mut self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::PUT, path, Some(body)).await
    }

    async fn login(&mut self, username: &str, password: &str) -> (StatusCode, Value) {
        self.post("/auth/login", json!({"username": username, "password": password}))
            .await
    }
}

async fn admin(base: &str) -> Browser {
    let mut b = Browser::new(base);
    let (s, v) = b.login("admin", ADMIN_PASSWORD).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    b
}

#[tokio::test]
async fn everything_needs_login_once_enabled() {
    let dir = TempDir::new().unwrap();
    let base = serve(config(dir.path(), ADMIN_PASSWORD)).await;
    let mut anon = Browser::new(&base);

    // 公開的只有健康檢查、伺服器資訊和登入
    assert_eq!(anon.get("/health").await.0, StatusCode::OK);
    let (_, meta) = anon.get("/meta").await;
    assert_eq!(meta["auth"]["enabled"], true);
    let (_, me) = anon.get("/auth/me").await;
    assert_eq!(me, json!({"enabled": true, "user": null}));
    for path in [
        "/projects",
        "/annotators",
        "/templates",
        "/media/1",
        "/projects/1/export?format=csv",
        "/samples/textDataExample.csv",
    ] {
        assert_eq!(anon.get(path).await.0, StatusCode::UNAUTHORIZED, "{path}");
    }
    assert_eq!(
        anon.post("/items/1/suggest", json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    // 沒有預設密碼：用空密碼或常見密碼都登不進去
    assert_eq!(anon.login("admin", "").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(anon.login("admin", "admin").await.0, StatusCode::UNAUTHORIZED);

    // 管理員登入後什麼都能做；cookie 是 HttpOnly、SameSite=Lax
    let resp = reqwest::Client::new()
        .post(format!("{base}/auth/login"))
        .json(&json!({"username": "ADMIN", "password": ADMIN_PASSWORD}))
        .send()
        .await
        .unwrap();
    let cookie = resp.headers()[header::SET_COOKIE].to_str().unwrap().to_string();
    assert!(
        cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"),
        "{cookie}"
    );
    let mut a = admin(&base).await;
    let (_, me) = a.get("/auth/me").await;
    assert_eq!(me["user"]["username"], "admin");
    assert_eq!(me["user"]["role"], "admin");
    let (s, p) = a
        .post("/projects", json!({"name": "p", "templateId": "legacy-emotion"}))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{p}");

    // 登出之後 session 就沒用了
    let stolen = a.cookie.clone();
    assert_eq!(a.post("/auth/logout", json!({})).await.0, StatusCode::NO_CONTENT);
    assert!(a.cookie.is_none());
    let mut replay = Browser::new(&base);
    replay.cookie = stolen;
    assert_eq!(replay.get("/projects").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn roles_and_annotator_identity() {
    let dir = TempDir::new().unwrap();
    let base = serve(config(dir.path(), ADMIN_PASSWORD)).await;
    let mut a = admin(&base).await;

    // 啟用登入前就有的標注者「Amy」，建立同名帳號時會連過去
    let (s, amy_annotator) = a.post("/annotators", json!({"name": "Amy"})).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, amy) = a
        .post(
            "/users",
            json!({"username": "amy", "password": "amy-password", "role": "annotator"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{amy}");
    assert_eq!(amy["annotatorId"], amy_annotator["id"]);
    let (s, _) = a
        .post(
            "/users",
            json!({"username": "rita", "password": "rita-password", "role": "reviewer"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(
        a.post(
            "/users",
            json!({"username": "bob", "password": "short", "role": "annotator"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        a.post(
            "/users",
            json!({"username": "AMY", "password": "another-pass", "role": "annotator"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );

    let (_, p) = a
        .post("/projects", json!({"name": "p", "templateId": "legacy-emotion"}))
        .await;
    let pid = p["id"].as_i64().unwrap();
    let form = multipart::Form::new().part(
        "file",
        multipart::Part::bytes(b"Text\nhello\n".to_vec()).file_name("a.csv"),
    );
    let resp = reqwest::Client::new()
        .post(format!("{base}/projects/{pid}/import"))
        .header(header::COOKIE, a.cookie.clone().unwrap())
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let (_, items) = a.get(&format!("/projects/{pid}/items")).await;
    let item = items["items"][0]["id"].as_i64().unwrap();

    // 標注者：可以讀、可以用自己的身分標注；不能用別人的身分，不能建專案、匯出、管理帳號
    let mut amy_b = Browser::new(&base);
    let (_, me) = amy_b.login("amy", "amy-password").await;
    let mine = me["user"]["annotatorId"].as_i64().unwrap();
    assert_eq!(amy_b.get("/projects").await.0, StatusCode::OK);
    let (_, next) = amy_b.get(&format!("/projects/{pid}/next?annotator={mine}")).await;
    assert_eq!(next["itemId"].as_i64(), Some(item));
    let values = json!({"score": 4, "tag": "happy"});
    let (s, saved) = amy_b
        .put(
            &format!("/items/{item}/annotation"),
            json!({"annotatorId": mine, "values": values}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    assert_eq!(saved["annotatorName"], "Amy");
    let admin_annotator = {
        let (_, me) = a.get("/auth/me").await;
        me["user"]["annotatorId"].as_i64().unwrap()
    };
    let (s, _) = amy_b
        .put(
            &format!("/items/{item}/annotation"),
            json!({"annotatorId": admin_annotator, "values": values}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "cannot label as someone else");
    assert_eq!(
        amy_b
            .get(&format!("/projects/{pid}/next?annotator={admin_annotator}"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        amy_b
            .post("/projects", json!({"name": "x", "templateId": "sentiment"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        amy_b.get(&format!("/projects/{pid}/export?format=csv")).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(amy_b.get("/users").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        amy_b
            .call(Method::PATCH, &format!("/items/{item}"), Some(json!({"hidden": true})))
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    // 審核者：可以匯出、隱藏資料，不能改專案、管理帳號
    let mut rita = Browser::new(&base);
    rita.login("rita", "rita-password").await;
    let resp = reqwest::Client::new()
        .get(format!("{base}/projects/{pid}/export?format=csv"))
        .header(header::COOKIE, rita.cookie.clone().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.text().await.unwrap().contains("hello"));
    assert_eq!(
        rita.call(Method::PATCH, &format!("/items/{item}"), Some(json!({"hidden": false})))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        rita.put(&format!("/projects/{pid}"), json!({"name": "改名"})).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(rita.get("/users").await.0, StatusCode::FORBIDDEN);

    // 管理員停用 amy：她已經登入的裝置也立刻失效；不能停用自己
    let amy_id = amy["id"].as_i64().unwrap();
    let (s, u) = a.put(&format!("/users/{amy_id}"), json!({"disabled": true})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(u["disabled"], true);
    assert_eq!(amy_b.get("/projects").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        Browser::new(&base).login("amy", "amy-password").await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, me) = a.get("/auth/me").await;
    let my_id = me["user"]["id"].as_i64().unwrap();
    assert_eq!(
        a.put(&format!("/users/{my_id}"), json!({"disabled": true})).await.0,
        StatusCode::BAD_REQUEST
    );
    let (_, users) = a.get("/users").await;
    assert_eq!(users.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn cross_site_writes_are_rejected() {
    let dir = TempDir::new().unwrap();
    let base = serve(config(dir.path(), ADMIN_PASSWORD)).await;
    let a = admin(&base).await;
    let client = reqwest::Client::new();
    let send = |origin: Option<&str>| {
        let mut req = client
            .post(format!("{base}/projects"))
            .header(header::COOKIE, a.cookie.clone().unwrap())
            .json(&json!({"name": "csrf", "templateId": "sentiment"}));
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        req.send()
    };
    assert_eq!(
        send(Some("https://evil.example")).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(Some("https://label.example.com")).await.unwrap().status(),
        StatusCode::CREATED,
        "PUBLIC_URL origin"
    );
    assert_eq!(
        send(None).await.unwrap().status(),
        StatusCode::CREATED,
        "command-line clients send no Origin"
    );
    // 登入本身也檢查來源
    let resp = client
        .post(format!("{base}/auth/login"))
        .header(header::ORIGIN, "https://evil.example")
        .json(&json!({"username": "admin", "password": ADMIN_PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn failed_logins_are_rate_limited() {
    let dir = TempDir::new().unwrap();
    let base = serve(config(dir.path(), ADMIN_PASSWORD)).await;
    let mut b = Browser::new(&base);
    for _ in 0..5 {
        assert_eq!(b.login("admin", "wrong-password").await.0, StatusCode::UNAUTHORIZED);
    }
    // 第 6 次就算密碼對了也先擋下
    let (s, err) = b.login("admin", ADMIN_PASSWORD).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{err}");
    assert!(err["error"].as_str().unwrap().contains("分鐘後再試"));
    // 其他帳號不受影響
    assert_eq!(b.login("someone", "whatever-pass").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_password_comes_from_the_environment_on_every_start() {
    let dir = TempDir::new().unwrap();
    serve(config(dir.path(), ADMIN_PASSWORD)).await;
    // 忘記密碼：改 ADMIN_PASSWORD 重新啟動，就以新的為準
    let base = serve(config(dir.path(), "a-brand-new-password")).await;
    let mut b = Browser::new(&base);
    assert_eq!(b.login("admin", ADMIN_PASSWORD).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(b.login("admin", "a-brand-new-password").await.0, StatusCode::OK);
}
