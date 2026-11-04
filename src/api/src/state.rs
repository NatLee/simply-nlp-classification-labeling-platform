use std::sync::Arc;

use sqlx::SqlitePool;

use crate::{ai::Jobs, config::Config, llm::Provider};

pub type AppState = Arc<Inner>;

pub struct Inner {
    pub db: SqlitePool,
    pub config: Config,
    /// AI 輔助標注；沒有設定時是 None。
    pub llm: Option<Provider>,
    /// 進行中的批次預標（每個專案最多一個）。
    pub jobs: Jobs,
}
