//! 分類標注平台：自架的網頁系統，標注文字、圖片和音訊的分數與類別，可以讓模型先給建議。

pub mod agreement;
pub mod ai;
pub mod config;
pub mod consensus;
pub mod db;
pub mod error;
pub mod export;
pub mod import;
pub mod legacy;
pub mod llm;
pub mod media;
pub mod models;
pub mod routes;
pub mod schema;
pub mod state;
pub mod stats;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;

pub use config::Config;
pub use state::{AppState, Inner};

/// 開資料庫、建立 AI 服務的連線設定。
pub async fn build_state(config: Config) -> Result<AppState> {
    let db = db::connect(&config.database_path).await?;
    let llm = config
        .llm
        .provider
        .as_ref()
        .map(|p| llm::Provider::new(p, config.llm.timeout))
        .transpose()?;
    Ok(Arc::new(Inner {
        db,
        config,
        llm,
        jobs: ai::Jobs::default(),
    }))
}

pub fn app(state: AppState) -> Router {
    routes::router(state)
}
