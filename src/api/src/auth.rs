//! 內建登入（選用）。沒設定 `ADMIN_PASSWORD` 時這裡的檢查全部跳過，和沒有登入的版本行為一樣。
//!
//! - 密碼用 argon2 雜湊；session 是 HttpOnly、SameSite=Lax 的 cookie，資料庫只存 token 的 SHA-256。
//! - 誰能呼叫哪個 API 只寫在 [`required_level`] 這張表；沒列到的一律要登入，不會因為漏寫而變成公開。
//! - 寫入的請求如果帶了 `Origin`（瀏覽器都會帶），要和 `PUBLIC_URL` 或請求的 Host 同源，擋跨站請求偽造（CSRF）。
//!   命令列工具不帶 `Origin`，本來就要自己帶 session cookie，所以不擋。
//! - 同一個帳號 15 分鐘內登入失敗 5 次，就先擋下來。
//! - 登入後，標注者就是帳號本人（同名的標注者），不能用別人的身分標注。

use std::collections::HashMap;

use anyhow::anyhow;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{FromRequestParts, Request, State},
    http::{header, request::Parts, HeaderMap, Method},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::{
    config::{AuthConfig, MIN_PASSWORD_CHARS},
    db::{now, write_tx},
    error::{AppError, AppResult},
    models::{self, AI_ANNOTATOR_PREFIX},
    state::AppState,
};

pub const COOKIE_NAME: &str = "snl_session";
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 3600;
pub const FAILURE_WINDOW_SECS: i64 = 15 * 60;
pub const MAX_FAILURES: usize = 5;
const MAX_USERNAME_CHARS: usize = 40;
const MAX_PASSWORD_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Role {
    /// 標注、看資料和統計
    Annotator,
    /// 再加上匯出、隱藏資料、批次預標
    Reviewer,
    /// 全部，包括專案設定、匯入、帳號管理
    Admin,
}

/// 一個 API 需要的權限。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Public,
    /// 任何登入的人
    User,
    /// reviewer 或 admin
    Review,
    Admin,
}

impl Role {
    fn level(self) -> Level {
        match self {
            Role::Annotator => Level::User,
            Role::Reviewer => Level::Review,
            Role::Admin => Level::Admin,
        }
    }
}

/// 誰能呼叫哪個 API。`path` 是 `/api` 後面的部分，例如 `/projects/3/export`。
pub fn required_level(method: &Method, path: &str) -> Level {
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    let read = method == Method::GET || method == Method::HEAD;
    match segs.as_slice() {
        ["health"] => Level::Public,
        ["meta"] if read => Level::Public,
        ["auth", "login" | "logout"] if *method == Method::POST => Level::Public,
        ["auth", "me"] if read => Level::Public,
        ["users", ..] => Level::Admin,
        ["projects"] if !read => Level::Admin,
        ["projects", _] if !read => Level::Admin,
        ["projects", _, "import"] | ["import", "legacy-db"] => Level::Admin,
        ["annotators", ..] if !read => Level::Admin,
        ["projects", _, "export"] => Level::Review,
        ["projects", _, "ai", "batch"] if !read => Level::Review,
        ["items", _] if *method == Method::PATCH => Level::Review,
        // 其他（讀取、標注、AI 建議、媒體檔）都要登入
        _ => Level::User,
    }
}

/// 目前登入的帳號。
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub role: Role,
    pub annotator_id: i64,
}

/// handler 用來取得目前的帳號：沒啟用登入時是 None。
pub struct Viewer(pub Option<CurrentUser>);

impl<S: Send + Sync> FromRequestParts<S> for Viewer {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Viewer(parts.extensions.get::<CurrentUser>().cloned()))
    }
}

impl Viewer {
    /// 「要用哪個標注者」只在這裡決定：有登入就用登入的人，送別人的 id 會被拒絕；沒登入照舊用請求裡的 id。
    pub fn annotator(&self, requested: i64) -> AppResult<i64> {
        match &self.0 {
            Some(u) if u.annotator_id != requested => Err(AppError::Forbidden("登入後只能用自己的身分標注".into())),
            _ => Ok(requested),
        }
    }

    pub fn at_least(&self, level: Level) -> bool {
        self.0.as_ref().is_none_or(|u| u.role.level() >= level)
    }
}

// ───────────────────────────── 密碼、session ─────────────────────────────

/// 16 bytes 的隨機鹽（argon2 建議的長度）。
fn new_salt() -> anyhow::Result<SaltString> {
    SaltString::encode_b64(&rand::random::<[u8; 16]>()).map_err(|e| anyhow!("產生鹽失敗：{e}"))
}

pub async fn hash_password(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        let salt = new_salt()?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow!("密碼雜湊失敗：{e}"))
    })
    .await?
}

/// 雜湊不合法（例如帳號不存在時傳空字串）也照樣算一次 argon2，回應時間不會洩漏帳號是否存在。
pub async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || match PasswordHash::new(&hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => {
            if let Ok(salt) = new_salt() {
                let _ = Argon2::default().hash_password(password.as_bytes(), &salt);
            }
            false
        }
    })
    .await
    .unwrap_or(false)
}

pub fn check_new_password(password: &str) -> AppResult<()> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(AppError::bad(format!("密碼至少要 {MIN_PASSWORD_CHARS} 個字元")));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(AppError::bad("密碼太長了"));
    }
    Ok(())
}

pub fn check_username(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_USERNAME_CHARS
        || name.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AppError::bad(format!("帳號不能有空白，最多 {MAX_USERNAME_CHARS} 個字")));
    }
    if name.starts_with(AI_ANNOTATOR_PREFIX) {
        return Err(AppError::bad(format!(
            "「{AI_ANNOTATOR_PREFIX}」開頭的名字保留給模型標注者"
        )));
    }
    Ok(name.to_string())
}

fn sha256_hex(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

pub async fn create_session(db: &SqlitePool, user_id: i64) -> anyhow::Result<String> {
    let token: String = rand::random::<[u8; 32]>().iter().map(|b| format!("{b:02x}")).collect();
    let t = now();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(sha256_hex(&token))
        .bind(user_id)
        .bind(t)
        .bind(t + SESSION_TTL_SECS)
        .execute(db)
        .await?;
    sqlx::query("UPDATE users SET last_login_at = ? WHERE id = ?")
        .bind(t)
        .bind(user_id)
        .execute(db)
        .await?;
    Ok(token)
}

pub async fn delete_session(db: &SqlitePool, token: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(sha256_hex(token))
        .execute(db)
        .await?;
    Ok(())
}

const USER_SELECT: &str = "SELECT u.id, u.username, u.role, a.id AS annotator_id \
    FROM users u JOIN annotators a ON a.user_id = u.id";

/// 停用的帳號，已經登入的 session 也立刻失效。
pub async fn user_for_token(db: &SqlitePool, token: &str) -> anyhow::Result<Option<CurrentUser>> {
    Ok(sqlx::query_as(&format!(
        "{USER_SELECT} JOIN sessions s ON s.user_id = u.id WHERE s.token_hash = ? AND s.expires_at > ? AND u.disabled = 0"
    ))
    .bind(sha256_hex(token))
    .bind(now())
    .fetch_optional(db)
    .await?)
}

pub async fn user_by_id(db: &SqlitePool, id: i64) -> anyhow::Result<Option<CurrentUser>> {
    Ok(sqlx::query_as(&format!("{USER_SELECT} WHERE u.id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?)
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECS}{secure}")
}

pub fn clear_cookie(secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}")
}

pub fn token_from(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|p| p.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE_NAME)
        .map(|(_, v)| v.to_string())
        .filter(|v| !v.is_empty())
}

// ───────────────────────────── 帳號 ─────────────────────────────

/// 新增帳號，並連到同名的標注者（沒有就建立）。啟用登入前用這個名字標的資料，會歸到這個帳號。
pub async fn create_user(db: &SqlitePool, username: &str, password_hash: &str, role: Role) -> AppResult<i64> {
    let mut tx = write_tx(db).await?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict(format!("已經有「{username}」這個帳號")));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, created_at) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(username)
    .bind(password_hash)
    .bind(role)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    let annotator_id = models::annotator_by_name(&mut tx, username, "human").await?;
    let linked: Option<Option<i64>> = sqlx::query_scalar("SELECT user_id FROM annotators WHERE id = ?")
        .bind(annotator_id)
        .fetch_optional(&mut *tx)
        .await?;
    if matches!(linked, Some(Some(_))) {
        return Err(AppError::Conflict(format!("標注者「{username}」已經屬於另一個帳號")));
    }
    sqlx::query("UPDATE annotators SET user_id = ? WHERE id = ?")
        .bind(id)
        .bind(annotator_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

/// 啟動時確認管理員帳號：沒有就建立；已經有了就以環境變數的密碼為準（忘記密碼時改環境變數重新啟動就好）。
pub async fn bootstrap_admin(db: &SqlitePool, cfg: &AuthConfig) -> anyhow::Result<()> {
    let username =
        check_username(&cfg.admin_username).map_err(|_| anyhow!("ADMIN_USERNAME 不能有空白，最多 40 個字"))?;
    let hash = hash_password(cfg.admin_password.clone()).await?;
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(db)
        .await?;
    match existing {
        Some(id) => {
            sqlx::query("UPDATE users SET password_hash = ?, role = 'admin', disabled = 0 WHERE id = ?")
                .bind(&hash)
                .bind(id)
                .execute(db)
                .await?;
        }
        None => {
            create_user(db, &username, &hash, Role::Admin)
                .await
                .map_err(|e| anyhow!("建立管理員失敗：{e:?}"))?;
            tracing::info!("已建立管理員帳號 {username}");
        }
    }
    Ok(())
}

// ───────────────────────────── 登入失敗限速 ─────────────────────────────

/// 帳號（小寫）→ 最近登入失敗的時間。
#[derive(Default)]
pub struct LoginFailures(std::sync::Mutex<HashMap<String, Vec<i64>>>);

impl LoginFailures {
    fn recent(list: &mut Vec<i64>, t: i64) {
        list.retain(|&x| t - x < FAILURE_WINDOW_SECS);
    }

    pub fn locked(&self, username: &str) -> bool {
        let mut map = self.0.lock().unwrap();
        let list = map.entry(username.to_lowercase()).or_default();
        Self::recent(list, now());
        list.len() >= MAX_FAILURES
    }

    pub fn record(&self, username: &str) {
        let t = now();
        let mut map = self.0.lock().unwrap();
        let list = map.entry(username.to_lowercase()).or_default();
        Self::recent(list, t);
        list.push(t);
    }

    pub fn clear(&self, username: &str) {
        self.0.lock().unwrap().remove(&username.to_lowercase());
    }
}

// ───────────────────────────── middleware ─────────────────────────────

/// 寫入請求的來源要和這個網站一樣。
fn same_origin(headers: &HeaderMap, cfg: &AuthConfig) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true; // 不是瀏覽器送的
    };
    let origin = origin.to_lowercase();
    if cfg.public_origin.as_deref() == Some(origin.as_str()) {
        return true;
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_lowercase();
    origin.split_once("://").is_some_and(|(_, h)| h == host)
}

/// 每個 `/api` 請求都經過這裡：檢查同源、登入和角色。沒啟用登入時直接放行。
pub async fn guard(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let Some(cfg) = &state.config.auth else {
        return next.run(req).await;
    };
    let method = req.method().clone();
    let writes = !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS);
    if writes && !same_origin(req.headers(), cfg) {
        return AppError::Forbidden("請求的來源和這個網站不同，已拒絕".into()).into_response();
    }
    let level = required_level(&method, req.uri().path());
    if level == Level::Public {
        return next.run(req).await;
    }
    let user = match token_from(req.headers()) {
        Some(t) => match user_for_token(&state.db, &t).await {
            Ok(u) => u,
            Err(e) => return AppError::Internal(e).into_response(),
        },
        None => None,
    };
    let Some(user) = user else {
        return AppError::Unauthorized("請先登入".into()).into_response();
    };
    if user.role.level() < level {
        return AppError::Forbidden("你的帳號沒有這個權限".into()).into_response();
    }
    req.extensions_mut().insert(user);
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_table() {
        use Level::*;
        let cases = [
            (Method::GET, "/health", Public),
            (Method::GET, "/meta", Public),
            (Method::POST, "/auth/login", Public),
            (Method::GET, "/auth/me", Public),
            (Method::GET, "/projects", User),
            (Method::GET, "/media/3", User),
            (Method::PUT, "/items/3/annotation", User),
            (Method::POST, "/items/3/suggest", User),
            (Method::GET, "/projects/1/export", Review),
            (Method::POST, "/projects/1/ai/batch", Review),
            (Method::GET, "/projects/1/ai/batch", User),
            (Method::PATCH, "/items/3", Review),
            (Method::POST, "/projects", Admin),
            (Method::PUT, "/projects/1", Admin),
            (Method::DELETE, "/projects/1", Admin),
            (Method::POST, "/projects/1/import", Admin),
            (Method::POST, "/import/legacy-db", Admin),
            (Method::GET, "/users", Admin),
            (Method::POST, "/annotators", Admin),
            (Method::GET, "/annotators", User),
            // 沒列到的寫入一律要登入
            (Method::POST, "/something/new", User),
            (Method::POST, "/meta", User),
        ];
        for (m, path, want) in cases {
            assert_eq!(required_level(&m, path), want, "{m} {path}");
        }
    }

    #[test]
    fn roles_are_ordered() {
        assert!(Role::Admin.level() > Role::Reviewer.level());
        assert!(Role::Reviewer.level() > Role::Annotator.level());
        assert_eq!(Role::Annotator.level(), Level::User);
    }

    #[test]
    fn failures_lock_after_the_limit() {
        let f = LoginFailures::default();
        for _ in 0..MAX_FAILURES - 1 {
            f.record("Amy");
        }
        assert!(!f.locked("amy"));
        f.record("AMY");
        assert!(f.locked("amy"));
        f.clear("amy");
        assert!(!f.locked("amy"));
    }

    #[test]
    fn origin_check() {
        let cfg = AuthConfig {
            admin_username: "admin".into(),
            admin_password: "x".into(),
            public_origin: Some("https://label.example.com".into()),
            cookie_secure: true,
        };
        let h = |pairs: &[(&str, &str)]| {
            let mut m = HeaderMap::new();
            for (k, v) in pairs {
                m.insert(
                    header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                    v.parse().unwrap(),
                );
            }
            m
        };
        assert!(same_origin(&h(&[]), &cfg), "no Origin: command-line clients");
        assert!(same_origin(&h(&[("origin", "https://label.example.com")]), &cfg));
        assert!(same_origin(
            &h(&[("origin", "http://localhost:5000"), ("host", "localhost:5000")]),
            &cfg
        ));
        assert!(!same_origin(
            &h(&[("origin", "https://evil.test"), ("host", "label.example.com")]),
            &cfg
        ));
    }

    #[test]
    fn cookies() {
        assert!(session_cookie("abc", true).contains("HttpOnly; SameSite=Lax"));
        assert!(session_cookie("abc", true).ends_with("; Secure"));
        assert!(!session_cookie("abc", false).contains("Secure"));
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; snl_session=tok; b=2".parse().unwrap());
        assert_eq!(token_from(&h).as_deref(), Some("tok"));
    }
}
