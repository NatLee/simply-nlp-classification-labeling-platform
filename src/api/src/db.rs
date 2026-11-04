use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};

pub async fn connect(path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| format!("建立資料夾失敗：{}", parent.display()))?;
        }
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .with_context(|| format!("開不了資料庫：{}", path.display()))?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("資料庫 migration 失敗")?;
    Ok(pool)
}

/// 開一個寫入用的交易。用 `BEGIN IMMEDIATE` 一開始就拿寫入鎖：
/// 預設的 `BEGIN` 先讀後寫時，如果另一個連線剛好寫入，SQLite 會直接回「database is locked」，
/// 不會等 busy_timeout（批次預標同時跑好幾筆時就會遇到）。
pub async fn write_tx(db: &SqlitePool) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, sqlx::Error> {
    db.begin_with("BEGIN IMMEDIATE").await
}

/// 現在的 Unix 秒數。
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
