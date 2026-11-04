//! 登入、登出、目前的帳號，以及管理員的帳號管理。只有啟用內建登入時才有作用。

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{self, Role, Viewer},
    db::now,
    error::{AppError, AppResult},
    state::AppState,
};

fn auth_config(state: &AppState) -> AppResult<&crate::config::AuthConfig> {
    state
        .config
        .auth
        .as_ref()
        .ok_or_else(|| AppError::not_found("沒有啟用登入（設定 ADMIN_PASSWORD 才會啟用）"))
}

/// `{enabled, user}`：前端靠這個決定要不要顯示登入頁。
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Json<Value>> {
    if state.config.auth.is_none() {
        return Ok(Json(json!({ "enabled": false, "user": null })));
    }
    let user = match auth::token_from(&headers) {
        Some(t) => auth::user_for_token(&state.db, &t).await?,
        None => None,
    };
    Ok(Json(json!({ "enabled": true, "user": user })))
}

#[derive(Deserialize)]
pub struct LoginBody {
    username: String,
    password: String,
}

pub async fn login(State(state): State<AppState>, Json(b): Json<LoginBody>) -> AppResult<Response> {
    let cfg = auth_config(&state)?;
    let username = b.username.trim().to_string();
    if state.login_failures.locked(&username) {
        return Err(AppError::TooManyRequests(format!(
            "登入失敗太多次，請 {} 分鐘後再試",
            auth::FAILURE_WINDOW_SECS / 60
        )));
    }
    let row: Option<(i64, String, bool)> =
        sqlx::query_as("SELECT id, password_hash, disabled FROM users WHERE username = ?")
            .bind(&username)
            .fetch_optional(&state.db)
            .await?;
    // 帳號不存在也照樣算一次 argon2，回應時間不會洩漏帳號是否存在
    let (id, hash, disabled) = row.unwrap_or((0, String::new(), true));
    let ok = auth::verify_password(b.password, hash).await;
    if !ok || disabled || id == 0 {
        state.login_failures.record(&username);
        return Err(AppError::Unauthorized("帳號或密碼不對，或帳號已停用".into()));
    }
    state.login_failures.clear(&username);
    let token = auth::create_session(&state.db, id).await?;
    let user = auth::user_by_id(&state.db, id).await?;
    Ok((
        [(header::SET_COOKIE, auth::session_cookie(&token, cfg.cookie_secure))],
        Json(json!({ "enabled": true, "user": user })),
    )
        .into_response())
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    let cfg = auth_config(&state)?;
    if let Some(t) = auth::token_from(&headers) {
        auth::delete_session(&state.db, &t).await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, auth::clear_cookie(cfg.cookie_secure))],
    )
        .into_response())
}

// ───────────────────────────── 帳號管理（admin） ─────────────────────────────

#[derive(sqlx::FromRow, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRow {
    id: i64,
    username: String,
    role: Role,
    disabled: bool,
    created_at: i64,
    last_login_at: Option<i64>,
    annotator_id: Option<i64>,
}

const USER_LIST: &str =
    "SELECT u.id, u.username, u.role, u.disabled, u.created_at, u.last_login_at, a.id AS annotator_id \
    FROM users u LEFT JOIN annotators a ON a.user_id = u.id";

pub async fn list_users(State(state): State<AppState>) -> AppResult<Json<Vec<UserRow>>> {
    auth_config(&state)?;
    Ok(Json(
        sqlx::query_as(&format!("{USER_LIST} ORDER BY u.username"))
            .fetch_all(&state.db)
            .await?,
    ))
}

async fn one_user(state: &AppState, id: i64) -> AppResult<UserRow> {
    sqlx::query_as(&format!("{USER_LIST} WHERE u.id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這個帳號"))
}

#[derive(Deserialize)]
pub struct NewUser {
    username: String,
    password: String,
    role: Role,
}

pub async fn create_user(
    State(state): State<AppState>,
    Json(b): Json<NewUser>,
) -> AppResult<(StatusCode, Json<UserRow>)> {
    auth_config(&state)?;
    let username = auth::check_username(&b.username)?;
    auth::check_new_password(&b.password)?;
    let hash = auth::hash_password(b.password).await?;
    let id = auth::create_user(&state.db, &username, &hash, b.role).await?;
    Ok((StatusCode::CREATED, Json(one_user(&state, id).await?)))
}

#[derive(Deserialize)]
pub struct UpdateUser {
    role: Option<Role>,
    disabled: Option<bool>,
    password: Option<String>,
}

/// 改角色、停用或重設密碼。不能停用或降級自己；停用和重設密碼時，那個人已經登入的裝置會被登出。
pub async fn update_user(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(id): Path<i64>,
    Json(b): Json<UpdateUser>,
) -> AppResult<Json<UserRow>> {
    auth_config(&state)?;
    let current = one_user(&state, id).await?;
    let me = viewer.0.as_ref().map(|u| u.id);
    if me == Some(id) && (b.disabled == Some(true) || b.role.is_some_and(|r| r != Role::Admin)) {
        return Err(AppError::bad("不能停用或降級自己的帳號"));
    }
    let mut logout = false;
    if let Some(role) = b.role {
        sqlx::query("UPDATE users SET role = ? WHERE id = ?")
            .bind(role)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(d) = b.disabled {
        sqlx::query("UPDATE users SET disabled = ? WHERE id = ?")
            .bind(d)
            .bind(id)
            .execute(&state.db)
            .await?;
        logout |= d && !current.disabled;
    }
    if let Some(p) = b.password {
        auth::check_new_password(&p)?;
        let hash = auth::hash_password(p).await?;
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(hash)
            .bind(id)
            .execute(&state.db)
            .await?;
        logout = true;
    }
    if logout {
        sqlx::query("DELETE FROM sessions WHERE user_id = ? OR expires_at < ?")
            .bind(id)
            .bind(now())
            .execute(&state.db)
            .await?;
    }
    Ok(Json(one_user(&state, id).await?))
}
