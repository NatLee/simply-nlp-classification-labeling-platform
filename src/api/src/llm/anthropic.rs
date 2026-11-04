//! Claude API（Messages API）。Rust 沒有官方 SDK，直接送 HTTP：
//! `POST {base}/v1/messages`，標頭 `x-api-key`、`anthropic-version`。
//!
//! - 結構化輸出：`output_config.format` 給 JSON Schema，模型的文字輸出一定是符合的 JSON。
//! - 圖片：base64 的 `image` 區塊，放在文字前面。
//! - 思考：Claude Opus 5.5 一律開著自適應思考，不送 `thinking`；用 `output_config.effort` 控制深度。
//! - 拒答：`stop_reason = "refusal"`。預設加上 `fallbacks: "default"`，讓 API 自動改用建議的備用模型重跑。
//! - API 版本 2026-10-21 不接受 `temperature`、`top_p`、`top_k`，所以都不送。

use serde_json::{json, Value};

use super::{send_json, LlmError, LlmReply, LlmRequest};

pub const API_VERSION: &str = "2026-10-21";
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// 不用串流時建議的上限（思考也算在裡面）。
pub const MAX_TOKENS: u32 = 16_000;

pub struct Client {
    pub http: reqwest::Client,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub effort: Option<String>,
    pub server_fallbacks: bool,
}

impl Client {
    pub fn body(&self, req: &LlmRequest) -> Value {
        let mut content = Vec::new();
        if let Some(img) = &req.image {
            content.push(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": img.mime, "data": img.base64 },
            }));
        }
        content.push(json!({ "type": "text", "text": req.text }));
        let mut output_config = json!({ "format": { "type": "json_schema", "schema": req.schema } });
        if let Some(effort) = &self.effort {
            output_config["effort"] = Value::from(effort.clone());
        }
        let mut body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": req.system,
            "messages": [{ "role": "user", "content": content }],
            "output_config": output_config,
        });
        if self.server_fallbacks {
            body["fallbacks"] = Value::from("default");
        }
        body
    }

    pub async fn complete(&self, req: &LlmRequest) -> Result<LlmReply, LlmError> {
        let body = self.body(req);
        let url = format!("{}/v1/messages", self.base_url);
        let resp = send_json(|| {
            let mut b = self
                .http
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION)
                .json(&body);
            if self.server_fallbacks {
                b = b.header("anthropic-beta", FALLBACK_BETA);
            }
            b
        })
        .await?;
        parse(&resp, &self.model)
    }
}

pub fn parse(resp: &Value, configured_model: &str) -> Result<LlmReply, LlmError> {
    match resp["stop_reason"].as_str() {
        Some("refusal") => {
            let why = resp
                .pointer("/stop_details/explanation")
                .and_then(Value::as_str)
                .or_else(|| resp.pointer("/stop_details/category").and_then(Value::as_str))
                .unwrap_or("沒有提供原因");
            return Err(LlmError::Refusal(why.to_string()));
        }
        Some("max_tokens") => return Err(LlmError::Truncated),
        _ => {}
    }
    // 思考區塊（thinking）和備用模型的標記（fallback）都略過，只取文字
    let text: String = resp["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    Ok(LlmReply {
        json: super::extract_json(&text)?,
        model: resp["model"].as_str().unwrap_or(configured_model).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::LlmImage;

    fn client(effort: Option<&str>, fallbacks: bool) -> Client {
        Client {
            http: reqwest::Client::new(),
            base_url: "http://x".into(),
            api_key: "k".into(),
            model: "claude-opus-5-5".into(),
            effort: effort.map(str::to_string),
            server_fallbacks: fallbacks,
        }
    }

    fn request(image: bool) -> LlmRequest {
        LlmRequest {
            system: "sys".into(),
            text: "hello".into(),
            image: image.then(|| LlmImage {
                mime: "image/png".into(),
                base64: "AAAA".into(),
            }),
            schema: json!({"type": "object"}),
        }
    }

    #[test]
    fn body_has_image_first_structured_output_and_no_sampling_params() {
        let b = client(Some("low"), true).body(&request(true));
        assert_eq!(b["messages"][0]["content"][0]["type"], "image");
        assert_eq!(b["messages"][0]["content"][0]["source"]["media_type"], "image/png");
        assert_eq!(b["messages"][0]["content"][1]["text"], "hello");
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["output_config"]["effort"], "low");
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["max_tokens"], MAX_TOKENS);
        for removed in ["temperature", "top_p", "top_k", "thinking"] {
            assert!(b.get(removed).is_none(), "{removed} must not be sent");
        }
        let b = client(None, false).body(&request(false));
        assert!(b["output_config"].get("effort").is_none());
        assert!(b.get("fallbacks").is_none());
        assert_eq!(b["messages"][0]["content"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn parses_text_after_thinking_and_detects_refusals() {
        let ok = json!({
            "model": "claude-opus-4-8",
            "stop_reason": "end_turn",
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "fallback", "from": {"model": "claude-opus-5-5"}, "to": {"model": "claude-opus-4-8"}},
                {"type": "text", "text": "{\"values\": {}, \"confidence\": 0.5, \"rationale\": \"r\"}"}
            ]
        });
        let r = parse(&ok, "claude-opus-5-5").unwrap();
        assert_eq!(r.model, "claude-opus-4-8");
        assert_eq!(r.json["confidence"], 0.5);
        let refusal =
            json!({"stop_reason": "refusal", "stop_details": {"type": "refusal", "category": "cyber"}, "content": []});
        assert!(matches!(parse(&refusal, "m"), Err(LlmError::Refusal(c)) if c == "cyber"));
        assert!(matches!(
            parse(&json!({"stop_reason": "max_tokens", "content": []}), "m"),
            Err(LlmError::Truncated)
        ));
    }
}
