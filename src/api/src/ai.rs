//! AI 輔助標注：替一筆資料要建議、批次預標（信心夠高的自動存成 AI 標注者的標注）。

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use serde::Serialize;
use serde_json::Value;

use crate::{
    db::now,
    error::{AppError, AppResult},
    llm::{prompt, LlmRequest},
    media,
    models::{self, Item, NewAnnotation, Project, Source, Suggestion},
    state::AppState,
};

/// 批次預標一次最多幾筆。
pub const MAX_BATCH: i64 = 5_000;
/// 批次裡失敗的訊息最多留幾則。
const MAX_JOB_ERRORS: usize = 20;

fn unavailable(state: &AppState) -> AppError {
    AppError::Unavailable(format!(
        "AI 輔助沒有開啟：{}",
        state
            .config
            .llm
            .disabled_reason
            .clone()
            .unwrap_or_else(|| "沒有設定".into())
    ))
}

/// 請模型替一筆資料給建議，存起來並回傳。
pub async fn suggest(state: &AppState, project: &Project, item: &Item) -> AppResult<Suggestion> {
    let provider = state.llm.as_ref().ok_or_else(|| unavailable(state))?;
    let image = match &item.media {
        Some(m) if m.kind == "image" => {
            let bytes = tokio::fs::read(state.config.media_dir().join(&m.path))
                .await
                .map_err(anyhow::Error::from)?;
            Some(media::prepare_for_llm(&bytes, &m.mime).map_err(AppError::Internal)?)
        }
        _ => None,
    };
    let req = LlmRequest {
        system: prompt::system_prompt(project, true),
        text: prompt::user_text(item),
        image,
        schema: project.schema.suggestion_json_schema(true),
    };
    let reply = provider
        .complete(&req)
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    let raw = reply
        .json
        .get("values")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let (values, notes) = project.schema.coerce_suggestion(&raw);
    if !notes.is_empty() {
        tracing::info!(
            "item {}：模型的建議有部分欄位不能用，已略過：{}",
            item.id,
            notes.join("；")
        );
    }
    let confidence = reply
        .json
        .get("confidence")
        .and_then(Value::as_f64)
        .map(|c| c.clamp(0.0, 1.0));
    let rationale = reply
        .json
        .get("rationale")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO suggestions (item_id, provider, model, \"values\", confidence, rationale, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(item.id)
    .bind(provider.name())
    .bind(&reply.model)
    .bind(serde_json::to_string(&values)?)
    .bind(confidence)
    .bind(&rationale)
    .bind(now())
    .fetch_one(&state.db)
    .await?;
    models::get_suggestion(&state.db, id).await
}

// ───────────────────────────── 批次預標 ─────────────────────────────

#[derive(Default)]
pub struct Jobs(Mutex<HashMap<i64, Arc<Job>>>);

#[derive(Default)]
pub struct Job {
    total: usize,
    threshold: Option<f64>,
    done: AtomicUsize,
    auto_accepted: AtomicUsize,
    failed: AtomicUsize,
    cancelled: AtomicBool,
    finished: AtomicBool,
    errors: Mutex<Vec<String>>,
    started_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub running: bool,
    pub total: usize,
    pub done: usize,
    pub auto_accepted: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub threshold: Option<f64>,
    pub errors: Vec<String>,
    pub started_at: i64,
}

impl Job {
    fn status(&self) -> JobStatus {
        JobStatus {
            running: !self.finished.load(Ordering::SeqCst),
            total: self.total,
            done: self.done.load(Ordering::SeqCst),
            auto_accepted: self.auto_accepted.load(Ordering::SeqCst),
            failed: self.failed.load(Ordering::SeqCst),
            cancelled: self.cancelled.load(Ordering::SeqCst),
            threshold: self.threshold,
            errors: self.errors.lock().unwrap().clone(),
            started_at: self.started_at,
        }
    }
}

impl Jobs {
    pub fn status(&self, project_id: i64) -> Option<JobStatus> {
        self.0.lock().unwrap().get(&project_id).map(|j| j.status())
    }

    pub fn cancel(&self, project_id: i64) -> bool {
        match self.0.lock().unwrap().get(&project_id) {
            Some(j) if !j.finished.load(Ordering::SeqCst) => {
                j.cancelled.store(true, Ordering::SeqCst);
                true
            }
            _ => false,
        }
    }
}

/// 開始批次預標：挑還沒有人標過、也還沒有 AI 建議的資料，最多 `limit` 筆。
pub async fn start_batch(
    state: &AppState,
    project: Project,
    limit: i64,
    threshold: Option<f64>,
) -> AppResult<JobStatus> {
    if state.llm.is_none() {
        return Err(unavailable(state));
    }
    if let Some(t) = threshold {
        if !(0.0..=1.0).contains(&t) {
            return Err(AppError::bad("自動接受的門檻要介於 0 和 1"));
        }
    }
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT i.id FROM items i WHERE i.project_id = ? AND i.hidden = 0 \
         AND NOT EXISTS (SELECT 1 FROM annotations a WHERE a.item_id = i.id) \
         AND NOT EXISTS (SELECT 1 FROM suggestions s WHERE s.item_id = i.id) \
         ORDER BY i.id LIMIT ?",
    )
    .bind(project.id)
    .bind(limit.clamp(1, MAX_BATCH))
    .fetch_all(&state.db)
    .await?;
    let job = Arc::new(Job {
        total: ids.len(),
        threshold,
        started_at: now(),
        ..Default::default()
    });
    {
        let mut jobs = state.jobs.0.lock().unwrap();
        if jobs
            .get(&project.id)
            .is_some_and(|j| !j.finished.load(Ordering::SeqCst))
        {
            return Err(AppError::Conflict("這個專案已經有批次預標在跑了".into()));
        }
        jobs.insert(project.id, job.clone());
    }
    let status = job.status();
    tokio::spawn(run_batch(state.clone(), project, ids, job));
    Ok(status)
}

async fn run_batch(state: AppState, project: Project, ids: Vec<i64>, job: Arc<Job>) {
    let project = Arc::new(project);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(state.config.llm.concurrency));
    let mut tasks = tokio::task::JoinSet::new();
    for id in ids {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore is never closed");
        if job.cancelled.load(Ordering::SeqCst) {
            break;
        }
        let (state, project, job) = (state.clone(), project.clone(), job.clone());
        tasks.spawn(async move {
            let _permit = permit;
            match prelabel_one(&state, &project, id, job.threshold).await {
                Ok(auto) => {
                    if auto {
                        job.auto_accepted.fetch_add(1, Ordering::SeqCst);
                    }
                }
                Err(e) => {
                    job.failed.fetch_add(1, Ordering::SeqCst);
                    let msg = match e {
                        AppError::BadGateway(m) | AppError::BadRequest(m, _) | AppError::NotFound(m) => m,
                        other => format!("{other:?}"),
                    };
                    tracing::warn!("批次預標 item {id} 失敗：{msg}");
                    let mut errors = job.errors.lock().unwrap();
                    if errors.len() < MAX_JOB_ERRORS {
                        errors.push(format!("#{id}：{msg}"));
                    }
                }
            }
            job.done.fetch_add(1, Ordering::SeqCst);
        });
    }
    while tasks.join_next().await.is_some() {}
    job.finished.store(true, Ordering::SeqCst);
}

/// 回傳是否自動接受了。
async fn prelabel_one(state: &AppState, project: &Project, item_id: i64, threshold: Option<f64>) -> AppResult<bool> {
    let item = models::get_item(&state.db, item_id).await?;
    let s = suggest(state, project, &item).await?;
    let confident = matches!((threshold, s.confidence), (Some(t), Some(c)) if c >= t);
    // 高信心、而且必填欄位都有值，才自動存成標注
    if !confident || project.schema.validate_values(&s.values).is_err() || s.values.is_empty() {
        return Ok(false);
    }
    let mut tx = crate::db::write_tx(&state.db).await?;
    let annotator_id =
        models::annotator_by_name(&mut tx, &format!("{} {}", models::AI_ANNOTATOR_PREFIX, s.model), "ai").await?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM annotations WHERE item_id = ? AND annotator_id = ?")
        .bind(item_id)
        .bind(annotator_id)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_some() {
        return Ok(false);
    }
    models::insert_annotation(
        &mut tx,
        &NewAnnotation {
            item_id,
            annotator_id: Some(annotator_id),
            values: s.values.clone(),
            source: Source::AiAuto,
            suggestion_id: Some(s.id),
            at: now(),
        },
    )
    .await?;
    sqlx::query("UPDATE suggestions SET status = 'auto' WHERE id = ?")
        .bind(s.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}
