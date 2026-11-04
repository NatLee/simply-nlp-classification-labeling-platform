use std::collections::BTreeMap;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{
    ai,
    auth::{Level, Viewer},
    db::now,
    error::{AppError, AppResult},
    models::{self, Annotation, Source},
    schema::{FieldKind, Values},
    state::AppState,
};

/// 自由標籤欄位最多提示幾個常用標籤。
const TAG_HINTS: i64 = 20;

#[derive(Deserialize)]
pub struct DetailQuery {
    annotator: Option<i64>,
}

/// 一筆資料的全部資訊：內容、所有標注、目前標注者自己的標注、最新的 AI 建議、標籤提示。
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<DetailQuery>,
) -> AppResult<Json<Value>> {
    let item = models::get_item(&state.db, id).await?;
    let project = models::get_project(&state.db, item.project_id).await?;
    let annotations = models::item_annotations(&state.db, id).await?;
    let mine = q
        .annotator
        .and_then(|a| annotations.iter().find(|x| x.annotator_id == Some(a)).cloned());
    let suggestion = models::latest_suggestion(&state.db, id).await?;

    // 舊版會列出這筆文字以前被標過的自訂標籤；新版再加上整個專案常用的標籤
    let mut tag_hints = BTreeMap::new();
    for f in project
        .schema
        .fields
        .iter()
        .filter(|f| matches!(f.kind, FieldKind::Tags { .. }))
    {
        let mut hints: Vec<String> = Vec::new();
        let mut push = |t: &str| {
            if !hints.iter().any(|h| h == t) {
                hints.push(t.to_string());
            }
        };
        for a in &annotations {
            for t in a
                .values
                .get(&f.id)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                push(t);
            }
        }
        if let FieldKind::Tags { suggestions } = &f.kind {
            suggestions.iter().for_each(|s| push(s));
        }
        let popular: Vec<String> = sqlx::query_scalar(
            "SELECT j.value FROM annotations a JOIN items i ON i.id = a.item_id, json_each(a.\"values\", '$.' || ?) j \
             WHERE i.project_id = ? GROUP BY j.value ORDER BY COUNT(*) DESC, j.value LIMIT ?",
        )
        .bind(&f.id)
        .bind(project.id)
        .bind(TAG_HINTS)
        .fetch_all(&state.db)
        .await?;
        popular.iter().for_each(|t| push(t));
        tag_hints.insert(f.id.clone(), hints);
    }
    Ok(Json(json!({
        "item": item,
        "annotations": annotations,
        "mine": mine,
        "suggestion": suggestion,
        "tagHints": tag_hints,
    })))
}

#[derive(Deserialize)]
pub struct UpdateItem {
    hidden: Option<bool>,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateItem>,
) -> AppResult<Json<models::Item>> {
    models::get_item(&state.db, id).await?;
    if let Some(h) = req.hidden {
        sqlx::query("UPDATE items SET hidden = ? WHERE id = ?")
            .bind(h)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    Ok(Json(models::get_item(&state.db, id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAnnotation {
    annotator_id: i64,
    values: Values,
    /// 表單是從這筆 AI 建議帶入的。
    suggestion_id: Option<i64>,
}

/// 新增或更新目前標注者對這筆資料的標注（每人每筆一份）。
pub async fn save_annotation(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(id): Path<i64>,
    Json(req): Json<SaveAnnotation>,
) -> AppResult<Json<Annotation>> {
    let annotator_id = viewer.annotator(req.annotator_id)?;
    let item = models::get_item(&state.db, id).await?;
    let project = models::get_project(&state.db, item.project_id).await?;
    let annotator = models::get_annotator(&state.db, annotator_id).await?;
    if annotator.kind != "human" {
        return Err(AppError::bad("AI 標注者的標注只能由批次預標產生"));
    }
    let values = project
        .schema
        .validate_values(&req.values)
        .map_err(|e| AppError::invalid("標注有問題", e))?;
    let (source, status) = match req.suggestion_id {
        Some(sid) => {
            let s = models::get_suggestion(&state.db, sid).await?;
            if s.item_id != id {
                return Err(AppError::bad("這筆 AI 建議不是這筆資料的"));
            }
            (
                Source::AiAssisted,
                Some(if s.values == values { "accepted" } else { "modified" }),
            )
        }
        None => (Source::Human, None),
    };
    let t = now();
    let annotation_id: i64 = sqlx::query_scalar(
        "INSERT INTO annotations (item_id, annotator_id, \"values\", source, suggestion_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (item_id, annotator_id) WHERE annotator_id IS NOT NULL DO UPDATE SET \
           \"values\" = excluded.\"values\", source = excluded.source, suggestion_id = excluded.suggestion_id, \
           updated_at = excluded.updated_at \
         RETURNING id",
    )
    .bind(id)
    .bind(annotator_id)
    .bind(serde_json::to_string(&values)?)
    .bind(source.as_str())
    .bind(req.suggestion_id)
    .bind(t)
    .bind(t)
    .fetch_one(&state.db)
    .await?;
    if let (Some(sid), Some(status)) = (req.suggestion_id, status) {
        models::set_suggestion_status(&state.db, sid, status).await?;
    }
    Ok(Json(models::get_annotation(&state.db, annotation_id).await?))
}

/// 登入後只能刪自己的標注；reviewer 和 admin 可以刪別人的。
pub async fn delete_annotation(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let a = models::get_annotation(&state.db, id).await?;
    if let Some(u) = &viewer.0 {
        if a.annotator_id != Some(u.annotator_id) && !viewer.at_least(Level::Review) {
            return Err(AppError::Forbidden("只能刪自己的標注".into()));
        }
    }
    sqlx::query("DELETE FROM annotations WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn suggest(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<models::Suggestion>> {
    let item = models::get_item(&state.db, id).await?;
    let project = models::get_project(&state.db, item.project_id).await?;
    Ok(Json(ai::suggest(&state, &project, &item).await?))
}

pub async fn reject_suggestion(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<models::Suggestion>> {
    models::get_suggestion(&state.db, id).await?;
    models::set_suggestion_status(&state.db, id, "rejected").await?;
    Ok(Json(models::get_suggestion(&state.db, id).await?))
}

/// 媒體檔。檔名是內容的雜湊，內容不會變，可以長時間快取；支援 Range（音訊拖拉進度條要用）。
pub async fn media(State(state): State<AppState>, Path(id): Path<i64>, req: Request<Body>) -> AppResult<Response> {
    let (path, mime): (String, String) = sqlx::query_as("SELECT path, mime FROM media WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個媒體檔"))?;
    let file = state.config.media_dir().join(path);
    let mut resp = ServeFile::new_with_mime(file, &mime.parse::<mime::Mime>().map_err(anyhow::Error::from)?)
        .oneshot(req)
        .await
        .map_err(anyhow::Error::from)?
        .into_response();
    if resp.status().is_success() {
        let h = resp.headers_mut();
        h.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
        h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    }
    Ok(resp)
}
