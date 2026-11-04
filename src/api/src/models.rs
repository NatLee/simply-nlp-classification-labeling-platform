//! 資料表的資料列和常用的查詢。查詢一律用 `query_as` 對應到結構，欄位名稱和 SQL 的別名一致。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, Sqlite, SqlitePool, Transaction};

use crate::{
    db::now,
    error::{AppError, AppResult},
    schema::{Schema, Values},
};

pub type Tx<'a> = Transaction<'a, Sqlite>;

// ───────────────────────────── 專案 ─────────────────────────────

/// 專案的 AI 輔助設定。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// 給模型和標注者看的標注說明。
    pub instructions: String,
    /// 批次預標時，信心至少這麼高的建議直接存成 AI 標注者的標注；None 表示全部留給人確認。
    pub auto_accept_threshold: Option<f64>,
}

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id: i64,
    name: String,
    description: String,
    schema: String,
    ai: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub schema: Schema,
    pub ai: AiSettings,
    pub created_at: i64,
    pub updated_at: i64,
}

impl TryFrom<ProjectRow> for Project {
    type Error = anyhow::Error;
    fn try_from(r: ProjectRow) -> anyhow::Result<Self> {
        Ok(Self {
            id: r.id,
            name: r.name,
            description: r.description,
            schema: serde_json::from_str(&r.schema)?,
            ai: serde_json::from_str(&r.ai)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

const PROJECT_COLUMNS: &str = "id, name, description, schema, ai, created_at, updated_at";

pub async fn get_project(db: &SqlitePool, id: i64) -> AppResult<Project> {
    let row: Option<ProjectRow> = sqlx::query_as(&format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.ok_or_else(|| AppError::not_found("找不到這個專案"))?.try_into()?)
}

pub async fn list_projects(db: &SqlitePool) -> AppResult<Vec<Project>> {
    let rows: Vec<ProjectRow> = sqlx::query_as(&format!("SELECT {PROJECT_COLUMNS} FROM projects ORDER BY id DESC"))
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(Project::try_from).collect::<anyhow::Result<_>>()?)
}

pub async fn insert_project(
    tx: &mut Tx<'_>,
    name: &str,
    description: &str,
    schema: &Schema,
    ai: &AiSettings,
) -> anyhow::Result<i64> {
    let t = now();
    let id = sqlx::query_scalar(
        "INSERT INTO projects (name, description, schema, ai, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(name)
    .bind(description)
    .bind(serde_json::to_string(schema)?)
    .bind(serde_json::to_string(ai)?)
    .bind(t)
    .bind(t)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

// ───────────────────────────── 資料 ─────────────────────────────

pub struct NewItem {
    pub text: String,
    pub media_id: Option<i64>,
    pub media_sha: Option<String>,
    pub meta: Value,
    pub hidden: bool,
    pub created_at: i64,
}

/// 同一個專案裡，文字和媒體都一樣的資料視為重複。
pub fn dedupe_key(text: &str, media_sha: Option<&str>) -> String {
    let mut h = Sha256::new();
    h.update(text.trim().as_bytes());
    h.update([0u8]);
    h.update(media_sha.unwrap_or("").as_bytes());
    format!("{:x}", h.finalize())
}

/// 新增一筆資料；重複時回傳 None。
pub async fn insert_item(tx: &mut Tx<'_>, project_id: i64, item: &NewItem) -> anyhow::Result<Option<i64>> {
    let id = sqlx::query_scalar(
        "INSERT INTO items (project_id, text, media_id, meta, hidden, dedupe_key, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (project_id, dedupe_key) DO NOTHING RETURNING id",
    )
    .bind(project_id)
    .bind(&item.text)
    .bind(item.media_id)
    .bind(serde_json::to_string(&item.meta)?)
    .bind(item.hidden)
    .bind(dedupe_key(&item.text, item.media_sha.as_deref()))
    .bind(item.created_at)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(id)
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub id: i64,
    pub kind: String,
    pub mime: String,
    pub sha256: String,
    #[serde(skip)]
    pub path: String,
}

#[derive(sqlx::FromRow)]
struct ItemRow {
    id: i64,
    project_id: i64,
    text: String,
    meta: String,
    hidden: bool,
    created_at: i64,
    media_id: Option<i64>,
    media_kind: Option<String>,
    media_mime: Option<String>,
    media_sha: Option<String>,
    media_path: Option<String>,
    annotation_count: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: i64,
    pub project_id: i64,
    pub text: String,
    pub media: Option<MediaInfo>,
    pub meta: Value,
    pub hidden: bool,
    pub created_at: i64,
    pub annotation_count: i64,
}

impl From<ItemRow> for Item {
    fn from(r: ItemRow) -> Self {
        let media = match (r.media_id, r.media_kind, r.media_mime, r.media_sha, r.media_path) {
            (Some(id), Some(kind), Some(mime), Some(sha256), Some(path)) => Some(MediaInfo {
                id,
                kind,
                mime,
                sha256,
                path,
            }),
            _ => None,
        };
        Self {
            id: r.id,
            project_id: r.project_id,
            text: r.text,
            media,
            meta: serde_json::from_str(&r.meta).unwrap_or(Value::Null),
            hidden: r.hidden,
            created_at: r.created_at,
            annotation_count: r.annotation_count,
        }
    }
}

const ITEM_SELECT: &str = "SELECT i.id, i.project_id, i.text, i.meta, i.hidden, i.created_at, \
    m.id AS media_id, m.kind AS media_kind, m.mime AS media_mime, m.sha256 AS media_sha, m.path AS media_path, \
    (SELECT COUNT(*) FROM annotations a WHERE a.item_id = i.id) AS annotation_count \
    FROM items i LEFT JOIN media m ON m.id = i.media_id";

pub async fn get_item(db: &SqlitePool, id: i64) -> AppResult<Item> {
    let row: Option<ItemRow> = sqlx::query_as(&format!("{ITEM_SELECT} WHERE i.id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.ok_or_else(|| AppError::not_found("找不到這筆資料"))?.into())
}

/// 列出資料。`filter` 見 [`ItemFilter`]；`query` 是文字搜尋。
pub async fn list_items(
    db: &SqlitePool,
    project_id: i64,
    filter: ItemFilter,
    annotator_id: Option<i64>,
    query: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Item>, i64)> {
    let push_where = |b: &mut QueryBuilder<Sqlite>| {
        b.push(" WHERE i.project_id = ").push_bind(project_id);
        match filter {
            ItemFilter::All => {}
            ItemFilter::Unlabeled => {
                b.push(" AND i.hidden = 0 AND NOT EXISTS (SELECT 1 FROM annotations a WHERE a.item_id = i.id)");
            }
            ItemFilter::Labeled => {
                b.push(" AND EXISTS (SELECT 1 FROM annotations a WHERE a.item_id = i.id)");
            }
            ItemFilter::Mine => {
                b.push(" AND EXISTS (SELECT 1 FROM annotations a WHERE a.item_id = i.id AND a.annotator_id = ")
                    .push_bind(annotator_id)
                    .push(")");
            }
            ItemFilter::Hidden => {
                b.push(" AND i.hidden = 1");
            }
            ItemFilter::Pending => {
                b.push(" AND EXISTS (SELECT 1 FROM suggestions s WHERE s.item_id = i.id AND s.status = 'pending')");
                b.push(
                    " AND NOT EXISTS (SELECT 1 FROM annotations a WHERE a.item_id = i.id AND a.source <> 'ai_auto')",
                );
            }
        }
        if let Some(q) = query {
            b.push(" AND i.text LIKE '%' || ")
                .push_bind(q.to_string())
                .push(" || '%'");
        }
    };
    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM items i");
    push_where(&mut count);
    let total: i64 = count.build_query_scalar().fetch_one(db).await?;
    let mut select = QueryBuilder::<Sqlite>::new(ITEM_SELECT);
    push_where(&mut select);
    select
        .push(" ORDER BY i.id LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);
    let rows: Vec<ItemRow> = select.build_query_as().fetch_all(db).await?;
    Ok((rows.into_iter().map(Item::from).collect(), total))
}

#[derive(Clone, Copy, Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ItemFilter {
    #[default]
    All,
    /// 還沒有任何人標過（不含隱藏的）。
    Unlabeled,
    Labeled,
    /// 目前的標注者標過的。
    Mine,
    Hidden,
    /// 有 AI 建議在等人確認。
    Pending,
}

pub async fn all_items(db: &SqlitePool, project_id: i64) -> AppResult<Vec<Item>> {
    let rows: Vec<ItemRow> = sqlx::query_as(&format!("{ITEM_SELECT} WHERE i.project_id = ? ORDER BY i.id"))
        .bind(project_id)
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(Item::from).collect())
}

// ───────────────────────────── 標注者 ─────────────────────────────

/// 模型標注者的名字開頭（後面接模型名稱），人不能用這個開頭。
pub const AI_ANNOTATOR_PREFIX: &str = "AI ·";

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Annotator {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub created_at: i64,
}

pub async fn list_annotators(db: &SqlitePool) -> AppResult<Vec<Annotator>> {
    Ok(
        sqlx::query_as("SELECT id, name, kind, created_at FROM annotators ORDER BY kind, name")
            .fetch_all(db)
            .await?,
    )
}

pub async fn get_annotator(db: &SqlitePool, id: i64) -> AppResult<Annotator> {
    sqlx::query_as("SELECT id, name, kind, created_at FROM annotators WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::not_found("找不到這位標注者"))
}

/// 用名字找標注者，沒有就建立。
pub async fn annotator_by_name(tx: &mut Tx<'_>, name: &str, kind: &str) -> anyhow::Result<i64> {
    let name = name.trim();
    if let Some(id) = sqlx::query_scalar("SELECT id FROM annotators WHERE name = ?")
        .bind(name)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(id);
    }
    Ok(
        sqlx::query_scalar("INSERT INTO annotators (name, kind, created_at) VALUES (?, ?, ?) RETURNING id")
            .bind(name)
            .bind(kind)
            .bind(now())
            .fetch_one(&mut **tx)
            .await?,
    )
}

// ───────────────────────────── 標注 ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Human,
    AiAssisted,
    AiAuto,
    Import,
    Legacy,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::AiAssisted => "ai_assisted",
            Self::AiAuto => "ai_auto",
            Self::Import => "import",
            Self::Legacy => "legacy",
        }
    }
}

pub struct NewAnnotation {
    pub item_id: i64,
    pub annotator_id: Option<i64>,
    pub values: Values,
    pub source: Source,
    pub suggestion_id: Option<i64>,
    pub at: i64,
}

pub async fn insert_annotation(tx: &mut Tx<'_>, a: &NewAnnotation) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar(
        "INSERT INTO annotations (item_id, annotator_id, \"values\", source, suggestion_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(a.item_id)
    .bind(a.annotator_id)
    .bind(serde_json::to_string(&a.values)?)
    .bind(a.source.as_str())
    .bind(a.suggestion_id)
    .bind(a.at)
    .bind(a.at)
    .fetch_one(&mut **tx)
    .await?)
}

#[derive(sqlx::FromRow)]
struct AnnotationRow {
    id: i64,
    item_id: i64,
    annotator_id: Option<i64>,
    annotator_name: Option<String>,
    annotator_kind: Option<String>,
    values: String,
    source: String,
    suggestion_id: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub id: i64,
    pub item_id: i64,
    pub annotator_id: Option<i64>,
    pub annotator_name: Option<String>,
    pub annotator_kind: Option<String>,
    pub values: Values,
    pub source: String,
    pub suggestion_id: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<AnnotationRow> for Annotation {
    fn from(r: AnnotationRow) -> Self {
        Self {
            id: r.id,
            item_id: r.item_id,
            annotator_id: r.annotator_id,
            annotator_name: r.annotator_name,
            annotator_kind: r.annotator_kind,
            values: serde_json::from_str(&r.values).unwrap_or_default(),
            source: r.source,
            suggestion_id: r.suggestion_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const ANNOTATION_SELECT: &str =
    "SELECT a.id, a.item_id, a.annotator_id, n.name AS annotator_name, n.kind AS annotator_kind, \
    a.\"values\" AS \"values\", a.source, a.suggestion_id, a.created_at, a.updated_at \
    FROM annotations a LEFT JOIN annotators n ON n.id = a.annotator_id";

pub async fn item_annotations(db: &SqlitePool, item_id: i64) -> AppResult<Vec<Annotation>> {
    let rows: Vec<AnnotationRow> = sqlx::query_as(&format!(
        "{ANNOTATION_SELECT} WHERE a.item_id = ? ORDER BY a.created_at, a.id"
    ))
    .bind(item_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Annotation::from).collect())
}

/// 整個專案的標注，依資料、時間排序。
pub async fn project_annotations(db: &SqlitePool, project_id: i64) -> AppResult<Vec<Annotation>> {
    let rows: Vec<AnnotationRow> = sqlx::query_as(&format!(
        "{ANNOTATION_SELECT} JOIN items i ON i.id = a.item_id WHERE i.project_id = ? ORDER BY a.item_id, a.created_at, a.id"
    ))
    .bind(project_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Annotation::from).collect())
}

pub async fn get_annotation(db: &SqlitePool, id: i64) -> AppResult<Annotation> {
    let row: Option<AnnotationRow> = sqlx::query_as(&format!("{ANNOTATION_SELECT} WHERE a.id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.ok_or_else(|| AppError::not_found("找不到這筆標注"))?.into())
}

// ───────────────────────────── AI 建議 ─────────────────────────────

#[derive(sqlx::FromRow)]
struct SuggestionRow {
    id: i64,
    item_id: i64,
    provider: String,
    model: String,
    values: String,
    confidence: Option<f64>,
    rationale: String,
    status: String,
    created_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: i64,
    pub item_id: i64,
    pub provider: String,
    pub model: String,
    pub values: Values,
    pub confidence: Option<f64>,
    pub rationale: String,
    pub status: String,
    pub created_at: i64,
}

impl From<SuggestionRow> for Suggestion {
    fn from(r: SuggestionRow) -> Self {
        Self {
            id: r.id,
            item_id: r.item_id,
            provider: r.provider,
            model: r.model,
            values: serde_json::from_str(&r.values).unwrap_or_default(),
            confidence: r.confidence,
            rationale: r.rationale,
            status: r.status,
            created_at: r.created_at,
        }
    }
}

const SUGGESTION_SELECT: &str =
    "SELECT id, item_id, provider, model, \"values\" AS \"values\", confidence, rationale, status, created_at FROM suggestions";

pub async fn latest_suggestion(db: &SqlitePool, item_id: i64) -> AppResult<Option<Suggestion>> {
    let row: Option<SuggestionRow> = sqlx::query_as(&format!(
        "{SUGGESTION_SELECT} WHERE item_id = ? ORDER BY created_at DESC, id DESC LIMIT 1"
    ))
    .bind(item_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(Suggestion::from))
}

pub async fn get_suggestion(db: &SqlitePool, id: i64) -> AppResult<Suggestion> {
    let row: Option<SuggestionRow> = sqlx::query_as(&format!("{SUGGESTION_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.ok_or_else(|| AppError::not_found("找不到這筆 AI 建議"))?.into())
}

pub async fn set_suggestion_status(db: &SqlitePool, id: i64, status: &str) -> AppResult<()> {
    sqlx::query("UPDATE suggestions SET status = ? WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
