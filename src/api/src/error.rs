use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// API 錯誤。回應格式固定是 `{"error": "訊息"}`；欄位驗證錯誤另外附 `details`。
#[derive(Debug)]
pub enum AppError {
    BadRequest(String, Vec<String>),
    /// 沒有登入（啟用內建登入時）。
    Unauthorized(String),
    /// 登入了，但帳號的角色不夠。
    Forbidden(String),
    TooManyRequests(String),
    NotFound(String),
    Conflict(String),
    PayloadTooLarge(String),
    /// AI 服務（Claude 或 OpenAI 相容 API）回了錯誤或沒回應。
    BadGateway(String),
    /// AI 功能沒開。
    Unavailable(String),
    Internal(anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn bad(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into(), Vec::new())
    }
    pub fn invalid(msg: impl Into<String>, details: Vec<String>) -> Self {
        Self::BadRequest(msg.into(), details)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg, details) = match self {
            AppError::BadRequest(m, d) => (StatusCode::BAD_REQUEST, m, d),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m, vec![]),
            AppError::Forbidden(m) => (StatusCode::FORBIDDEN, m, vec![]),
            AppError::TooManyRequests(m) => (StatusCode::TOO_MANY_REQUESTS, m, vec![]),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m, vec![]),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m, vec![]),
            AppError::PayloadTooLarge(m) => (StatusCode::PAYLOAD_TOO_LARGE, m, vec![]),
            AppError::BadGateway(m) => (StatusCode::BAD_GATEWAY, m, vec![]),
            AppError::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m, vec![]),
            AppError::Internal(err) => {
                tracing::error!("internal error: {err:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "伺服器內部錯誤，請查看伺服器 log".to_string(),
                    vec![],
                )
            }
        };
        let body = if details.is_empty() {
            json!({ "error": msg })
        } else {
            json!({ "error": msg, "details": details })
        };
        (status, Json(body)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        AppError::Internal(err.into())
    }
}
