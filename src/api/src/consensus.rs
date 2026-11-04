//! 多人標同一筆資料時，合併成一份「共識」標注（匯出、微調資料用）。
//!
//! 規則都是「過半數」：
//! - 分數：平均後對齊到最近的一格。
//! - 單選：超過一半的人選的那個；沒有過半就不給。
//! - 多選、標籤：超過一半的人選的項目。沒填這一欄的人算是「一個都沒選」。
//! - 文字說明：最後一筆有寫的。

use serde_json::Value;

use crate::schema::{snap, FieldKind, Schema, Values};

/// `annotations` 依時間由舊到新排列。
pub fn consensus(schema: &Schema, annotations: &[&Values]) -> Values {
    let mut out = Values::new();
    if annotations.is_empty() {
        return out;
    }
    for f in &schema.fields {
        let present: Vec<&Value> = annotations
            .iter()
            .filter_map(|a| a.get(&f.id))
            .filter(|v| !v.is_null())
            .collect();
        if present.is_empty() {
            continue;
        }
        let voters = present.len();
        let value = match &f.kind {
            FieldKind::Scale { min, max, step, .. } => {
                let nums: Vec<f64> = present.iter().filter_map(|v| v.as_f64()).collect();
                (!nums.is_empty())
                    .then(|| Value::from(snap(nums.iter().sum::<f64>() / nums.len() as f64, *min, *max, *step)))
            }
            FieldKind::Single { options, .. } => options
                .iter()
                .map(|o| (o, present.iter().filter(|v| v.as_str() == Some(&o.value)).count()))
                .find(|(_, c)| 2 * c > voters)
                .map(|(o, _)| Value::from(o.value.clone())),
            FieldKind::Multi { .. } | FieldKind::Tags { .. } => {
                let voters = annotations.len();
                let mut items: Vec<String> = Vec::new();
                for v in &present {
                    for s in v.as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
                        if !items.iter().any(|i| i == s) {
                            items.push(s.to_string());
                        }
                    }
                }
                let kept: Vec<String> = items
                    .into_iter()
                    .filter(|s| {
                        let votes = present
                            .iter()
                            .filter(|v| v.as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(s))))
                            .count();
                        2 * votes > voters
                    })
                    .collect();
                (!kept.is_empty()).then(|| Value::from(kept))
            }
            FieldKind::Text { .. } => present.last().map(|v| (*v).clone()),
        };
        if let Some(v) = value {
            out.insert(f.id.clone(), v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::legacy_emotion_schema;
    use serde_json::json;

    fn v(x: Value) -> Values {
        x.as_object().unwrap().clone()
    }

    #[test]
    fn majority_rules() {
        let s = legacy_emotion_schema();
        let a = v(json!({"score": 4, "tag": "happy", "type": "閒聊", "tag_opt": ["政治", "城市"]}));
        let b = v(json!({"score": 5, "tag": "happy", "type": "問答", "tag_opt": ["政治"]}));
        let c = v(json!({"score": 2, "tag": "sad", "type": "天氣"}));
        let out = consensus(&s, &[&a, &b, &c]);
        assert_eq!(out["score"], json!(4.0)); // (4+5+2)/3 = 3.67 → 4
        assert_eq!(out["tag"], json!("happy"));
        assert!(!out.contains_key("type")); // 三個人三種答案，沒有過半
        assert_eq!(out["tag_opt"], json!(["政治"])); // 三人中兩人選了政治，城市只有一人
    }

    #[test]
    fn tie_gives_no_single_choice() {
        let s = legacy_emotion_schema();
        let a = v(json!({"tag": "happy"}));
        let b = v(json!({"tag": "sad"}));
        assert!(!consensus(&s, &[&a, &b]).contains_key("tag"));
        assert_eq!(consensus(&s, &[&a])["tag"], json!("happy"));
    }
}
