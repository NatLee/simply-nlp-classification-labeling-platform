//! 標注佇列（下一筆要標哪一筆）和統計（進度、分布、一致性）。

use std::collections::{BTreeMap, HashMap};

use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::{
    agreement,
    error::AppResult,
    models::{self, Annotation, Project},
    schema::FieldKind,
};

// ───────────────────────────── 佇列 ─────────────────────────────

/// 舊版的「票箱」加權抽樣：被標過越少次的資料越容易被抽到。
/// 每筆的權重 = 所有可標資料裡最多的標注數 − 這筆的標注數 + 1。
/// 新版只從這位標注者還沒標過的資料裡抽；`exclude` 是剛剛跳過的資料，盡量不要馬上又抽到。
pub fn weighted_pick(candidates: &[(i64, i64)], max_votes: i64, rng: &mut impl Rng) -> Option<i64> {
    let weights: Vec<i64> = candidates
        .iter()
        .map(|(_, votes)| (max_votes - votes + 1).max(1))
        .collect();
    let total: i64 = weights.iter().sum();
    if total <= 0 {
        return None;
    }
    let mut ticket = rng.random_range(0..total);
    for ((id, _), w) in candidates.iter().zip(weights) {
        if ticket < w {
            return Some(*id);
        }
        ticket -= w;
    }
    None
}

#[derive(sqlx::FromRow)]
struct QueueRow {
    id: i64,
    votes: i64,
    mine: bool,
}

pub async fn next_item(db: &SqlitePool, project_id: i64, annotator_id: i64, exclude: &[i64]) -> AppResult<Option<i64>> {
    let rows: Vec<QueueRow> = sqlx::query_as(
        "SELECT i.id, \
           (SELECT COUNT(*) FROM annotations a WHERE a.item_id = i.id) AS votes, \
           EXISTS (SELECT 1 FROM annotations a WHERE a.item_id = i.id AND a.annotator_id = ?) AS mine \
         FROM items i WHERE i.project_id = ? AND i.hidden = 0",
    )
    .bind(annotator_id)
    .bind(project_id)
    .fetch_all(db)
    .await?;
    let max_votes = rows.iter().map(|r| r.votes).max().unwrap_or(0);
    let open: Vec<(i64, i64)> = rows.iter().filter(|r| !r.mine).map(|r| (r.id, r.votes)).collect();
    let fresh: Vec<(i64, i64)> = open.iter().copied().filter(|(id, _)| !exclude.contains(id)).collect();
    // 剩下的都跳過了，就從跳過的裡面再抽
    let pool = if fresh.is_empty() { &open } else { &fresh };
    Ok(weighted_pick(pool, max_votes, &mut rand::rng()))
}

// ───────────────────────────── 統計 ─────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub items: ItemStats,
    pub mine: Option<i64>,
    pub annotators: Vec<AnnotatorCount>,
    pub anonymous: usize,
    pub sources: BTreeMap<String, usize>,
    pub suggestions: BTreeMap<String, i64>,
    pub fields: Vec<FieldStats>,
}

/// 和舊版儀表板一樣的三個數字：覆蓋率、每筆被標次數的平均和標準差（只算可標注的資料）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemStats {
    pub total: i64,
    pub visible: i64,
    pub hidden: i64,
    pub labeled: i64,
    pub cover_rate: f64,
    pub mean_labels: f64,
    pub std_labels: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotatorCount {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldStats {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// 每個值被選了幾次（分數欄位是每個分數出現幾次）。
    pub distribution: Vec<Bucket>,
    pub agreement: Option<Agreement>,
}

#[derive(Debug, Serialize)]
pub struct Bucket {
    pub value: Value,
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agreement {
    /// 至少被標兩次、用來算一致性的資料筆數。
    pub items_used: usize,
    pub fleiss_kappa: Option<f64>,
    pub krippendorff_alpha: Option<f64>,
    pub pairs: Vec<PairAgreement>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairAgreement {
    pub a: String,
    pub b: String,
    pub n: usize,
    pub cohen_kappa: Option<f64>,
    pub percent: Option<f64>,
}

/// 列出幾組兩兩比較。
const MAX_PAIRS: usize = 30;
/// 自由標籤列出前幾名。
const TOP_TAGS: usize = 20;

pub async fn project_stats(db: &SqlitePool, project: &Project, annotator_id: Option<i64>) -> AppResult<Stats> {
    let items = models::all_items(db, project.id).await?;
    let anns = models::project_annotations(db, project.id).await?;
    let hidden: std::collections::HashSet<i64> = items.iter().filter(|i| i.hidden).map(|i| i.id).collect();
    let mut by_item: HashMap<i64, Vec<&Annotation>> = HashMap::new();
    for a in &anns {
        by_item.entry(a.item_id).or_default().push(a);
    }

    let visible: Vec<i64> = items.iter().filter(|i| !i.hidden).map(|i| i.id).collect();
    let counts: Vec<f64> = visible
        .iter()
        .map(|id| by_item.get(id).map_or(0, Vec::len) as f64)
        .collect();
    let labeled = counts.iter().filter(|&&c| c > 0.0).count() as i64;
    let n = counts.len() as f64;
    let mean = if n > 0.0 { counts.iter().sum::<f64>() / n } else { 0.0 };
    let std = if n > 0.0 {
        (counts.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / n).sqrt()
    } else {
        0.0
    };
    let item_stats = ItemStats {
        total: items.len() as i64,
        visible: visible.len() as i64,
        hidden: hidden.len() as i64,
        labeled,
        cover_rate: if n > 0.0 { labeled as f64 / n } else { 0.0 },
        mean_labels: mean,
        std_labels: std,
    };

    let mut per_annotator: BTreeMap<i64, AnnotatorCount> = BTreeMap::new();
    let mut anonymous = 0;
    let mut sources = BTreeMap::new();
    for a in &anns {
        *sources.entry(a.source.clone()).or_insert(0) += 1;
        match (a.annotator_id, &a.annotator_name) {
            (Some(id), Some(name)) => {
                per_annotator
                    .entry(id)
                    .or_insert_with(|| AnnotatorCount {
                        id,
                        name: name.clone(),
                        kind: a.annotator_kind.clone().unwrap_or_default(),
                        count: 0,
                    })
                    .count += 1
            }
            _ => anonymous += 1,
        }
    }
    let mut annotators: Vec<AnnotatorCount> = per_annotator.into_values().collect();
    annotators.sort_by_key(|a| std::cmp::Reverse(a.count));
    let mine = annotator_id.map(|id| {
        anns.iter()
            .filter(|a| a.annotator_id == Some(id) && !hidden.contains(&a.item_id))
            .count() as i64
    });

    let suggestions: BTreeMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
        "SELECT s.status, COUNT(*) FROM suggestions s JOIN items i ON i.id = s.item_id WHERE i.project_id = ? GROUP BY s.status",
    )
    .bind(project.id)
    .fetch_all(db)
    .await?
    .into_iter()
    .collect();

    let fields = project
        .schema
        .fields
        .iter()
        .map(|f| field_stats(f, &anns, &by_item, &hidden))
        .collect();

    Ok(Stats {
        items: item_stats,
        mine,
        annotators,
        anonymous,
        sources,
        suggestions,
        fields,
    })
}

fn field_stats(
    f: &crate::schema::Field,
    anns: &[Annotation],
    by_item: &HashMap<i64, Vec<&Annotation>>,
    hidden: &std::collections::HashSet<i64>,
) -> FieldStats {
    let values = || anns.iter().filter_map(|a| a.values.get(&f.id));
    let mut distribution = Vec::new();
    let mut agreement = None;
    match &f.kind {
        FieldKind::Single { options, .. } | FieldKind::Multi { options, .. } => {
            for o in options {
                let count = values()
                    .filter(|v| {
                        v.as_str() == Some(&o.value)
                            || v.as_array()
                                .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(&o.value)))
                    })
                    .count();
                let label = if o.emoji.is_empty() {
                    o.display().to_string()
                } else {
                    format!("{} {}", o.emoji, o.display())
                };
                distribution.push(Bucket {
                    value: Value::from(o.value.clone()),
                    label,
                    count,
                });
            }
            if let FieldKind::Single { options, .. } = &f.kind {
                agreement = Some(single_agreement(
                    &f.id,
                    options.len(),
                    |s| options.iter().position(|o| o.value == s),
                    by_item,
                    hidden,
                ));
            }
        }
        FieldKind::Scale { .. } => {
            let mut counts: BTreeMap<String, (f64, usize)> = BTreeMap::new();
            for x in values().filter_map(Value::as_f64) {
                let e = counts.entry(format!("{x:020.6}")).or_insert((x, 0));
                e.1 += 1;
            }
            distribution = counts
                .into_values()
                .map(|(x, count)| Bucket {
                    value: Value::from(x),
                    label: trim_float(x),
                    count,
                })
                .collect();
            let units: Vec<Vec<f64>> = by_item
                .iter()
                .filter(|(id, _)| !hidden.contains(id))
                .map(|(_, list)| {
                    list.iter()
                        .filter_map(|a| a.values.get(&f.id).and_then(Value::as_f64))
                        .collect::<Vec<_>>()
                })
                .collect();
            agreement = Some(Agreement {
                items_used: units.iter().filter(|u| u.len() >= 2).count(),
                krippendorff_alpha: agreement::krippendorff_alpha_interval(&units),
                ..Default::default()
            });
        }
        FieldKind::Tags { .. } => {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for v in values() {
                for t in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    *counts.entry(t.to_string()).or_default() += 1;
                }
            }
            let mut sorted: Vec<(String, usize)> = counts.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            distribution = sorted
                .into_iter()
                .take(TOP_TAGS)
                .map(|(t, count)| Bucket {
                    value: Value::from(t.clone()),
                    label: t,
                    count,
                })
                .collect();
        }
        FieldKind::Text { .. } => {}
    }
    FieldStats {
        id: f.id.clone(),
        label: f.label.clone(),
        kind: f.kind.type_name(),
        distribution,
        agreement,
    }
}

fn trim_float(x: f64) -> String {
    let s = format!("{x:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn single_agreement(
    field_id: &str,
    k: usize,
    index: impl Fn(&str) -> Option<usize>,
    by_item: &HashMap<i64, Vec<&Annotation>>,
    hidden: &std::collections::HashSet<i64>,
) -> Agreement {
    let mut table = Vec::new();
    // 具名標注者 → (資料 → 類別)
    let mut per_annotator: BTreeMap<String, HashMap<i64, usize>> = BTreeMap::new();
    for (item_id, list) in by_item.iter().filter(|(id, _)| !hidden.contains(id)) {
        let mut row = vec![0usize; k];
        for a in list {
            let Some(j) = a.values.get(field_id).and_then(Value::as_str).and_then(&index) else {
                continue;
            };
            row[j] += 1;
            if let Some(name) = &a.annotator_name {
                per_annotator.entry(name.clone()).or_default().insert(*item_id, j);
            }
        }
        table.push(row);
    }
    let items_used = table.iter().filter(|r| r.iter().sum::<usize>() >= 2).count();
    let names: Vec<&String> = per_annotator.keys().collect();
    let mut pairs = Vec::new();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            let (la, lb) = (&per_annotator[*a], &per_annotator[*b]);
            let both: Vec<(usize, usize)> = la
                .iter()
                .filter_map(|(item, &x)| lb.get(item).map(|&y| (x, y)))
                .collect();
            if both.is_empty() {
                continue;
            }
            pairs.push(PairAgreement {
                a: (*a).clone(),
                b: (*b).clone(),
                n: both.len(),
                cohen_kappa: agreement::cohen_kappa(&both, k),
                percent: agreement::percent_agreement(&both),
            });
        }
    }
    pairs.sort_by_key(|x| std::cmp::Reverse(x.n));
    pairs.truncate(MAX_PAIRS);
    Agreement {
        items_used,
        fleiss_kappa: agreement::fleiss_kappa(&table),
        krippendorff_alpha: None,
        pairs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn weighted_pick_favours_items_with_fewer_labels() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        // 最多 3 票：沒人標過的權重 4，標過 3 次的權重 1
        let candidates = [(1, 0), (2, 3)];
        let mut hits = [0usize; 2];
        for _ in 0..10_000 {
            match weighted_pick(&candidates, 3, &mut rng) {
                Some(1) => hits[0] += 1,
                Some(2) => hits[1] += 1,
                other => panic!("{other:?}"),
            }
        }
        let ratio = hits[0] as f64 / hits[1] as f64;
        assert!((3.5..4.5).contains(&ratio), "expected about 4:1, got {hits:?}");
        assert_eq!(weighted_pick(&[], 0, &mut rng), None);
    }
}
