use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    ai,
    db::now,
    error::{AppError, AppResult},
    export, import, legacy, media,
    models::{self, AiSettings, ItemFilter, Project},
    schema::{self, Schema},
    state::AppState,
    stats,
};

/// 內建的範例檔（`examples/` 資料夾），可以從匯入頁下載。
const SAMPLES: &[(&str, &str, &[u8])] = &[
    (
        "textDataExample.csv",
        "text/csv; charset=utf-8",
        include_bytes!("../../../../examples/textDataExample.csv"),
    ),
    (
        "labelDataExample.csv",
        "text/csv; charset=utf-8",
        include_bytes!("../../../../examples/labelDataExample.csv"),
    ),
    (
        "sentiment-sample.jsonl",
        "application/x-ndjson; charset=utf-8",
        include_bytes!("../../../../examples/sentiment-sample.jsonl"),
    ),
    (
        "image-text-sample.zip",
        "application/zip",
        include_bytes!("../../../../examples/image-text-sample.zip"),
    ),
];

pub async fn sample(Path(name): Path<String>) -> AppResult<Response> {
    let (_, mime, bytes) = SAMPLES
        .iter()
        .find(|(n, ..)| *n == name)
        .ok_or_else(|| AppError::not_found("沒有這個範例檔"))?;
    Ok(download(&name, mime, bytes.to_vec()))
}

fn download(filename: &str, content_type: &str, bytes: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

// ───────────────────────────── 專案 ─────────────────────────────

pub async fn list(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let projects = models::list_projects(&state.db).await?;
    let counts: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT i.project_id, COUNT(*), SUM(i.hidden = 0), \
           SUM(i.hidden = 0 AND EXISTS (SELECT 1 FROM annotations a WHERE a.item_id = i.id)) \
         FROM items i GROUP BY i.project_id",
    )
    .fetch_all(&state.db)
    .await?;
    let out: Vec<Value> = projects
        .into_iter()
        .map(|p| {
            let (_, total, visible, labeled) = counts.iter().find(|c| c.0 == p.id).copied().unwrap_or((p.id, 0, 0, 0));
            let mut v = serde_json::to_value(&p).unwrap_or_default();
            v["counts"] = json!({ "items": total, "visible": visible, "labeled": labeled });
            v
        })
        .collect();
    Ok(Json(Value::from(out)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProject {
    name: String,
    #[serde(default)]
    description: String,
    /// 用範本建立；和 `schema` 二選一。
    template_id: Option<String>,
    schema: Option<Schema>,
    ai: Option<AiSettings>,
}

fn check_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::bad("專案名稱不能是空的，最多 100 個字"));
    }
    Ok(name.to_string())
}

fn check_schema(schema: &Schema) -> AppResult<()> {
    schema.validate().map_err(|e| AppError::invalid("標注結構有問題", e))
}

fn check_ai(ai: &AiSettings) -> AppResult<()> {
    if let Some(t) = ai.auto_accept_threshold {
        if !(0.0..=1.0).contains(&t) {
            return Err(AppError::bad("自動接受的門檻要介於 0 和 1"));
        }
    }
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateProject>,
) -> AppResult<(StatusCode, Json<Project>)> {
    let name = check_name(&req.name)?;
    let template = req
        .template_id
        .as_deref()
        .map(|id| {
            schema::templates()
                .into_iter()
                .find(|t| t.id == id)
                .ok_or_else(|| AppError::bad(format!("沒有「{id}」這個範本")))
        })
        .transpose()?;
    let schema = match (req.schema, &template) {
        (Some(s), _) => s,
        (None, Some(t)) => t.schema.clone(),
        (None, None) => return Err(AppError::bad("請選一個範本，或提供標注結構")),
    };
    check_schema(&schema)?;
    let ai = req.ai.unwrap_or_else(|| AiSettings {
        instructions: template
            .as_ref()
            .map(|t| t.instructions.to_string())
            .unwrap_or_default(),
        auto_accept_threshold: None,
    });
    check_ai(&ai)?;
    let mut tx = crate::db::write_tx(&state.db).await?;
    let id = models::insert_project(&mut tx, &name, req.description.trim(), &schema, &ai).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(models::get_project(&state.db, id).await?)))
}

pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Project>> {
    Ok(Json(models::get_project(&state.db, id).await?))
}

#[derive(Deserialize)]
pub struct UpdateProject {
    name: Option<String>,
    description: Option<String>,
    schema: Option<Schema>,
    ai: Option<AiSettings>,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateProject>,
) -> AppResult<Json<Project>> {
    let mut p = models::get_project(&state.db, id).await?;
    if let Some(name) = req.name {
        p.name = check_name(&name)?;
    }
    if let Some(d) = req.description {
        p.description = d.trim().to_string();
    }
    if let Some(s) = req.schema {
        check_schema(&s)?;
        // 結構可以改，但不能讓已經存在的標注變成不合法
        let existing = models::project_annotations(&state.db, id).await?;
        if let Some(why) = s.first_incompatible(existing.iter().map(|a| &a.values)) {
            return Err(AppError::Conflict(format!(
                "這樣改會讓已經有的標注不合法：{why}。請先保留舊的選項或欄位。"
            )));
        }
        p.schema = s;
    }
    if let Some(ai) = req.ai {
        check_ai(&ai)?;
        p.ai = ai;
    }
    sqlx::query("UPDATE projects SET name = ?, description = ?, schema = ?, ai = ?, updated_at = ? WHERE id = ?")
        .bind(&p.name)
        .bind(&p.description)
        .bind(serde_json::to_string(&p.schema)?)
        .bind(serde_json::to_string(&p.ai)?)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(models::get_project(&state.db, id).await?))
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    models::get_project(&state.db, id).await?;
    if state.jobs.cancel(id) {
        tracing::info!("刪除專案 {id}，已停止它的批次預標");
    }
    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    let removed = media::remove_orphans(&state.db, &state.config.media_dir()).await?;
    tracing::info!("刪除專案 {id}，清掉 {removed} 個沒有用到的媒體檔");
    Ok(StatusCode::NO_CONTENT)
}

// ───────────────────────────── 資料、佇列、統計 ─────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemsQuery {
    #[serde(default)]
    filter: ItemFilter,
    q: Option<String>,
    annotator: Option<i64>,
    #[serde(default = "first_page")]
    page: i64,
    #[serde(default = "default_per_page")]
    per_page: i64,
}

fn first_page() -> i64 {
    1
}
fn default_per_page() -> i64 {
    50
}
const MAX_PER_PAGE: i64 = 200;

pub async fn items(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<ItemsQuery>,
) -> AppResult<Json<Value>> {
    models::get_project(&state.db, id).await?;
    let per_page = q.per_page.clamp(1, MAX_PER_PAGE);
    let page = q.page.max(1);
    let search = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let (items, total) = models::list_items(
        &state.db,
        id,
        q.filter,
        q.annotator,
        search,
        per_page,
        (page - 1) * per_page,
    )
    .await?;
    Ok(Json(
        json!({ "items": items, "total": total, "page": page, "perPage": per_page }),
    ))
}

#[derive(Deserialize)]
pub struct NextQuery {
    annotator: i64,
    /// 剛剛跳過的資料 id，用逗號分隔。
    #[serde(default)]
    exclude: String,
}

pub async fn next(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<NextQuery>,
) -> AppResult<Json<Value>> {
    models::get_project(&state.db, id).await?;
    models::get_annotator(&state.db, q.annotator).await?;
    let exclude: Vec<i64> = q.exclude.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    Ok(Json(
        json!({ "itemId": stats::next_item(&state.db, id, q.annotator, &exclude).await? }),
    ))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    annotator: Option<i64>,
}

pub async fn stats(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<StatsQuery>,
) -> AppResult<Json<stats::Stats>> {
    let p = models::get_project(&state.db, id).await?;
    Ok(Json(stats::project_stats(&state.db, &p, q.annotator).await?))
}

// ───────────────────────────── 匯入匯出 ─────────────────────────────

/// 讀出 multipart 裡的檔案和其他文字欄位。
async fn read_upload(mut form: Multipart) -> AppResult<(String, Vec<u8>, std::collections::HashMap<String, String>)> {
    let mut file = None;
    let mut fields = std::collections::HashMap::new();
    while let Some(field) = form.next_field().await.map_err(|e| upload_error(&e.to_string()))? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            let filename = field.file_name().unwrap_or("upload").to_string();
            let bytes = field.bytes().await.map_err(|e| upload_error(&e.to_string()))?;
            file = Some((filename, bytes.to_vec()));
        } else {
            fields.insert(name, field.text().await.unwrap_or_default());
        }
    }
    let (filename, bytes) = file.ok_or_else(|| AppError::bad("沒有收到檔案（表單欄位名稱要是 file）"))?;
    if bytes.is_empty() {
        return Err(AppError::bad("檔案是空的"));
    }
    Ok((filename, bytes, fields))
}

fn upload_error(msg: &str) -> AppError {
    if msg.contains("length limit") || msg.contains("too large") {
        AppError::PayloadTooLarge("檔案太大，可以用 LABEL_MAX_UPLOAD_MB 調高上限".into())
    } else {
        AppError::bad(format!("上傳失敗：{msg}"))
    }
}

pub async fn import(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    form: Multipart,
) -> AppResult<Json<import::ImportReport>> {
    let p = models::get_project(&state.db, id).await?;
    let (filename, bytes, _) = read_upload(form).await?;
    Ok(Json(import::import_file(&state, &p, &filename, bytes).await?))
}

pub async fn import_template(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Response> {
    let p = models::get_project(&state.db, id).await?;
    Ok(download(
        &format!("project-{id}-template.csv"),
        "text/csv; charset=utf-8",
        import::template_csv(&p.schema).into_bytes(),
    ))
}

pub async fn export(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<export::ExportQuery>,
) -> AppResult<Response> {
    let p = models::get_project(&state.db, id).await?;
    let file = export::export(&state, &p, &q).await?;
    Ok(download(&file.filename, file.content_type, file.bytes))
}

/// 匯入舊版的 emoLabel.db，建立一個新專案。
pub async fn import_legacy(
    State(state): State<AppState>,
    form: Multipart,
) -> AppResult<(StatusCode, Json<legacy::LegacyImport>)> {
    let (filename, bytes, fields) = read_upload(form).await?;
    if !bytes.starts_with(b"SQLite format 3\0") {
        return Err(AppError::bad("這不是 SQLite 資料庫檔（舊版的 emoLabel.db）"));
    }
    let name = fields
        .get("name")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("舊版資料（{filename}）"));
    let name = check_name(&name)?;
    // 瀏覽器傳來的時區（分鐘），舊版存的是沒有時區的當地時間
    let offset: i32 = fields
        .get("utcOffsetMinutes")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let tmp_dir = state.config.data_dir.join("tmp");
    tokio::fs::create_dir_all(&tmp_dir).await.map_err(anyhow::Error::from)?;
    let tmp = tmp_dir.join(format!("legacy-{}-{}.db", now(), rand::random::<u32>()));
    tokio::fs::write(&tmp, &bytes).await.map_err(anyhow::Error::from)?;
    let result = legacy::import_db(&state.db, &tmp, &name, offset).await;
    let _ = tokio::fs::remove_file(&tmp).await;
    let report = result.map_err(|e| AppError::bad(format!("{e:#}")))?;
    Ok((StatusCode::CREATED, Json(report)))
}

// ───────────────────────────── 批次預標 ─────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    #[serde(default = "default_batch")]
    limit: i64,
    /// 沒給就用專案設定的門檻。
    threshold: Option<f64>,
}

fn default_batch() -> i64 {
    50
}

pub async fn batch_start(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<BatchRequest>,
) -> AppResult<(StatusCode, Json<ai::JobStatus>)> {
    let p = models::get_project(&state.db, id).await?;
    let threshold = req.threshold.or(p.ai.auto_accept_threshold);
    Ok((
        StatusCode::ACCEPTED,
        Json(ai::start_batch(&state, p, req.limit, threshold).await?),
    ))
}

pub async fn batch_status(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({ "job": state.jobs.status(id), "ai": super::ai_status(&state) }),
    ))
}

pub async fn batch_cancel(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    if state.jobs.cancel(id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("沒有進行中的批次預標"))
    }
}
