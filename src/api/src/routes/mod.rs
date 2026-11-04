//! HTTP API（規格見 `docs/api.md`）與前端靜態檔。

mod account;
mod annotators;
mod items;
mod projects;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderValue},
    middleware::from_fn_with_state,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

use crate::{auth, error::AppError, schema, state::AppState};

pub fn router(state: AppState) -> Router {
    let upload_limit = DefaultBodyLimit::max(state.config.max_upload_bytes);
    let api = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/meta", get(meta))
        .route("/auth/me", get(account::me))
        .route("/auth/login", post(account::login))
        .route("/auth/logout", post(account::logout))
        .route("/users", get(account::list_users).post(account::create_user))
        .route("/users/{id}", put(account::update_user))
        .route("/templates", get(|| async { Json(schema::templates()) }))
        .route("/samples/{name}", get(projects::sample))
        .route("/projects", get(projects::list).post(projects::create))
        .route(
            "/projects/{id}",
            get(projects::get_one).put(projects::update).delete(projects::remove),
        )
        .route("/projects/{id}/items", get(projects::items))
        .route("/projects/{id}/next", get(projects::next))
        .route("/projects/{id}/stats", get(projects::stats))
        .route("/projects/{id}/import", post(projects::import).layer(upload_limit))
        .route("/projects/{id}/import-template.csv", get(projects::import_template))
        .route("/projects/{id}/export", get(projects::export))
        .route(
            "/projects/{id}/ai/batch",
            get(projects::batch_status)
                .post(projects::batch_start)
                .delete(projects::batch_cancel),
        )
        .route("/import/legacy-db", post(projects::import_legacy).layer(upload_limit))
        .route("/items/{id}", get(items::detail).patch(items::update))
        .route("/items/{id}/annotation", put(items::save_annotation))
        .route("/items/{id}/suggest", post(items::suggest))
        .route("/annotations/{id}", delete(items::delete_annotation))
        .route("/suggestions/{id}/reject", post(items::reject_suggestion))
        .route("/media/{id}", get(items::media))
        .route("/annotators", get(annotators::list).post(annotators::create))
        .route("/annotators/{id}", put(annotators::rename).delete(annotators::remove))
        .fallback(|| async { AppError::not_found("找不到這個 API") })
        // 登入、角色、同源檢查（沒啟用登入時直接放行）；權限表在 auth::required_level
        .layer(from_fn_with_state(state.clone(), auth::guard));

    // 前端是單頁應用：找不到的路徑一律回 index.html，交給前端路由。
    let static_dir = state.config.static_dir.clone();
    let spa = ServeDir::new(&static_dir).fallback(ServeFile::new(static_dir.join("index.html")));

    Router::new()
        .nest("/api", api)
        .fallback_service(spa)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// 前端需要的基本資訊。AI 的部分只回服務名稱和模型，不回 API key 或網址。
async fn meta(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "ai": ai_status(&state),
        "maxUploadMb": state.config.max_upload_bytes / 1024 / 1024,
        "auth": { "enabled": state.config.auth.is_some() },
    }))
}

pub fn ai_status(state: &AppState) -> Value {
    match &state.llm {
        Some(p) => json!({ "enabled": true, "provider": p.name(), "model": p.model() }),
        None => json!({ "enabled": false, "reason": state.config.llm.disabled_reason }),
    }
}
