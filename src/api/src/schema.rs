//! 標注結構（schema）：每個專案要標哪些欄位，以及怎麼檢查一筆標注是否有效。
//!
//! 欄位有五種：連續分數（scale）、單選（single）、多選（multi）、自由標籤（tags）、文字說明（text）。
//! 一筆標注是 JSON 物件，鍵是欄位 id，例如 `{"score": 3, "tag": "happy", "tag_opt": ["政治"]}`。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub type Values = Map<String, Value>;

pub const MAX_FIELDS: usize = 50;
pub const MAX_OPTIONS: usize = 200;
pub const MAX_SCALE_STEPS: f64 = 10_000.0;
pub const MAX_TAGS: usize = 100;
pub const MAX_TAG_CHARS: usize = 100;
pub const MAX_TEXT_CHARS: usize = 20_000;
pub const DEFAULT_TEXT_CHARS: usize = 2_000;
const STEP_EPSILON: f64 = 1e-6;

/// 匯出時固定使用的欄位名稱，標注欄位的 id 不能和它們撞名。
pub const RESERVED_IDS: &[&str] = &[
    "id",
    "item_id",
    "text",
    "media",
    "image",
    "audio",
    "file_name",
    "meta",
    "hidden",
    "annotator",
    "source",
    "created_at",
    "updated_at",
    "annotations",
    "consensus",
    "n_annotations",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Schema {
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(flatten)]
    pub kind: FieldKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FieldKind {
    /// 連續分數滑桿。
    Scale {
        min: f64,
        max: f64,
        step: f64,
        #[serde(default)]
        default: Option<f64>,
        #[serde(default)]
        min_label: String,
        #[serde(default)]
        max_label: String,
    },
    /// 單選。
    Single {
        options: Vec<Choice>,
        #[serde(default)]
        default: Option<String>,
    },
    /// 多選。
    Multi {
        options: Vec<Choice>,
        #[serde(default)]
        max_selected: Option<usize>,
    },
    /// 自由標籤：標注者可以自己打字新增。
    Tags {
        #[serde(default)]
        suggestions: Vec<String>,
    },
    /// 文字說明。
    Text {
        #[serde(default = "default_text_chars")]
        max_length: usize,
    },
}

fn default_text_chars() -> usize {
    DEFAULT_TEXT_CHARS
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub value: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub emoji: String,
    #[serde(default)]
    pub description: String,
}

impl Choice {
    pub fn new(value: &str, label: &str, emoji: &str) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            emoji: emoji.into(),
            description: String::new(),
        }
    }
    pub fn display(&self) -> &str {
        if self.label.is_empty() {
            &self.value
        } else {
            &self.label
        }
    }
}

impl FieldKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Scale { .. } => "scale",
            Self::Single { .. } => "single",
            Self::Multi { .. } => "multi",
            Self::Tags { .. } => "tags",
            Self::Text { .. } => "text",
        }
    }

    /// 這個欄位的值是不是字串陣列（多選、自由標籤）。
    pub fn is_list(&self) -> bool {
        matches!(self, Self::Multi { .. } | Self::Tags { .. })
    }
}

/// 把分數對齊到最接近的一格，並限制在範圍內。
pub fn snap(value: f64, min: f64, max: f64, step: f64) -> f64 {
    let steps = ((value - min) / step).round();
    let snapped = (min + steps * step).clamp(min, max);
    // 去掉 0.1 + 0.2 這類浮點誤差
    (snapped * 1e9).round() / 1e9
}

fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some('a'..='z')) && id.len() <= 40 && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

impl Schema {
    pub fn field(&self, id: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.id == id)
    }

    /// 檢查結構本身是否合理。回傳所有問題，不是只回第一個。
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.fields.is_empty() {
            errors.push("至少要有一個標注欄位".into());
        }
        if self.fields.len() > MAX_FIELDS {
            errors.push(format!("欄位最多 {MAX_FIELDS} 個"));
        }
        let mut ids = HashSet::new();
        for f in &self.fields {
            let name = if f.label.trim().is_empty() {
                f.id.clone()
            } else {
                f.label.clone()
            };
            if !is_valid_id(&f.id) {
                errors.push(format!(
                    "「{name}」的 id「{}」要用小寫英文字母開頭，只能有小寫英數字和底線，最多 40 個字元",
                    f.id
                ));
            }
            if RESERVED_IDS.contains(&f.id.as_str()) {
                errors.push(format!("id「{}」是匯出檔保留的欄位名稱，請換一個", f.id));
            }
            if !ids.insert(f.id.as_str()) {
                errors.push(format!("id「{}」重複了", f.id));
            }
            if f.label.trim().is_empty() {
                errors.push(format!("欄位「{}」要有顯示名稱", f.id));
            }
            match &f.kind {
                FieldKind::Scale {
                    min,
                    max,
                    step,
                    default,
                    ..
                } => {
                    if !(min.is_finite() && max.is_finite() && step.is_finite()) || min >= max {
                        errors.push(format!("「{name}」的最小值要小於最大值"));
                    } else if *step <= 0.0 {
                        errors.push(format!("「{name}」的間隔要大於 0"));
                    } else {
                        let steps = (max - min) / step;
                        if steps > MAX_SCALE_STEPS {
                            errors.push(format!("「{name}」的格數太多（最多 {MAX_SCALE_STEPS} 格）"));
                        } else if (steps - steps.round()).abs() > STEP_EPSILON {
                            errors.push(format!("「{name}」的範圍要能被間隔整除"));
                        }
                        if let Some(d) = default {
                            if (snap(*d, *min, *max, *step) - d).abs() > STEP_EPSILON {
                                errors.push(format!("「{name}」的預設值要在範圍內，而且落在格子上"));
                            }
                        }
                    }
                }
                FieldKind::Single { options, default } => {
                    check_options(&name, options, 2, &mut errors);
                    if let Some(d) = default {
                        if !options.iter().any(|o| &o.value == d) {
                            errors.push(format!("「{name}」的預設值「{d}」不在選項裡"));
                        }
                    }
                }
                FieldKind::Multi { options, max_selected } => {
                    check_options(&name, options, 1, &mut errors);
                    if let Some(m) = max_selected {
                        if *m == 0 || *m > options.len() {
                            errors.push(format!("「{name}」最多可選幾個，要介於 1 和選項數之間"));
                        }
                    }
                }
                FieldKind::Tags { suggestions } => {
                    let mut seen = HashSet::new();
                    for s in suggestions {
                        if s.trim().is_empty() || s.chars().count() > MAX_TAG_CHARS {
                            errors.push(format!(
                                "「{name}」的建議標籤不能是空的，也不能超過 {MAX_TAG_CHARS} 個字"
                            ));
                        } else if !seen.insert(s.trim()) {
                            errors.push(format!("「{name}」的建議標籤「{s}」重複了"));
                        }
                    }
                }
                FieldKind::Text { max_length } => {
                    if *max_length == 0 || *max_length > MAX_TEXT_CHARS {
                        errors.push(format!("「{name}」的字數上限要介於 1 和 {MAX_TEXT_CHARS}"));
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// 檢查一筆標注，回傳整理過的值（標籤去掉前後空白和重複、分數對齊到格子）。
    /// 沒有值的欄位不放進結果；`null` 視同沒有值。
    pub fn validate_values(&self, values: &Values) -> Result<Values, Vec<String>> {
        let mut errors = Vec::new();
        let mut out = Values::new();
        for key in values.keys() {
            if self.field(key).is_none() {
                errors.push(format!("沒有「{key}」這個欄位"));
            }
        }
        for f in &self.fields {
            let value = values.get(&f.id).filter(|v| !v.is_null());
            match value.map(|v| normalize(f, v)) {
                None => {
                    if f.required {
                        errors.push(format!("「{}」是必填", f.label));
                    }
                }
                Some(Err(e)) => errors.push(e),
                Some(Ok(None)) => {
                    if f.required {
                        errors.push(format!("「{}」是必填", f.label));
                    }
                }
                Some(Ok(Some(v))) => {
                    out.insert(f.id.clone(), v);
                }
            }
        }
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors)
        }
    }

    /// 模型給的建議比較寬鬆：分數超出範圍就夾回範圍內，不認得的選項丟掉，
    /// 無法使用的欄位直接略過。回傳整理後的值和被略過的原因。
    pub fn coerce_suggestion(&self, values: &Values) -> (Values, Vec<String>) {
        let mut out = Values::new();
        let mut notes = Vec::new();
        for f in &self.fields {
            let Some(v) = values.get(&f.id).filter(|v| !v.is_null()) else {
                continue;
            };
            let lenient = match (&f.kind, v) {
                (FieldKind::Scale { min, max, step, .. }, Value::Number(n)) => {
                    n.as_f64().map(|x| Value::from(snap(x, *min, *max, *step)))
                }
                (FieldKind::Multi { options, .. }, Value::Array(arr)) => Some(Value::Array(
                    arr.iter()
                        .filter(|x| x.as_str().is_some_and(|s| options.iter().any(|o| o.value == s)))
                        .cloned()
                        .collect(),
                )),
                _ => Some(v.clone()),
            };
            match lenient.map(|v| normalize(f, &v)) {
                Some(Ok(Some(v))) => {
                    out.insert(f.id.clone(), v);
                }
                Some(Ok(None)) | None => {}
                Some(Err(e)) => notes.push(e),
            }
        }
        (out, notes)
    }

    /// 給模型的 JSON Schema：`values` 裡每個欄位都列為必填，沒有答案時填 null。
    /// Claude 和 OpenAI 的結構化輸出都不支援數字範圍限制，範圍寫在 description 裡。
    pub fn suggestion_json_schema(&self, with_meta: bool) -> Value {
        let mut props = Map::new();
        for f in &self.fields {
            let (ty, desc) = match &f.kind {
                FieldKind::Scale { min, max, step, .. } => (
                    serde_json::json!({ "type": ["number", "null"] }),
                    format!("number from {min} to {max} in steps of {step}"),
                ),
                FieldKind::Single { options, .. } => {
                    let mut values: Vec<Value> = options.iter().map(|o| Value::from(o.value.clone())).collect();
                    values.push(Value::Null);
                    (serde_json::json!({ "enum": values }), "exactly one option".into())
                }
                FieldKind::Multi { options, .. } => (
                    serde_json::json!({
                        "type": "array",
                        "items": { "enum": options.iter().map(|o| o.value.clone()).collect::<Vec<_>>() }
                    }),
                    "zero or more options".into(),
                ),
                FieldKind::Tags { .. } => (
                    serde_json::json!({ "type": "array", "items": { "type": "string" } }),
                    "short free-form tags".into(),
                ),
                FieldKind::Text { .. } => (serde_json::json!({ "type": ["string", "null"] }), "short text".into()),
            };
            let mut ty = ty;
            ty["description"] = Value::from(format!("{}: {desc}", f.label));
            props.insert(f.id.clone(), ty);
        }
        let required: Vec<&str> = self.fields.iter().map(|f| f.id.as_str()).collect();
        let values = serde_json::json!({
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false,
        });
        if with_meta {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "values": values,
                    "confidence": {
                        "type": "number",
                        "description": "probability from 0 to 1 that a careful human annotator would choose exactly these values"
                    },
                    "rationale": { "type": "string", "description": "one or two sentences explaining the choice" }
                },
                "required": ["values", "confidence", "rationale"],
                "additionalProperties": false,
            })
        } else {
            serde_json::json!({
                "type": "object",
                "properties": { "values": values },
                "required": ["values"],
                "additionalProperties": false,
            })
        }
    }

    /// 結構改了之後，舊的標注還能不能用。回傳第一個不相容的原因。
    pub fn first_incompatible<'a>(&self, values: impl IntoIterator<Item = &'a Values>) -> Option<String> {
        values
            .into_iter()
            .find_map(|v| self.validate_values(v).err().map(|e| e.join("；")))
    }
}

fn check_options(name: &str, options: &[Choice], min: usize, errors: &mut Vec<String>) {
    if options.len() < min {
        errors.push(format!("「{name}」至少要有 {min} 個選項"));
    }
    if options.len() > MAX_OPTIONS {
        errors.push(format!("「{name}」最多 {MAX_OPTIONS} 個選項"));
    }
    let mut seen = HashSet::new();
    for o in options {
        if o.value.trim().is_empty() || o.value.trim() != o.value {
            errors.push(format!("「{name}」的選項值不能是空的，前後也不能有空白"));
        } else if !seen.insert(o.value.as_str()) {
            errors.push(format!("「{name}」的選項「{}」重複了", o.value));
        }
    }
}

/// 檢查並整理單一欄位的值。`Ok(None)` 表示等同於沒填（例如空陣列、空字串）。
fn normalize(f: &Field, v: &Value) -> Result<Option<Value>, String> {
    let label = &f.label;
    match &f.kind {
        FieldKind::Scale { min, max, step, .. } => {
            let x = v.as_f64().ok_or_else(|| format!("「{label}」要是數字"))?;
            if x < min - STEP_EPSILON || x > max + STEP_EPSILON {
                return Err(format!("「{label}」要介於 {min} 和 {max}"));
            }
            let snapped = snap(x, *min, *max, *step);
            if (snapped - x).abs() > STEP_EPSILON * step.max(1.0) {
                return Err(format!("「{label}」要落在每 {step} 一格的位置上"));
            }
            Ok(Some(Value::from(snapped)))
        }
        FieldKind::Single { options, .. } => {
            let s = v.as_str().ok_or_else(|| format!("「{label}」要是文字"))?;
            if options.iter().any(|o| o.value == s) {
                Ok(Some(Value::from(s)))
            } else {
                Err(format!("「{label}」沒有「{s}」這個選項"))
            }
        }
        FieldKind::Multi { options, max_selected } => {
            let list = string_list(v).ok_or_else(|| format!("「{label}」要是文字陣列"))?;
            let mut out: Vec<String> = Vec::new();
            for s in list {
                if !options.iter().any(|o| o.value == s) {
                    return Err(format!("「{label}」沒有「{s}」這個選項"));
                }
                if !out.contains(&s) {
                    out.push(s);
                }
            }
            if let Some(m) = max_selected {
                if out.len() > *m {
                    return Err(format!("「{label}」最多選 {m} 個"));
                }
            }
            // 照選項的順序排，匯出和比較時才穩定
            out.sort_by_key(|s| options.iter().position(|o| &o.value == s));
            Ok((!out.is_empty()).then(|| Value::from(out)))
        }
        FieldKind::Tags { .. } => {
            let list = string_list(v).ok_or_else(|| format!("「{label}」要是文字陣列"))?;
            let mut out: Vec<String> = Vec::new();
            for s in list {
                let t = s.trim().to_string();
                if t.is_empty() {
                    continue;
                }
                if t.chars().count() > MAX_TAG_CHARS {
                    return Err(format!("「{label}」的每個標籤最多 {MAX_TAG_CHARS} 個字"));
                }
                if !out.contains(&t) {
                    out.push(t);
                }
            }
            if out.len() > MAX_TAGS {
                return Err(format!("「{label}」最多 {MAX_TAGS} 個標籤"));
            }
            Ok((!out.is_empty()).then(|| Value::from(out)))
        }
        FieldKind::Text { max_length } => {
            let s = v.as_str().ok_or_else(|| format!("「{label}」要是文字"))?;
            let s = s.trim();
            if s.chars().count() > *max_length {
                return Err(format!("「{label}」最多 {max_length} 個字"));
            }
            Ok((!s.is_empty()).then(|| Value::from(s)))
        }
    }
}

fn string_list(v: &Value) -> Option<Vec<String>> {
    v.as_array()?.iter().map(|x| x.as_str().map(str::to_string)).collect()
}

// ───────────────────────────── 範本 ─────────────────────────────

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Schema,
    pub instructions: &'static str,
}

/// 舊版（Flask 版 emoLabel）的四個欄位。匯入舊的 emoLabel.db 時用這個結構。
pub fn legacy_emotion_schema() -> Schema {
    let types = [
        "其它",
        "日曆",
        "導覽",
        "問答",
        "輕應用",
        "媒體指令",
        "閒聊",
        "人工客服",
        "天氣",
        "導航",
        "接待",
        "驗證碼",
        "引領",
        "註冊",
        "通用指令",
    ];
    Schema {
        fields: vec![
            Field {
                id: "score".into(),
                label: "評分".into(),
                description: "1 到 5 分".into(),
                required: true,
                kind: FieldKind::Scale {
                    min: 1.0,
                    max: 5.0,
                    step: 1.0,
                    default: Some(3.0),
                    min_label: String::new(),
                    max_label: String::new(),
                },
            },
            Field {
                id: "tag".into(),
                label: "情緒（Sentiment）".into(),
                description: String::new(),
                required: true,
                kind: FieldKind::Single {
                    options: vec![
                        Choice::new("happy", "happy", "😀"),
                        Choice::new("angry", "angry", "😡"),
                        Choice::new("wow", "wow", "😲"),
                        Choice::new("sad", "sad", "😢"),
                        Choice::new("none", "none", "😐"),
                    ],
                    default: Some("wow".into()),
                },
            },
            Field {
                id: "type".into(),
                label: "類型（Type）".into(),
                // 舊版的 Level,Classes,Custom classes,Text 範本沒有這一欄，所以不設成必填；
                // 標注畫面會先帶入預設值，和舊版一樣每筆都會有值。
                description: String::new(),
                required: false,
                kind: FieldKind::Single {
                    options: types.iter().map(|t| Choice::new(t, t, "")).collect(),
                    default: Some("其它".into()),
                },
            },
            Field {
                id: "tag_opt".into(),
                label: "自定義標籤（Optional）".into(),
                description: String::new(),
                required: false,
                kind: FieldKind::Tags { suggestions: vec![] },
            },
        ],
    }
}

pub fn templates() -> Vec<Template> {
    let choice = |v: &str, l: &str, e: &str| Choice::new(v, l, e);
    vec![
        Template {
            id: "legacy-emotion",
            name: "情緒評分（舊版相容）",
            description: "和舊版 emoLabel 一樣的四個欄位：1–5 分、情緒、類型、自訂標籤。",
            schema: legacy_emotion_schema(),
            instructions: "依照文字表達的情緒評分：1 是很負面，5 是很正面，3 是中性。",
        },
        Template {
            id: "sentiment",
            name: "情緒分類",
            description: "正面／中性／負面，加上強度和一句說明。",
            schema: Schema {
                fields: vec![
                    Field {
                        id: "polarity".into(),
                        label: "情緒傾向".into(),
                        description: String::new(),
                        required: true,
                        kind: FieldKind::Single {
                            options: vec![
                                choice("positive", "正面", "🙂"),
                                choice("neutral", "中性", "😐"),
                                choice("negative", "負面", "🙁"),
                            ],
                            default: None,
                        },
                    },
                    Field {
                        id: "intensity".into(),
                        label: "強度".into(),
                        description: "0 是幾乎沒有情緒，1 是非常強烈".into(),
                        required: false,
                        kind: FieldKind::Scale {
                            min: 0.0,
                            max: 1.0,
                            step: 0.1,
                            default: Some(0.5),
                            min_label: "很弱".into(),
                            max_label: "很強".into(),
                        },
                    },
                    Field {
                        id: "note".into(),
                        label: "說明".into(),
                        description: String::new(),
                        required: false,
                        kind: FieldKind::Text {
                            max_length: DEFAULT_TEXT_CHARS,
                        },
                    },
                ],
            },
            instructions: "判斷這段內容整體的情緒傾向。反諷要依實際意思判斷。",
        },
        Template {
            id: "image-text",
            name: "圖文分類",
            description: "看圖片和文字：圖文是否相符、主題（可多選）、標籤。",
            schema: Schema {
                fields: vec![
                    Field {
                        id: "match".into(),
                        label: "圖文相符".into(),
                        description: String::new(),
                        required: true,
                        kind: FieldKind::Single {
                            options: vec![
                                choice("yes", "相符", "✅"),
                                choice("partly", "部分相符", "➖"),
                                choice("no", "不相符", "❌"),
                            ],
                            default: None,
                        },
                    },
                    Field {
                        id: "topics".into(),
                        label: "主題".into(),
                        description: String::new(),
                        required: false,
                        kind: FieldKind::Multi {
                            options: vec![
                                choice("food", "食物", "🍜"),
                                choice("travel", "旅遊", "🧳"),
                                choice("pets", "寵物", "🐾"),
                                choice("tech", "科技", "💻"),
                                choice("other", "其他", ""),
                            ],
                            max_selected: None,
                        },
                    },
                    Field {
                        id: "tags".into(),
                        label: "標籤".into(),
                        description: String::new(),
                        required: false,
                        kind: FieldKind::Tags { suggestions: vec![] },
                    },
                ],
            },
            instructions: "先看圖片，再讀文字。圖片和文字講的是同一件事才算相符。",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(v: Value) -> Values {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn templates_are_valid() {
        for t in templates() {
            t.schema.validate().unwrap_or_else(|e| panic!("{}: {e:?}", t.id));
        }
    }

    #[test]
    fn schema_json_round_trips_in_camel_case() {
        let s = legacy_emotion_schema();
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["fields"][0]["type"], "scale");
        assert_eq!(json["fields"][0]["minLabel"], "");
        let back: Schema = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn schema_validation_reports_every_problem() {
        let s: Schema = serde_json::from_value(json!({"fields": [
            {"id": "Score", "label": "", "type": "scale", "min": 5, "max": 1, "step": 1},
            {"id": "x", "label": "X", "type": "scale", "min": 0, "max": 1, "step": 0.3},
            {"id": "x", "label": "X2", "type": "single", "options": [{"value": "a"}], "default": "b"},
            {"id": "text", "label": "T", "type": "multi", "options": [{"value": "a"}, {"value": "a"}], "maxSelected": 3},
            {"id": "n", "label": "N", "type": "text", "maxLength": 0}
        ]}))
        .unwrap();
        let errors = s.validate().unwrap_err();
        let all = errors.join("\n");
        for needle in [
            "小寫英文字母",
            "要有顯示名稱",
            "最小值要小於最大值",
            "整除",
            "重複了",
            "至少要有 2 個選項",
            "不在選項裡",
            "保留的欄位名稱",
            "選項「a」重複",
            "最多可選幾個",
            "字數上限",
        ] {
            assert!(all.contains(needle), "missing「{needle}」in:\n{all}");
        }
    }

    #[test]
    fn values_are_validated_and_normalized() {
        let s = legacy_emotion_schema();
        let ok = s
            .validate_values(&values(
                json!({"score": 4, "tag": "sad", "type": "閒聊", "tag_opt": [" 政治 ", "政治", ""]}),
            ))
            .unwrap();
        assert_eq!(
            Value::Object(ok),
            json!({"score": 4.0, "tag": "sad", "type": "閒聊", "tag_opt": ["政治"]})
        );

        let err = s
            .validate_values(&values(json!({"score": 4.5, "tag": "meh", "extra": 1})))
            .unwrap_err()
            .join("\n");
        assert!(err.contains("每 1 一格"));
        assert!(err.contains("沒有「meh」這個選項"));
        assert!(err.contains("沒有「extra」這個欄位"));
        assert!(s
            .validate_values(&values(json!({"score": 1})))
            .unwrap_err()
            .join("")
            .contains("「情緒（Sentiment）」是必填"));
    }

    #[test]
    fn decimal_steps_accept_float_noise() {
        let t = templates().into_iter().find(|t| t.id == "sentiment").unwrap();
        let v = t
            .schema
            .validate_values(&values(
                json!({"polarity": "neutral", "intensity": 0.30000000000000004}),
            ))
            .unwrap();
        assert_eq!(v["intensity"], json!(0.3));
        assert!(t
            .schema
            .validate_values(&values(json!({"polarity": "neutral", "intensity": 0.35})))
            .is_err());
    }

    #[test]
    fn multi_select_keeps_option_order_and_limit() {
        let s: Schema = serde_json::from_value(json!({"fields": [
            {"id": "m", "label": "M", "type": "multi", "maxSelected": 2,
             "options": [{"value": "a"}, {"value": "b"}, {"value": "c"}]}
        ]}))
        .unwrap();
        let v = s.validate_values(&values(json!({"m": ["c", "a", "c"]}))).unwrap();
        assert_eq!(v["m"], json!(["a", "c"]));
        assert!(s.validate_values(&values(json!({"m": ["a", "b", "c"]}))).is_err());
        // 空陣列等於沒填
        assert!(s.validate_values(&values(json!({"m": []}))).unwrap().is_empty());
    }

    #[test]
    fn suggestions_are_coerced_leniently() {
        let s = legacy_emotion_schema();
        let (v, notes) = s.coerce_suggestion(&values(
            json!({"score": 7.2, "tag": "joy", "type": "天氣", "tag_opt": null}),
        ));
        assert_eq!(v["score"], json!(5.0));
        assert_eq!(v["type"], json!("天氣"));
        assert!(!v.contains_key("tag"));
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn json_schema_lists_every_field_as_required() {
        let s = legacy_emotion_schema();
        let js = s.suggestion_json_schema(true);
        assert_eq!(js["required"], json!(["values", "confidence", "rationale"]));
        assert_eq!(
            js["properties"]["values"]["required"],
            json!(["score", "tag", "type", "tag_opt"])
        );
        assert_eq!(js["properties"]["values"]["properties"]["tag"]["enum"][5], Value::Null);
        assert_eq!(js["properties"]["values"]["additionalProperties"], json!(false));
    }

    #[test]
    fn incompatible_schema_change_is_detected() {
        let old = legacy_emotion_schema();
        let labels = [old
            .validate_values(&values(json!({"score": 2, "tag": "angry", "type": "其它"})))
            .unwrap()];
        let mut new = old.clone();
        if let FieldKind::Single { options, .. } = &mut new.fields[1].kind {
            options.retain(|o| o.value != "angry");
        }
        assert!(new.first_incompatible(labels.iter()).unwrap().contains("angry"));
        assert!(old.first_incompatible(labels.iter()).is_none());
    }
}
