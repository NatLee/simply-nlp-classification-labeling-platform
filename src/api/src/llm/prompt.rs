//! 提示詞。AI 輔助標注和「聊天格式」匯出共用同一套，拿匯出的資料微調出來的模型，
//! 接回這個平台當 OpenAI 相容的服務時，看到的提示和訓練時一樣。

use std::fmt::Write;

use crate::{
    models::{Item, Project},
    schema::FieldKind,
};

/// 理由用什麼語言寫。介面是繁體中文，理由也用繁體中文。
pub const RATIONALE_LANGUAGE: &str = "Traditional Chinese (zh-TW)";

/// `with_meta`：要不要請模型附上信心和理由（AI 輔助要；微調資料只要標注值）。
pub fn system_prompt(project: &Project, with_meta: bool) -> String {
    let mut s = String::from(
        "You are a careful data annotator for a classification dataset. \
         Read the item (text, image, or both) and fill in every field of the annotation form.\n\n",
    );
    if !project.ai.instructions.trim().is_empty() {
        let _ = writeln!(s, "<guidelines>\n{}\n</guidelines>\n", project.ai.instructions.trim());
    }
    s.push_str("<fields>\n");
    for f in &project.schema.fields {
        let _ = write!(s, "- {} ({}): {}", f.id, f.label, f.kind.type_name());
        if !f.description.is_empty() {
            let _ = write!(s, " — {}", f.description);
        }
        if f.required {
            s.push_str(" [required]");
        }
        s.push('\n');
        match &f.kind {
            FieldKind::Scale {
                min,
                max,
                step,
                min_label,
                max_label,
                ..
            } => {
                let _ = writeln!(s, "  a number from {min} to {max} in steps of {step}");
                if !min_label.is_empty() || !max_label.is_empty() {
                    let _ = writeln!(s, "  {min} means \"{min_label}\", {max} means \"{max_label}\"");
                }
            }
            FieldKind::Single { options, .. } | FieldKind::Multi { options, .. } => {
                let how = if matches!(f.kind, FieldKind::Single { .. }) {
                    "pick exactly one"
                } else {
                    "pick any number"
                };
                let _ = writeln!(s, "  {how} of:");
                for o in options {
                    let _ = write!(s, "  - {}", o.value);
                    if !o.label.is_empty() && o.label != o.value {
                        let _ = write!(s, " ({})", o.label);
                    }
                    if !o.description.is_empty() {
                        let _ = write!(s, ": {}", o.description);
                    }
                    s.push('\n');
                }
            }
            FieldKind::Tags { suggestions } => {
                s.push_str("  a list of short free-form tags (can be empty)");
                if !suggestions.is_empty() {
                    let _ = write!(s, "; commonly used: {}", suggestions.join(", "));
                }
                s.push('\n');
            }
            FieldKind::Text { max_length } => {
                let _ = writeln!(
                    s,
                    "  a short note, at most {max_length} characters (null if nothing to add)"
                );
            }
        }
    }
    s.push_str("</fields>\n\n");
    if with_meta {
        let _ = write!(
            s,
            "Answer with JSON only: {{\"values\": {{...}}, \"confidence\": 0-1, \"rationale\": \"...\"}}. \
             Use null for a field you cannot decide. `confidence` is your probability that a careful human annotator \
             would pick exactly the same values; be honest, low confidence is useful. \
             Write `rationale` in {RATIONALE_LANGUAGE}, one or two sentences."
        );
    } else {
        s.push_str("Answer with JSON only: {\"values\": {...}}. Use null for a field you cannot decide.");
    }
    s
}

/// 使用者訊息裡的文字部分。
pub fn user_text(item: &Item) -> String {
    let has_image = item.media.as_ref().is_some_and(|m| m.kind == "image");
    match (item.text.trim().is_empty(), has_image) {
        (false, _) => format!("<item>\n{}\n</item>", item.text.trim()),
        (true, true) => "<item>\n(no text; annotate the image)\n</item>".into(),
        (true, false) => "<item>\n(empty)\n</item>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::AiSettings, schema::legacy_emotion_schema};

    fn project() -> Project {
        Project {
            id: 1,
            name: "p".into(),
            description: String::new(),
            schema: legacy_emotion_schema(),
            ai: AiSettings {
                instructions: "1 是很負面".into(),
                auto_accept_threshold: None,
            },
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn prompt_lists_fields_options_and_guidelines() {
        let s = system_prompt(&project(), true);
        assert!(s.contains("<guidelines>\n1 是很負面"));
        assert!(s.contains("- score (評分): scale"));
        assert!(s.contains("a number from 1 to 5 in steps of 1"));
        assert!(s.contains("- 媒體指令"));
        assert!(s.contains("confidence"));
        assert!(!system_prompt(&project(), false).contains("rationale"));
    }
}
