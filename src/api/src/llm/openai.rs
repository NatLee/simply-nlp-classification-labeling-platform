//! OpenAI 相容的 Chat Completions API：`POST {base}/chat/completions`。
//! OpenAI、Ollama（`http://host:11434/v1`）、vLLM、LM Studio 都用這個格式。
//!
//! - 結構化輸出：`response_format` 的 `json_schema`（strict）。不支援的服務也會照提示輸出 JSON，
//!   解析時容許前後多出文字。
//! - 圖片：`image_url` 放 data URL。
//! - 不送 temperature 等取樣參數，用各服務的預設值。

use serde_json::{json, Value};

use super::{send_json, LlmError, LlmReply, LlmRequest};

pub struct Client {
    pub http: reqwest::Client,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

impl Client {
    pub fn body(&self, req: &LlmRequest) -> Value {
        let user: Value = match &req.image {
            Some(img) => json!([
                { "type": "image_url", "image_url": { "url": img.data_url() } },
                { "type": "text", "text": req.text },
            ]),
            None => Value::from(req.text.clone()),
        };
        json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user", "content": user },
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": "annotation", "strict": true, "schema": req.schema },
            },
        })
    }

    pub async fn complete(&self, req: &LlmRequest) -> Result<LlmReply, LlmError> {
        let body = self.body(req);
        let url = format!("{}/chat/completions", self.base_url);
        let resp = send_json(|| {
            let b = self.http.post(&url).json(&body);
            match &self.api_key {
                Some(key) => b.bearer_auth(key),
                None => b,
            }
        })
        .await?;
        parse(&resp, &self.model)
    }
}

pub fn parse(resp: &Value, configured_model: &str) -> Result<LlmReply, LlmError> {
    let choice = &resp["choices"][0];
    if let Some(refusal) = choice.pointer("/message/refusal").and_then(Value::as_str) {
        return Err(LlmError::Refusal(refusal.to_string()));
    }
    if choice["finish_reason"] == "length" {
        return Err(LlmError::Truncated);
    }
    let text = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(LlmReply {
        json: super::extract_json(text)?,
        model: resp["model"].as_str().unwrap_or(configured_model).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::LlmImage;

    #[test]
    fn body_uses_data_url_and_strict_schema() {
        let c = Client {
            http: reqwest::Client::new(),
            base_url: "http://x/v1".into(),
            api_key: None,
            model: "m".into(),
        };
        let req = LlmRequest {
            system: "sys".into(),
            text: "t".into(),
            image: Some(LlmImage {
                mime: "image/jpeg".into(),
                base64: "QUJD".into(),
            }),
            schema: json!({"type": "object"}),
        };
        let b = c.body(&req);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(
            b["messages"][1]["content"][0]["image_url"]["url"],
            "data:image/jpeg;base64,QUJD"
        );
        assert_eq!(b["response_format"]["json_schema"]["strict"], true);
        assert!(b.get("temperature").is_none());
        let text_only = c.body(&LlmRequest { image: None, ..req });
        assert_eq!(text_only["messages"][1]["content"], "t");
    }

    #[test]
    fn parses_content_refusal_and_length() {
        let ok = json!({"model": "qwen", "choices": [{"finish_reason": "stop", "message": {"content": "```json\n{\"values\": {}}\n```"}}]});
        assert_eq!(parse(&ok, "m").unwrap().model, "qwen");
        let refused = json!({"choices": [{"message": {"content": null, "refusal": "no"}}]});
        assert!(matches!(parse(&refused, "m"), Err(LlmError::Refusal(_))));
        let cut = json!({"choices": [{"finish_reason": "length", "message": {"content": "{"}}]});
        assert!(matches!(parse(&cut, "m"), Err(LlmError::Truncated)));
    }
}
