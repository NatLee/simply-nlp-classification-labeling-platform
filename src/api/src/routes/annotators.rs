use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::{
    db::now,
    error::{AppError, AppResult},
    models::{self, Annotator, AI_ANNOTATOR_PREFIX as AI_PREFIX},
    state::AppState,
};

const MAX_NAME_CHARS: usize = 40;

pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Annotator>>> {
    Ok(Json(models::list_annotators(&state.db).await?))
}

#[derive(Deserialize)]
pub struct NameBody {
    name: String,
}

fn check(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::bad(format!("名字不能是空的，最多 {MAX_NAME_CHARS} 個字")));
    }
    if name.starts_with(AI_PREFIX) {
        return Err(AppError::bad(format!("「{AI_PREFIX}」開頭的名字保留給模型標注者")));
    }
    Ok(name.to_string())
}

async fn name_taken(state: &AppState, name: &str, except: Option<i64>) -> AppResult<bool> {
    let id: Option<i64> = sqlx::query_scalar("SELECT id FROM annotators WHERE name = ?")
        .bind(name)
        .fetch_optional(&state.db)
        .await?;
    Ok(id.is_some_and(|id| Some(id) != except))
}

pub async fn create(
    State(state): State<AppState>,
    Json(b): Json<NameBody>,
) -> AppResult<(StatusCode, Json<Annotator>)> {
    let name = check(&b.name)?;
    if name_taken(&state, &name, None).await? {
        return Err(AppError::Conflict(format!("已經有叫「{name}」的標注者")));
    }
    let id: i64 =
        sqlx::query_scalar("INSERT INTO annotators (name, kind, created_at) VALUES (?, 'human', ?) RETURNING id")
            .bind(&name)
            .bind(now())
            .fetch_one(&state.db)
            .await?;
    Ok((StatusCode::CREATED, Json(models::get_annotator(&state.db, id).await?)))
}

pub async fn rename(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<NameBody>,
) -> AppResult<Json<Annotator>> {
    let current = models::get_annotator(&state.db, id).await?;
    if current.kind != "human" {
        return Err(AppError::bad("模型標注者的名字不能改"));
    }
    let name = check(&b.name)?;
    if name_taken(&state, &name, Some(id)).await? {
        return Err(AppError::Conflict(format!("已經有叫「{name}」的標注者")));
    }
    sqlx::query("UPDATE annotators SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(Json(models::get_annotator(&state.db, id).await?))
}

/// 已經有標注的標注者不能刪，避免標注變成不知道是誰標的。
pub async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    models::get_annotator(&state.db, id).await?;
    let used: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM annotations WHERE annotator_id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if used > 0 {
        return Err(AppError::Conflict(format!("這位標注者已經標了 {used} 筆，不能刪除")));
    }
    sqlx::query("DELETE FROM annotators WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
