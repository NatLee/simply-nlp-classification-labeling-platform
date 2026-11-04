//! 匯入資料：CSV、JSONL（或 JSON 陣列）、zip（圖片／音訊，可以附一個清單檔）。
//!
//! 每種格式都先轉成同一種 [`Record`]，全部檢查過才寫進資料庫；只要有一列有問題，整批都不匯入，
//! 回傳每一列的錯誤，使用者改好再上傳一次就好，不會留下匯入一半的資料。
//!
//! 欄位（CSV 的欄名或 JSONL 的鍵）：
//! - `text`：文字（舊範本的 `Text` 也可以）。
//! - `image`／`audio`／`media`／`file_name`：zip 裡的檔案路徑，或 `data:` 開頭的 base64 網址。
//! - `annotator`：這一列標注的標注者名字；沒有就是不具名的標注。
//! - `hidden`：true 時不出現在標注佇列。
//! - 和標注欄位 id 一樣的欄位：這一列的標注值（JSONL 也可以用 `annotations` 陣列放多筆）。
//! - 其他欄位都存進 `meta`，匯出時原樣帶出。

use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read},
};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    db::now,
    error::{AppError, AppResult},
    legacy, media,
    models::{self, NewAnnotation, NewItem, Project, Source},
    schema::{FieldKind, Schema, Values},
    state::AppState,
};

/// 錯誤太多時只回傳前面這幾筆。
const MAX_REPORTED_ERRORS: usize = 30;
/// zip 裡最多幾個檔案。
const MAX_ZIP_ENTRIES: usize = 100_000;
/// 解壓縮後的總大小最多是上傳大小上限的幾倍（擋 zip bomb）。
const MAX_UNZIPPED_FACTOR: usize = 5;
/// 被當成媒體路徑的欄名，依優先順序。
const MEDIA_KEYS: &[&str] = &["image", "audio", "media", "file_name"];
/// 匯出檔才有、匯入時忽略的欄位。
const IGNORED_KEYS: &[&str] = &[
    "id",
    "item_id",
    "created_at",
    "updated_at",
    "n_annotations",
    "consensus",
    "source",
];
/// zip 裡優先當成清單檔的檔名。
const MANIFEST_NAMES: &[&str] = &["items.jsonl", "metadata.jsonl", "items.csv", "metadata.csv"];

#[derive(Debug, Clone)]
pub enum MediaRef {
    /// zip 裡的路徑
    Path(String),
    /// data URL 解出來的內容
    Inline { mime: Option<String>, bytes: Vec<u8> },
}

#[derive(Debug, Clone)]
pub struct RecordLabel {
    pub annotator: Option<String>,
    pub values: Values,
}

#[derive(Debug, Clone)]
pub struct Record {
    /// 第幾列（CSV 不含標題列，從 1 開始）或第幾行（JSONL）
    pub row: usize,
    pub text: String,
    pub media: Option<MediaRef>,
    pub meta: Map<String, Value>,
    pub hidden: bool,
    pub labels: Vec<RecordLabel>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub format: String,
    pub rows: usize,
    pub created: usize,
    /// 專案裡原本就有的資料（不重複新增，帶的標注也略過）
    pub duplicates: usize,
    pub annotations: usize,
    pub media: usize,
}

#[derive(Debug, Serialize)]
pub struct RowError {
    pub row: usize,
    pub message: String,
}

fn row_errors(errors: Vec<RowError>) -> AppError {
    let total = errors.len();
    let details: Vec<String> = errors
        .into_iter()
        .take(MAX_REPORTED_ERRORS)
        .map(|e| {
            if e.row == 0 {
                e.message
            } else {
                format!("第 {} 列：{}", e.row, e.message)
            }
        })
        .collect();
    let more = if total > MAX_REPORTED_ERRORS {
        format!("（只列出前 {MAX_REPORTED_ERRORS} 個）")
    } else {
        String::new()
    };
    AppError::invalid(format!("有 {total} 個問題，這次沒有匯入任何資料{more}"), details)
}

/// 匯入一個上傳的檔案。格式看檔名和檔頭。
pub async fn import_file(
    state: &AppState,
    project: &Project,
    filename: &str,
    bytes: Vec<u8>,
) -> AppResult<ImportReport> {
    let lower = filename.to_lowercase();
    let is_zip = bytes.starts_with(b"PK\x03\x04") || lower.ends_with(".zip");
    if is_zip {
        let (records, files, format) = read_zip(&bytes, &project.schema, state.config.max_upload_bytes)?;
        apply(state, project, records, &format, |path| files.get(path).cloned()).await
    } else {
        let text = decode_text(&bytes)?;
        let (records, format) = if lower.ends_with(".jsonl") || lower.ends_with(".ndjson") || lower.ends_with(".json") {
            (
                parse_json_records(&text, &project.schema)?,
                if lower.ends_with(".json") { "json" } else { "jsonl" },
            )
        } else {
            (parse_csv(&text, &project.schema)?, "csv")
        };
        apply(state, project, records, format, |_| None).await
    }
}

fn decode_text(bytes: &[u8]) -> AppResult<String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes); // Excel 存的 UTF-8 會帶 BOM
    String::from_utf8(bytes.to_vec())
        .map_err(|_| AppError::bad("檔案不是 UTF-8 編碼。用 Excel 的話，請選「CSV UTF-8」格式另存。"))
}

// ───────────────────────────── 欄位值 ─────────────────────────────

enum Role {
    Text,
    Media,
    Annotator,
    Hidden,
    Field,
    Ignored,
    Meta,
}

fn role(key: &str, schema: &Schema) -> Role {
    match key {
        "text" => Role::Text,
        k if MEDIA_KEYS.contains(&k) => Role::Media,
        "annotator" => Role::Annotator,
        "hidden" => Role::Hidden,
        k if schema.field(k).is_some() => Role::Field,
        k if IGNORED_KEYS.contains(&k) || k == "annotations" || k == "meta" => Role::Ignored,
        _ => Role::Meta,
    }
}

/// CSV 儲存格（字串）→ 欄位值。
fn cell_value(schema: &Schema, field_id: &str, cell: &str) -> Result<Option<Value>, String> {
    let cell = cell.trim();
    if cell.is_empty() {
        return Ok(None);
    }
    let field = schema.field(field_id).expect("role checked the field exists");
    Ok(Some(match &field.kind {
        FieldKind::Scale { .. } => {
            let x: f64 = cell
                .parse()
                .map_err(|_| format!("「{}」要是數字，收到「{cell}」", field.label))?;
            Value::from(x)
        }
        FieldKind::Multi { .. } | FieldKind::Tags { .. } => {
            Value::from(legacy::parse_list_cell(cell).map_err(|e| format!("「{}」：{e}", field.label))?)
        }
        FieldKind::Single { .. } | FieldKind::Text { .. } => Value::from(cell),
    }))
}

fn truthy(v: &str) -> bool {
    matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "y")
}

fn media_ref(value: &str) -> Result<Option<MediaRef>, String> {
    let v = value.trim();
    if v.is_empty() {
        return Ok(None);
    }
    if v.starts_with("data:") {
        let (mime, bytes) = media::decode_data_url(v).ok_or("data URL 格式不對，要是 data:<mime>;base64,<內容>")?;
        return Ok(Some(MediaRef::Inline { mime, bytes }));
    }
    if v.starts_with("http://") || v.starts_with("https://") {
        return Err("不支援網址，請把圖片和清單一起打包成 zip，或改用 data URL".into());
    }
    Ok(Some(MediaRef::Path(normalize_path(v))))
}

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

// ───────────────────────────── CSV ─────────────────────────────

pub fn parse_csv(text: &str, schema: &Schema) -> AppResult<Vec<Record>> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(text.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| AppError::bad(format!("讀不到 CSV 的標題列：{e}")))?
        .iter()
        .map(|h| legacy::canonical_column(h, schema))
        .collect();
    if !headers.iter().any(|h| h == "text" || MEDIA_KEYS.contains(&h.as_str())) {
        return Err(AppError::bad(format!(
            "CSV 的第一列要有 text 欄位（或 image 等媒體欄位），目前的欄位是：{}。可以下載範本對照。",
            headers.join(", ")
        )));
    }
    let single_column = headers.len() == 1;
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for (i, row) in reader.records().enumerate() {
        let n = i + 1;
        let row = match row {
            Ok(r) => r,
            Err(e) => {
                errors.push(RowError {
                    row: n,
                    message: format!("CSV 格式錯誤：{e}"),
                });
                continue;
            }
        };
        if row.iter().all(|c| c.trim().is_empty()) {
            continue; // 舊版也會略過空行
        }
        let cells: Vec<String> = if single_column {
            vec![legacy::single_column_text(&row)]
        } else {
            row.iter().map(str::to_string).collect()
        };
        let mut rec = Record {
            row: n,
            text: String::new(),
            media: None,
            meta: Map::new(),
            hidden: false,
            labels: vec![],
        };
        let mut values = Values::new();
        let mut annotator = None;
        for (h, cell) in headers.iter().zip(cells.iter()) {
            match role(h, schema) {
                Role::Text => rec.text = cell.trim().to_string(),
                Role::Media => match media_ref(cell) {
                    Ok(Some(m)) if rec.media.is_none() => rec.media = Some(m),
                    Ok(_) => {}
                    Err(e) => errors.push(RowError { row: n, message: e }),
                },
                Role::Annotator => annotator = Some(cell.trim().to_string()).filter(|s| !s.is_empty()),
                Role::Hidden => rec.hidden = truthy(cell),
                Role::Field => match cell_value(schema, h, cell) {
                    Ok(Some(v)) => {
                        values.insert(h.clone(), v);
                    }
                    Ok(None) => {}
                    Err(e) => errors.push(RowError { row: n, message: e }),
                },
                Role::Ignored => {}
                Role::Meta => {
                    if !cell.is_empty() {
                        rec.meta.insert(h.clone(), Value::from(cell.as_str()));
                    }
                }
            }
        }
        if !values.is_empty() {
            rec.labels.push(RecordLabel { annotator, values });
        }
        records.push(rec);
    }
    if errors.is_empty() {
        Ok(records)
    } else {
        Err(row_errors(errors))
    }
}

// ───────────────────────────── JSONL ─────────────────────────────

pub fn parse_json_records(text: &str, schema: &Schema) -> AppResult<Vec<Record>> {
    let trimmed = text.trim_start();
    let objects: Vec<(usize, Result<Value, String>)> = if trimmed.starts_with('[') {
        match serde_json::from_str::<Vec<Value>>(trimmed) {
            Ok(list) => list.into_iter().enumerate().map(|(i, v)| (i + 1, Ok(v))).collect(),
            Err(e) => return Err(AppError::bad(format!("JSON 格式錯誤：{e}"))),
        }
    } else {
        text.lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| {
                (
                    i + 1,
                    serde_json::from_str(l).map_err(|e| format!("JSON 格式錯誤：{e}")),
                )
            })
            .collect()
    };
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for (n, parsed) in objects {
        match parsed.and_then(|v| json_record(n, v, schema)) {
            Ok(r) => records.push(r),
            Err(e) => errors.push(RowError { row: n, message: e }),
        }
    }
    if errors.is_empty() {
        Ok(records)
    } else {
        Err(row_errors(errors))
    }
}

fn json_record(n: usize, v: Value, schema: &Schema) -> Result<Record, String> {
    let Value::Object(obj) = v else {
        return Err("每一行要是一個 JSON 物件".into());
    };
    let mut rec = Record {
        row: n,
        text: String::new(),
        media: None,
        meta: Map::new(),
        hidden: false,
        labels: vec![],
    };
    let mut flat = Values::new();
    let mut annotator = None;
    for (key, value) in obj {
        let key = legacy::canonical_column(&key, schema);
        match role(&key, schema) {
            Role::Text => rec.text = value.as_str().ok_or("text 要是文字")?.trim().to_string(),
            Role::Media => {
                if rec.media.is_some() || value.is_null() {
                    continue;
                }
                // 匯出檔的 media 是 {"file": "media/xx.png", ...}
                let path = match &value {
                    Value::String(s) => s.clone(),
                    Value::Object(o) => o
                        .get("file")
                        .and_then(Value::as_str)
                        .ok_or("media 物件要有 file")?
                        .to_string(),
                    _ => return Err(format!("{key} 要是檔案路徑或 data URL")),
                };
                rec.media = media_ref(&path)?;
            }
            Role::Annotator => annotator = value.as_str().map(str::to_string).filter(|s| !s.trim().is_empty()),
            Role::Hidden => rec.hidden = value.as_bool().unwrap_or(false),
            Role::Field => {
                flat.insert(key, value);
            }
            Role::Ignored if key == "meta" => {
                if let Value::Object(m) = value {
                    rec.meta.extend(m);
                }
            }
            Role::Ignored if key == "annotations" => {
                for a in value.as_array().ok_or("annotations 要是陣列")? {
                    let values = a
                        .get("values")
                        .and_then(Value::as_object)
                        .ok_or("annotations 裡每一筆都要有 values 物件")?
                        .clone();
                    let who = a.get("annotator").and_then(Value::as_str).map(str::to_string);
                    rec.labels.push(RecordLabel { annotator: who, values });
                }
            }
            Role::Ignored => {}
            Role::Meta => {
                rec.meta.insert(key, value);
            }
        }
    }
    if !flat.is_empty() {
        rec.labels.push(RecordLabel {
            annotator,
            values: flat,
        });
    }
    Ok(rec)
}

// ───────────────────────────── zip ─────────────────────────────

type ZipFiles = HashMap<String, Vec<u8>>;

fn read_zip(bytes: &[u8], schema: &Schema, max_upload: usize) -> AppResult<(Vec<Record>, ZipFiles, String)> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| AppError::bad(format!("zip 檔打不開：{e}")))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(AppError::bad(format!("zip 裡的檔案太多（最多 {MAX_ZIP_ENTRIES} 個）")));
    }
    let budget = max_upload.saturating_mul(MAX_UNZIPPED_FACTOR);
    let mut used = 0usize;
    let mut files = ZipFiles::new();
    for i in 0..archive.len() {
        let mut f = archive
            .by_index(i)
            .map_err(|e| AppError::bad(format!("zip 檔損壞：{e}")))?;
        if f.is_dir() {
            continue;
        }
        let Some(name) = f.enclosed_name().map(|p| normalize_path(&p.to_string_lossy())) else {
            continue;
        };
        // macOS 壓縮檔會多出這些
        if name.starts_with("__MACOSX/")
            || name
                .rsplit('/')
                .next()
                .is_some_and(|b| b.starts_with("._") || b == ".DS_Store")
        {
            continue;
        }
        used = used.saturating_add(f.size() as usize);
        if used > budget {
            return Err(AppError::PayloadTooLarge(format!(
                "zip 解開後太大（上限 {} MB）",
                budget / 1024 / 1024
            )));
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .map_err(|e| AppError::bad(format!("讀不到 zip 裡的 {name}：{e}")))?;
        files.insert(name, buf);
    }

    let manifest = MANIFEST_NAMES
        .iter()
        .find_map(|m| files.keys().find(|k| k.rsplit('/').next() == Some(*m)).cloned())
        .or_else(|| {
            let mut candidates: Vec<&String> = files
                .keys()
                .filter(|k| k.ends_with(".jsonl") || k.ends_with(".csv"))
                .collect();
            candidates.sort();
            (candidates.len() == 1).then(|| candidates[0].clone())
        });

    let (mut records, format) = match &manifest {
        Some(path) => {
            let text = decode_text(&files[path])?;
            let records = if path.ends_with(".csv") {
                parse_csv(&text, schema)?
            } else {
                parse_json_records(&text, schema)?
            };
            // 清單裡的路徑相對於清單檔所在的資料夾
            let base = path.rsplit_once('/').map(|(d, _)| format!("{d}/")).unwrap_or_default();
            let records: Vec<Record> = records
                .into_iter()
                .map(|mut r| {
                    if let Some(MediaRef::Path(p)) = &r.media {
                        let joined = format!("{base}{p}");
                        if !base.is_empty() && files.contains_key(&joined) {
                            r.media = Some(MediaRef::Path(joined));
                        }
                    }
                    r
                })
                .collect();
            (records, format!("zip+{}", path.rsplit('/').next().unwrap_or(path)))
        }
        None => {
            // 沒有清單：每個媒體檔一筆；同名的 .txt 當作文字（例如 cat.jpg + cat.txt）
            let mut names: Vec<&String> = files.keys().filter(|k| media::is_media_name(k)).collect();
            names.sort();
            let records = names
                .into_iter()
                .enumerate()
                .map(|(i, name)| {
                    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
                    let text = files
                        .get(&format!("{stem}.txt"))
                        .and_then(|b| String::from_utf8(b.clone()).ok())
                        .map(|t| t.trim().to_string())
                        .unwrap_or_default();
                    let mut meta = Map::new();
                    meta.insert("file_name".into(), Value::from(name.as_str()));
                    Record {
                        row: i + 1,
                        text,
                        media: Some(MediaRef::Path(name.clone())),
                        meta,
                        hidden: false,
                        labels: vec![],
                    }
                })
                .collect();
            (records, "zip".to_string())
        }
    };
    if records.is_empty() {
        return Err(AppError::bad(
            "zip 裡沒有找到圖片、音訊或清單檔（items.jsonl、metadata.jsonl、*.csv）",
        ));
    }
    records.sort_by_key(|r| r.row);
    Ok((records, files, format))
}

// ───────────────────────────── 寫入 ─────────────────────────────

struct Prepared {
    record: Record,
    labels: Vec<(Option<String>, Values)>,
    media: Option<(Vec<u8>, String, Option<String>)>,
}

async fn apply(
    state: &AppState,
    project: &Project,
    records: Vec<Record>,
    format: &str,
    zip_file: impl Fn(&str) -> Option<Vec<u8>>,
) -> AppResult<ImportReport> {
    let schema = &project.schema;
    let mut errors = Vec::new();
    let mut prepared = Vec::new();
    for rec in records {
        let mut ok = true;
        if rec.text.is_empty() && rec.media.is_none() {
            errors.push(RowError {
                row: rec.row,
                message: "沒有文字也沒有媒體檔".into(),
            });
            continue;
        }
        let media = match &rec.media {
            None => None,
            Some(MediaRef::Inline { mime, bytes }) => Some((bytes.clone(), "inline".to_string(), mime.clone())),
            Some(MediaRef::Path(p)) => match zip_file(p) {
                Some(bytes) => Some((bytes, p.clone(), None)),
                None => {
                    let msg = if format.starts_with("zip") {
                        format!("zip 裡找不到「{p}」")
                    } else {
                        format!("「{p}」是檔案路徑：要匯入圖片，請把清單和圖片一起打包成 zip")
                    };
                    errors.push(RowError {
                        row: rec.row,
                        message: msg,
                    });
                    continue;
                }
            },
        };
        if let Some((bytes, name, mime)) = &media {
            if media::detect(bytes, name, mime.as_deref()).is_none() {
                errors.push(RowError {
                    row: rec.row,
                    message: format!("「{name}」不是支援的圖片或音訊格式"),
                });
                ok = false;
            }
        }
        let mut labels = Vec::new();
        for l in &rec.labels {
            match schema.validate_values(&l.values) {
                Ok(v) if v.is_empty() => {}
                Ok(v) => labels.push((l.annotator.clone().filter(|a| !a.trim().is_empty()), v)),
                Err(e) => {
                    errors.push(RowError {
                        row: rec.row,
                        message: e.join("；"),
                    });
                    ok = false;
                }
            }
        }
        if ok {
            prepared.push(Prepared {
                record: rec,
                labels,
                media,
            });
        }
    }
    if !errors.is_empty() {
        return Err(row_errors(errors));
    }

    let mut report = ImportReport {
        format: format.to_string(),
        rows: prepared.len(),
        ..Default::default()
    };
    let media_dir = state.config.media_dir();
    let t = now();
    let mut tx = crate::db::write_tx(&state.db).await?;
    // 同一次匯入裡相同的資料（例如每位標注者一列的 CSV）合併成一筆，標注都掛上去
    let mut created_in_this_import: HashMap<String, i64> = HashMap::new();
    let mut named_pairs: HashSet<(i64, String)> = HashSet::new();
    for p in prepared {
        let (media_id, media_sha) = match &p.media {
            Some((bytes, name, mime)) => {
                let (id, sha) = media::store(&mut tx, &media_dir, bytes, name, mime.as_deref())
                    .await
                    .map_err(|e| AppError::invalid("媒體檔無法儲存", vec![format!("第 {} 列：{e:#}", p.record.row)]))?;
                report.media += 1;
                (Some(id), Some(sha))
            }
            None => (None, None),
        };
        let key = models::dedupe_key(&p.record.text, media_sha.as_deref());
        let item_id = match created_in_this_import.get(&key) {
            Some(&id) => id,
            None => {
                let item = NewItem {
                    text: p.record.text.clone(),
                    media_id,
                    media_sha,
                    meta: Value::Object(p.record.meta.clone()),
                    hidden: p.record.hidden,
                    created_at: t,
                };
                match models::insert_item(&mut tx, project.id, &item).await? {
                    Some(id) => {
                        report.created += 1;
                        created_in_this_import.insert(key, id);
                        id
                    }
                    None => {
                        report.duplicates += 1;
                        continue;
                    }
                }
            }
        };
        for (who, values) in p.labels {
            let annotator_id = match &who {
                Some(name) => {
                    if !named_pairs.insert((item_id, name.to_lowercase())) {
                        return Err(AppError::invalid(
                            "同一筆資料有兩列是同一位標注者",
                            vec![format!("第 {} 列：「{name}」已經標過這筆資料", p.record.row)],
                        ));
                    }
                    Some(models::annotator_by_name(&mut tx, name, "human").await?)
                }
                None => None,
            };
            models::insert_annotation(
                &mut tx,
                &NewAnnotation {
                    item_id,
                    annotator_id,
                    values,
                    source: Source::Import,
                    suggestion_id: None,
                    at: t,
                },
            )
            .await?;
            report.annotations += 1;
        }
    }
    tx.commit().await?;
    Ok(report)
}

/// 匯入範本：標題列加一列範例，欄位依專案的標注結構產生。
pub fn template_csv(schema: &Schema) -> String {
    let mut w = csv::Writer::from_writer(Vec::new());
    let mut header = vec!["text".to_string()];
    let mut example = vec!["今天的晚餐超好吃，下次還要再來！".to_string()];
    for f in &schema.fields {
        header.push(f.id.clone());
        example.push(match &f.kind {
            FieldKind::Scale { default, min, .. } => default.unwrap_or(*min).to_string(),
            FieldKind::Single { options, default } => default.clone().unwrap_or_else(|| options[0].value.clone()),
            FieldKind::Multi { options, .. } => serde_json::to_string(&[&options[0].value]).unwrap_or_default(),
            FieldKind::Tags { .. } => "[\"美食\"]".into(),
            FieldKind::Text { .. } => String::new(),
        });
    }
    let _ = w.write_record(&header);
    let _ = w.write_record(&example);
    String::from_utf8(w.into_inner().unwrap_or_default()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::legacy_emotion_schema;
    use serde_json::json;

    #[test]
    fn legacy_text_csv_keeps_commas_like_the_old_line_reader() {
        let s = legacy_emotion_schema();
        let recs = parse_csv("Text\n今天,天氣很好\n\n\"有引號, 也可以\"\n", &s).unwrap();
        let texts: Vec<_> = recs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["今天,天氣很好", "有引號, 也可以"]);
        assert!(recs.iter().all(|r| r.labels.is_empty()));
    }

    #[test]
    fn legacy_label_csv_maps_to_fields() {
        let s = legacy_emotion_schema();
        let csv = "Level,Classes,Custom classes,Text\n3,happy,\"[\"\"城市\"\", \"\"政治\"\"]\",這城市有希望了\n4,angry,,不要臉的政客\n";
        let recs = parse_csv(csv, &s).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].text, "這城市有希望了");
        assert_eq!(
            Value::Object(recs[0].labels[0].values.clone()),
            json!({"score": 3.0, "tag": "happy", "tag_opt": ["城市", "政治"]})
        );
        assert_eq!(
            Value::Object(recs[1].labels[0].values.clone()),
            json!({"score": 4.0, "tag": "angry"})
        );
    }

    #[test]
    fn csv_errors_are_reported_per_row() {
        let s = legacy_emotion_schema();
        let err = parse_csv("text,score,tag_opt\na,x,[]\nb,3,not-a-list\n", &s).unwrap_err();
        let AppError::BadRequest(_, details) = err else {
            panic!()
        };
        assert!(details[0].starts_with("第 1 列"));
        assert!(details[1].starts_with("第 2 列"));
        assert!(parse_csv("foo,bar\n1,2\n", &s).is_err());
    }

    #[test]
    fn jsonl_records() {
        let s = legacy_emotion_schema();
        let text = r#"{"text": "a", "score": 2, "tag": "sad", "annotator": "amy", "source_id": 9}
{"text": "b", "media": {"file": "media/x.png"}, "annotations": [{"annotator": "bob", "values": {"score": 5}}, {"annotator": null, "values": {"tag": "wow"}}], "meta": {"k": 1}}
"#;
        let recs = parse_json_records(text, &s).unwrap();
        assert_eq!(recs[0].labels[0].annotator.as_deref(), Some("amy"));
        assert_eq!(recs[0].meta["source_id"], json!(9));
        assert!(matches!(&recs[1].media, Some(MediaRef::Path(p)) if p == "media/x.png"));
        assert_eq!(recs[1].labels.len(), 2);
        assert_eq!(recs[1].meta["k"], json!(1));
        let err = parse_json_records("{\"text\": 1}\nnot json\n", &s).unwrap_err();
        let AppError::BadRequest(_, details) = err else {
            panic!()
        };
        assert_eq!(details.len(), 2);
    }

    #[test]
    fn urls_are_rejected_and_data_urls_decoded() {
        assert!(media_ref("https://example.com/a.png").is_err());
        assert!(matches!(
            media_ref("data:image/png;base64,aGk=").unwrap(),
            Some(MediaRef::Inline { .. })
        ));
        assert!(matches!(media_ref(".\\img\\a.png").unwrap(), Some(MediaRef::Path(p)) if p == "img/a.png"));
    }

    #[test]
    fn template_round_trips_through_the_parser() {
        for t in crate::schema::templates() {
            let csv = template_csv(&t.schema);
            let recs = parse_csv(&csv, &t.schema).unwrap();
            assert_eq!(recs.len(), 1, "{}", t.id);
            t.schema.validate_values(&recs[0].labels[0].values).unwrap();
        }
    }
}
