//! 和舊版（2019–2023 的 Flask 版 emoLabel）相容的部分都集中在這裡。
//!
//! 為什麼要留：使用者手上可能還有舊的 `emoLabel.db` 和照舊範本做的 CSV。
//! 什麼時候可以拿掉：確定沒有人再匯入舊資料之後。這些函式只在匯入時用到，拿掉不影響其他功能。
//!
//! 舊版的資料長這樣：
//! - `emotion(id, text, date, flag)`：`text` 宣告成 INTEGER，純數字的文字會被 SQLite 存成整數；
//!   `flag = 1` 才會出現在標注畫面；`date` 是伺服器當地時間，沒有時區。
//! - `label(id, emotionId, score, tag, type, tag_opt, updateDate)`：沒有標注者身分，同一筆文字可以標很多次；
//!   `tag_opt` 是 Python 的 `repr(list)`，例如 `['政治', "it's"]`。
//! - CSV 範本：`Text` 一欄；或 `Level,Classes,Custom classes,Text`（Level 是 1–5 分、Classes 是情緒、
//!   Custom classes 是 JSON 陣列）。舊版只讀第一種，而且一行就是一筆，不理會逗號。

use std::{collections::BTreeSet, path::Path};

use anyhow::{bail, Context, Result};
use chrono::{FixedOffset, NaiveDateTime, TimeZone};
use serde_json::{json, Value};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};

use crate::{
    db::now,
    models::{self, AiSettings, NewAnnotation, Source},
    schema::{legacy_emotion_schema, Choice, FieldKind, Schema, Values},
};

/// 舊 CSV 範本的欄名 → 新版的欄位 id。只在專案真的有那個欄位時才換。
pub const COLUMN_ALIASES: &[(&str, &str)] = &[("Level", "score"), ("Classes", "tag"), ("Custom classes", "tag_opt")];

/// 舊版的時間格式，例如 `2019-05-24 13:05:01`。
const DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// 把 CSV 的欄名換成新版的名稱：`Text` → `text`，舊範本的欄名照 [`COLUMN_ALIASES`] 換。
pub fn canonical_column(header: &str, schema: &Schema) -> String {
    let h = header.trim();
    if let Some((_, id)) = COLUMN_ALIASES.iter().find(|(old, _)| old.eq_ignore_ascii_case(h)) {
        if schema.field(id).is_some() {
            return id.to_string();
        }
    }
    let lower = h.to_lowercase();
    if lower == "text" {
        return lower;
    }
    h.to_string()
}

/// 只有一欄的 CSV（舊版的 `Text` 範本）：舊版一行就是一筆，文字裡的逗號照留。
/// CSV 解析器會在沒加引號的逗號切開，這裡接回去。
pub fn single_column_text(fields: &csv::StringRecord) -> String {
    fields.iter().collect::<Vec<_>>().join(",")
}

/// 解析 CSV 裡的清單欄位（多選、標籤）：JSON 陣列，或舊版 `tag_opt` 用的 Python 清單寫法。
pub fn parse_list_cell(cell: &str) -> Result<Vec<String>, String> {
    let s = cell.trim();
    if s.is_empty() {
        return Ok(vec![]);
    }
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(s) {
        return arr
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                other => Err(format!("清單裡要都是文字，收到 {other}")),
            })
            .collect();
    }
    parse_python_list(s).ok_or_else(|| format!("「{s}」不是清單，請寫成 JSON 陣列，例如 [\"a\", \"b\"]"))
}

/// 解析 Python 的 `repr(list_of_str)`，例如 `['政治', "it's", 'a\\b']`。
pub fn parse_python_list(s: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = s.trim().chars().collect();
    let mut i = 0;
    let skip_ws = |i: &mut usize| {
        while *i < chars.len() && chars[*i].is_whitespace() {
            *i += 1;
        }
    };
    if chars.first() != Some(&'[') {
        return None;
    }
    i += 1;
    let mut out = Vec::new();
    loop {
        skip_ws(&mut i);
        match chars.get(i)? {
            ']' if out.is_empty() => {
                i += 1;
                break;
            }
            q @ ('\'' | '"') => {
                let quote = *q;
                i += 1;
                let mut item = String::new();
                loop {
                    let c = *chars.get(i)?;
                    i += 1;
                    if c == quote {
                        break;
                    }
                    if c != '\\' {
                        item.push(c);
                        continue;
                    }
                    let e = *chars.get(i)?;
                    i += 1;
                    match e {
                        'n' => item.push('\n'),
                        't' => item.push('\t'),
                        'r' => item.push('\r'),
                        '\\' | '\'' | '"' => item.push(e),
                        'x' | 'u' | 'U' => {
                            let len = match e {
                                'x' => 2,
                                'u' => 4,
                                _ => 8,
                            };
                            let hex: String = chars.get(i..i + len)?.iter().collect();
                            i += len;
                            item.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                        }
                        other => {
                            item.push('\\');
                            item.push(other);
                        }
                    }
                }
                out.push(item);
                skip_ws(&mut i);
                match chars.get(i)? {
                    ',' => i += 1,
                    ']' => {
                        i += 1;
                        break;
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    skip_ws(&mut i);
    (i == chars.len()).then_some(out)
}

/// 舊版的當地時間轉成 Unix 秒數。`offset_minutes` 是舊伺服器所在時區和 UTC 的差（台灣是 +480）。
pub fn parse_legacy_date(s: &str, offset_minutes: i32) -> Option<i64> {
    let naive = NaiveDateTime::parse_from_str(s.trim(), DATE_FORMAT).ok()?;
    let offset = FixedOffset::east_opt(offset_minutes * 60)?;
    offset.from_local_datetime(&naive).single().map(|t| t.timestamp())
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImport {
    pub project_id: i64,
    pub items: usize,
    pub hidden_items: usize,
    pub annotations: usize,
    /// 找不到對應文字的標注（舊版沒有外鍵限制），略過不匯入。
    pub orphan_labels: usize,
    /// 舊資料裡出現、但舊版介面沒有的選項，已經加進新專案的選項裡。
    pub added_options: Vec<String>,
}

struct LegacyLabel {
    emotion_id: i64,
    score: f64,
    tag: String,
    kind: String,
    tag_opt: String,
    update_date: String,
}

/// 把舊的 emoLabel.db 匯入成一個新專案。舊檔只會被讀取，不會被改動。
pub async fn import_db(db: &SqlitePool, path: &Path, name: &str, offset_minutes: i32) -> Result<LegacyImport> {
    let old = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
        .context("打不開這個檔案，請確認它是 SQLite 資料庫")?;
    let result = read_and_import(&old, db, name, offset_minutes).await;
    old.close().await;
    result
}

async fn read_and_import(old: &SqlitePool, db: &SqlitePool, name: &str, offset_minutes: i32) -> Result<LegacyImport> {
    for (table, columns) in [
        ("emotion", &["id", "text", "date", "flag"][..]),
        (
            "label",
            &["emotionId", "score", "tag", "type", "tag_opt", "updateDate"][..],
        ),
    ] {
        let found: Vec<String> = sqlx::query(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(old)
            .await
            .context("讀不到資料表結構，這不是 SQLite 資料庫嗎？")?
            .iter()
            .map(|r| r.get::<String, _>(0))
            .collect();
        if found.is_empty() {
            bail!("這不是舊版 emoLabel 的資料庫：找不到 {table} 資料表");
        }
        for c in columns {
            if !found.iter().any(|f| f == c) {
                bail!("這不是舊版 emoLabel 的資料庫：{table} 資料表少了 {c} 欄位");
            }
        }
    }

    // text 欄位宣告成 INTEGER，純數字的文字會變成整數，用 CAST 轉回字串
    let emotions: Vec<(i64, String, String, i64)> =
        sqlx::query_as("SELECT id, CAST(text AS TEXT), CAST(date AS TEXT), flag FROM emotion ORDER BY id")
            .fetch_all(old)
            .await?;
    let labels: Vec<LegacyLabel> = sqlx::query(
        "SELECT emotionId, score, CAST(tag AS TEXT), CAST(type AS TEXT), CAST(tag_opt AS TEXT), \
         CAST(updateDate AS TEXT) FROM label ORDER BY id",
    )
    .fetch_all(old)
    .await?
    .into_iter()
    .map(|r| LegacyLabel {
        emotion_id: r.get(0),
        score: r.get(1),
        tag: r.get(2),
        kind: r.get(3),
        tag_opt: r.get(4),
        update_date: r.get(5),
    })
    .collect();

    // 舊資料裡有、但舊版介面沒有的選項，補進選項清單，讓這些標注在新版也合法
    let mut schema = legacy_emotion_schema();
    let mut added = Vec::new();
    for (field_id, seen) in [
        ("tag", labels.iter().map(|l| l.tag.clone()).collect::<BTreeSet<_>>()),
        ("type", labels.iter().map(|l| l.kind.clone()).collect::<BTreeSet<_>>()),
    ] {
        let field = schema
            .fields
            .iter_mut()
            .find(|f| f.id == field_id)
            .expect("legacy schema has field");
        if let FieldKind::Single { options, .. } = &mut field.kind {
            for value in seen {
                if !value.trim().is_empty() && !options.iter().any(|o| o.value == value) {
                    added.push(format!("{field_id}={value}"));
                    options.push(Choice::new(&value, &value, ""));
                }
            }
        }
    }
    schema
        .validate()
        .map_err(|e| anyhow::anyhow!("舊資料的選項無法轉換：{}", e.join("；")))?;

    let ai = AiSettings {
        instructions: crate::schema::templates()
            .into_iter()
            .find(|t| t.id == "legacy-emotion")
            .map(|t| t.instructions.to_string())
            .unwrap_or_default(),
        ..Default::default()
    };
    let mut tx = crate::db::write_tx(db).await?;
    let project_id = models::insert_project(&mut tx, name, "從舊版 emoLabel.db 匯入", &schema, &ai).await?;
    let mut report = LegacyImport {
        project_id,
        added_options: added,
        ..Default::default()
    };
    let mut id_map = std::collections::HashMap::new();
    let fallback_time = now();
    for (old_id, text, date, flag) in &emotions {
        let hidden = *flag != 1;
        let meta = json!({ "legacy_id": old_id, "legacy_date": date });
        let created = parse_legacy_date(date, offset_minutes).unwrap_or(fallback_time);
        let item = models::NewItem {
            text: text.clone(),
            media_id: None,
            media_sha: None,
            meta,
            hidden,
            created_at: created,
        };
        if let Some(new_id) = models::insert_item(&mut tx, project_id, &item).await? {
            id_map.insert(*old_id, new_id);
            report.items += 1;
            report.hidden_items += usize::from(hidden);
        }
    }
    for (i, l) in labels.iter().enumerate() {
        let Some(&item_id) = id_map.get(&l.emotion_id) else {
            report.orphan_labels += 1;
            continue;
        };
        let tags =
            parse_list_cell(&l.tag_opt).map_err(|e| anyhow::anyhow!("第 {} 筆標注的自訂標籤無法解析：{e}", i + 1))?;
        let mut raw = Values::new();
        raw.insert("score".into(), json!(l.score));
        raw.insert("tag".into(), json!(l.tag));
        raw.insert("type".into(), json!(l.kind));
        raw.insert("tag_opt".into(), json!(tags));
        let values = schema
            .validate_values(&raw)
            .map_err(|e| anyhow::anyhow!("第 {} 筆標注不合法：{}", i + 1, e.join("；")))?;
        let at = parse_legacy_date(&l.update_date, offset_minutes).unwrap_or(fallback_time);
        models::insert_annotation(
            &mut tx,
            &NewAnnotation {
                item_id,
                annotator_id: None,
                values,
                source: Source::Legacy,
                suggestion_id: None,
                at,
            },
        )
        .await?;
        report.annotations += 1;
    }
    tx.commit().await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_list_repr() {
        assert_eq!(parse_python_list("[]"), Some(vec![]));
        assert_eq!(
            parse_python_list("['政治', '城市']"),
            Some(vec!["政治".into(), "城市".into()])
        );
        assert_eq!(
            parse_python_list(r#"["it's", 'a\\b', 'x\ny', 'é\x41']"#),
            Some(vec!["it's".into(), "a\\b".into(), "x\ny".into(), "éA".into()])
        );
        assert_eq!(parse_python_list("['a',]"), None);
        assert_eq!(parse_python_list("['a'"), None);
        assert_eq!(parse_python_list("a, b"), None);
    }

    #[test]
    fn list_cells_accept_json_and_python() {
        assert_eq!(parse_list_cell(r#"["城市", "政治"]"#).unwrap(), vec!["城市", "政治"]);
        assert_eq!(parse_list_cell("['城市']").unwrap(), vec!["城市"]);
        assert_eq!(parse_list_cell("  ").unwrap(), Vec::<String>::new());
        assert!(parse_list_cell("城市").is_err());
        assert!(parse_list_cell("[1, 2]").is_err());
    }

    #[test]
    fn legacy_dates_use_the_given_offset() {
        // 2019-05-24 13:05:01 台灣時間 = 05:05:01 UTC
        assert_eq!(parse_legacy_date("2019-05-24 13:05:01", 480), Some(1_558_674_301));
        assert_eq!(parse_legacy_date("not a date", 480), None);
    }

    #[test]
    fn columns_map_to_fields_only_when_the_field_exists() {
        let legacy = legacy_emotion_schema();
        assert_eq!(canonical_column("Text", &legacy), "text");
        assert_eq!(canonical_column("Level", &legacy), "score");
        assert_eq!(canonical_column("Custom classes", &legacy), "tag_opt");
        let other = crate::schema::templates()
            .into_iter()
            .find(|t| t.id == "sentiment")
            .unwrap()
            .schema;
        assert_eq!(canonical_column("Level", &other), "Level");
    }
}
