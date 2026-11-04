//! 匯出：CSV、JSONL、zip（JSONL＋媒體檔，可以再匯入），以及微調／評估用的聊天格式 JSONL。

use std::{collections::HashMap, io::Write};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    consensus::consensus,
    error::{AppError, AppResult},
    llm::prompt,
    media,
    models::{self, Annotation, Item, Project},
    schema::Values,
    state::AppState,
};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Csv,
    Jsonl,
    Zip,
    /// 一筆資料一行的 `messages` 格式：system（標注說明）、user（資料）、assistant（共識標注的 JSON）。
    /// OpenAI 微調、TRL、LLaMA-Factory 等工具都讀這種格式；用同一套提示拿來評估模型也行。
    ChatJsonl,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// 一筆標注一列（CSV 用）
    #[default]
    Annotations,
    /// 一筆資料一列，用共識標注（CSV 用）
    Items,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    pub format: Format,
    #[serde(default)]
    pub mode: Mode,
    /// 不要把「高信心自動接受」的 AI 標注算進共識（做評估資料時用，避免拿模型的答案評模型）。
    #[serde(default)]
    pub exclude_auto: bool,
    /// 包含隱藏的資料。
    #[serde(default)]
    pub include_hidden: bool,
}

pub struct ExportFile {
    pub filename: String,
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

fn iso(t: i64) -> String {
    DateTime::<Utc>::from_timestamp(t, 0)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}

fn media_file(m: &models::MediaInfo) -> String {
    let ext = m.path.rsplit('.').next().unwrap_or("bin");
    format!("media/{}.{ext}", m.sha256)
}

struct Dataset {
    items: Vec<Item>,
    by_item: HashMap<i64, Vec<Annotation>>,
}

async fn load(state: &AppState, project: &Project, q: &ExportQuery) -> AppResult<Dataset> {
    let items: Vec<Item> = models::all_items(&state.db, project.id)
        .await?
        .into_iter()
        .filter(|i| q.include_hidden || !i.hidden)
        .collect();
    let mut by_item: HashMap<i64, Vec<Annotation>> = HashMap::new();
    for a in models::project_annotations(&state.db, project.id).await? {
        by_item.entry(a.item_id).or_default().push(a);
    }
    Ok(Dataset { items, by_item })
}

fn consensus_for(project: &Project, anns: &[Annotation], exclude_auto: bool) -> Values {
    let vals: Vec<&Values> = anns
        .iter()
        .filter(|a| !(exclude_auto && a.source == "ai_auto"))
        .map(|a| &a.values)
        .collect();
    consensus(&project.schema, &vals)
}

pub async fn export(state: &AppState, project: &Project, q: &ExportQuery) -> AppResult<ExportFile> {
    let data = load(state, project, q).await?;
    let stem = format!("project-{}", project.id);
    match q.format {
        Format::Csv => Ok(ExportFile {
            filename: format!(
                "{stem}-{}.csv",
                if q.mode == Mode::Items { "items" } else { "annotations" }
            ),
            content_type: "text/csv; charset=utf-8",
            bytes: csv_bytes(project, &data, q)?,
        }),
        Format::Jsonl => Ok(ExportFile {
            filename: format!("{stem}.jsonl"),
            content_type: "application/x-ndjson; charset=utf-8",
            bytes: jsonl_bytes(project, &data, q),
        }),
        Format::Zip => Ok(ExportFile {
            filename: format!("{stem}.zip"),
            content_type: "application/zip",
            bytes: zip_bytes(state, project, &data, q).await?,
        }),
        Format::ChatJsonl => Ok(ExportFile {
            filename: format!("{stem}-chat.jsonl"),
            content_type: "application/x-ndjson; charset=utf-8",
            bytes: chat_bytes(state, project, &data, q).await?,
        }),
    }
}

/// CSV 儲存格：文字照原樣，數字和清單寫成 JSON（清單是 `["a","b"]`，匯入時讀得回來）。
fn cell(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// 一個 BOM 開頭，Excel 開啟時中文才不會變成亂碼。
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

fn csv_bytes(project: &Project, data: &Dataset, q: &ExportQuery) -> AppResult<Vec<u8>> {
    let mut w = csv::Writer::from_writer(UTF8_BOM.to_vec());
    let fields = &project.schema.fields;
    let mut header: Vec<String> = vec!["item_id".into(), "text".into(), "media".into()];
    if q.mode == Mode::Annotations {
        header.extend(["annotator".into(), "source".into(), "created_at".into()]);
    } else {
        header.push("n_annotations".into());
    }
    header.extend(fields.iter().map(|f| f.id.clone()));
    w.write_record(&header).map_err(anyhow::Error::from)?;
    for item in &data.items {
        let anns = data.by_item.get(&item.id).map(Vec::as_slice).unwrap_or(&[]);
        let base = [
            item.id.to_string(),
            item.text.clone(),
            item.media.as_ref().map(media_file).unwrap_or_default(),
        ];
        match q.mode {
            Mode::Annotations => {
                for a in anns {
                    let mut row: Vec<String> = base.to_vec();
                    row.extend([
                        a.annotator_name.clone().unwrap_or_default(),
                        a.source.clone(),
                        iso(a.created_at),
                    ]);
                    row.extend(fields.iter().map(|f| cell(a.values.get(&f.id))));
                    w.write_record(&row).map_err(anyhow::Error::from)?;
                }
            }
            Mode::Items => {
                let c = consensus_for(project, anns, q.exclude_auto);
                let mut row: Vec<String> = base.to_vec();
                row.push(anns.len().to_string());
                row.extend(fields.iter().map(|f| cell(c.get(&f.id))));
                w.write_record(&row).map_err(anyhow::Error::from)?;
            }
        }
    }
    Ok(w.into_inner().map_err(|e| anyhow::anyhow!("{e}"))?)
}

fn item_json(project: &Project, item: &Item, anns: &[Annotation], exclude_auto: bool) -> Value {
    json!({
        "id": item.id,
        "text": item.text,
        "media": item.media.as_ref().map(|m| json!({
            "file": media_file(m), "kind": m.kind, "mime": m.mime, "sha256": m.sha256,
        })),
        "meta": item.meta,
        "hidden": item.hidden,
        "annotations": anns.iter().map(|a| json!({
            "annotator": a.annotator_name,
            "source": a.source,
            "values": a.values,
            "created_at": iso(a.created_at),
        })).collect::<Vec<_>>(),
        "consensus": consensus_for(project, anns, exclude_auto),
    })
}

fn jsonl_bytes(project: &Project, data: &Dataset, q: &ExportQuery) -> Vec<u8> {
    let mut out = Vec::new();
    for item in &data.items {
        let anns = data.by_item.get(&item.id).map(Vec::as_slice).unwrap_or(&[]);
        out.extend(item_json(project, item, anns, q.exclude_auto).to_string().as_bytes());
        out.push(b'\n');
    }
    out
}

async fn zip_bytes(state: &AppState, project: &Project, data: &Dataset, q: &ExportQuery) -> AppResult<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("items.jsonl", opts).map_err(anyhow::Error::from)?;
    zip.write_all(&jsonl_bytes(project, data, q))
        .map_err(anyhow::Error::from)?;
    let mut written = std::collections::HashSet::new();
    let media_dir = state.config.media_dir();
    for item in &data.items {
        let Some(m) = &item.media else { continue };
        let name = media_file(m);
        if !written.insert(name.clone()) {
            continue;
        }
        let bytes = tokio::fs::read(media_dir.join(&m.path))
            .await
            .map_err(|e| anyhow::anyhow!("讀不到媒體檔 {}：{e}", m.path))?;
        // 圖片和音訊本身已經壓縮過，不用再壓
        zip.start_file(&name, stored).map_err(anyhow::Error::from)?;
        zip.write_all(&bytes).map_err(anyhow::Error::from)?;
    }
    Ok(zip.finish().map_err(anyhow::Error::from)?.into_inner())
}

async fn chat_bytes(state: &AppState, project: &Project, data: &Dataset, q: &ExportQuery) -> AppResult<Vec<u8>> {
    let system = prompt::system_prompt(project, false);
    let media_dir = state.config.media_dir();
    let mut out = Vec::new();
    let mut written = 0usize;
    for item in &data.items {
        let anns = data.by_item.get(&item.id).map(Vec::as_slice).unwrap_or(&[]);
        let target = consensus_for(project, anns, q.exclude_auto);
        if target.is_empty() {
            continue;
        }
        let image = match &item.media {
            Some(m) if m.kind == "image" => {
                let bytes = tokio::fs::read(media_dir.join(&m.path))
                    .await
                    .map_err(anyhow::Error::from)?;
                Some(media::prepare_for_llm(&bytes, &m.mime).map_err(AppError::Internal)?)
            }
            _ => None,
        };
        let user_text = prompt::user_text(item);
        let user: Value = match &image {
            Some(img) => json!([
                { "type": "image_url", "image_url": { "url": img.data_url() } },
                { "type": "text", "text": user_text },
            ]),
            None => Value::from(user_text),
        };
        let mut answer = Map::new();
        answer.insert("values".into(), Value::Object(target));
        let line = json!({
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
                { "role": "assistant", "content": Value::Object(answer).to_string() },
            ],
            "metadata": { "item_id": item.id, "n_annotations": anns.len() },
        });
        out.extend(line.to_string().as_bytes());
        out.push(b'\n');
        written += 1;
    }
    if written == 0 {
        return Err(AppError::bad("還沒有任何一筆資料有共識標注，沒有東西可以匯出"));
    }
    Ok(out)
}
