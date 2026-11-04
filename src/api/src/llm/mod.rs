//! AI 輔助標注的模型呼叫。兩種服務放在同一個介面後面：
//!
//! - Claude API（Anthropic 原生的 Messages API）：Rust 沒有官方 SDK，直接送 HTTP。
//! - OpenAI 相容的 Chat Completions API：OpenAI、Ollama、vLLM、LM Studio 都能接。
//!
//! 呼叫端只看到 [`Provider::complete`]：給它提示、圖片和 JSON Schema，拿回模型輸出的 JSON。

mod anthropic;
mod openai;
pub mod prompt;

use std::time::Duration;

use serde_json::Value;

use crate::{config::ProviderConfig, media::LlmImage};

/// 重試幾次（429、5xx、連線錯誤）。
const MAX_RETRIES: u32 = 2;
/// 服務沒給 retry-after 時，第一次重試前等多久；之後每次加倍。
const BASE_BACKOFF: Duration = Duration::from_secs(1);
/// retry-after 最多等多久。
const MAX_BACKOFF: Duration = Duration::from_secs(20);

pub struct LlmRequest {
    pub system: String,
    pub text: String,
    pub image: Option<LlmImage>,
    /// 模型輸出要符合的 JSON Schema。
    pub schema: Value,
}

#[derive(Debug)]
pub struct LlmReply {
    pub json: Value,
    /// 實際回答的模型（Claude 的備用模型接手時會和設定的不同）。
    pub model: String,
}

#[derive(Debug)]
pub enum LlmError {
    /// 服務回了錯誤，例如 key 錯了（401）、模型名稱不對（404）。
    Http { status: u16, message: String },
    /// 連不上、逾時。
    Network(String),
    /// 模型拒絕回答。
    Refusal(String),
    /// 輸出被 max_tokens 截斷。
    Truncated,
    /// 輸出不是 JSON。
    InvalidJson(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http {
                status: 401 | 403,
                message,
            } => write!(f, "API key 無效或沒有權限（{message}）"),
            Self::Http { status: 404, message } => {
                write!(f, "找不到模型或網址，請檢查模型名稱和 base URL（{message}）")
            }
            Self::Http { status: 429, message } => write!(f, "請求太頻繁或額度用完（{message}）"),
            Self::Http { status, message } => write!(f, "AI 服務回傳錯誤 {status}：{message}"),
            Self::Network(m) => write!(f, "連不上 AI 服務：{m}"),
            Self::Refusal(m) => write!(f, "模型拒絕回答：{m}"),
            // 推理型模型的思考也算在上下文裡，Ollama 預設只有 4096（見 docs/pitfalls.md）
            Self::Truncated => write!(
                f,
                "模型的回答太長被截斷。推理型模型的思考也會用掉上下文：\
                 換成不思考的模型，或調大上下文長度（Ollama 是 OLLAMA_CONTEXT_LENGTH）"
            ),
            Self::InvalidJson(m) => write!(f, "模型的回答不是有效的 JSON：{m}"),
        }
    }
}

impl std::error::Error for LlmError {}

pub enum Provider {
    Anthropic(anthropic::Client),
    OpenAi(openai::Client),
}

impl Provider {
    pub fn new(config: &ProviderConfig, timeout: Duration) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(concat!("simply-nlp-labeling/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(match config {
            ProviderConfig::Anthropic {
                base_url,
                api_key,
                model,
                effort,
                server_fallbacks,
            } => Self::Anthropic(anthropic::Client {
                http,
                base_url: base_url.clone(),
                api_key: api_key.clone(),
                model: model.clone(),
                effort: effort.clone(),
                server_fallbacks: *server_fallbacks,
            }),
            ProviderConfig::OpenAi {
                base_url,
                api_key,
                model,
            } => Self::OpenAi(openai::Client {
                http,
                base_url: base_url.clone(),
                api_key: api_key.clone(),
                model: model.clone(),
            }),
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Anthropic(_) => "anthropic",
            Self::OpenAi(_) => "openai",
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Self::Anthropic(c) => &c.model,
            Self::OpenAi(c) => &c.model,
        }
    }

    pub async fn complete(&self, req: &LlmRequest) -> Result<LlmReply, LlmError> {
        match self {
            Self::Anthropic(c) => c.complete(req).await,
            Self::OpenAi(c) => c.complete(req).await,
        }
    }
}

/// 送出請求；遇到 429、5xx（含 Anthropic 的 529 overloaded）或連線錯誤時重試。
async fn send_json(build: impl Fn() -> reqwest::RequestBuilder) -> Result<Value, LlmError> {
    let mut attempt = 0;
    loop {
        let retry_after = match build().send().await {
            Ok(resp) => {
                let status = resp.status();
                let wait = resp
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs);
                let body = resp.text().await.map_err(|e| LlmError::Network(e.to_string()))?;
                if status.is_success() {
                    return serde_json::from_str(&body)
                        .map_err(|e| LlmError::InvalidJson(format!("服務的回應不是 JSON：{e}")));
                }
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if !retryable || attempt >= MAX_RETRIES {
                    return Err(LlmError::Http {
                        status: status.as_u16(),
                        message: error_message(&body),
                    });
                }
                wait
            }
            Err(e) => {
                if attempt >= MAX_RETRIES {
                    return Err(LlmError::Network(e.to_string()));
                }
                None
            }
        };
        let backoff = retry_after.unwrap_or(BASE_BACKOFF * 2u32.pow(attempt)).min(MAX_BACKOFF);
        tracing::warn!(
            "AI 服務暫時失敗，{} 秒後重試（第 {} 次）",
            backoff.as_secs_f32(),
            attempt + 1
        );
        tokio::time::sleep(backoff).await;
        attempt += 1;
    }
}

/// 兩種服務的錯誤格式都是 `{"error": {"message": ...}}`；不是的話取前 300 個字。
fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect())
}

/// 從模型的文字輸出取出 JSON 物件。小模型常把 JSON 包在 ```json 區塊裡，或前後多寫幾句話，
/// 也有推理模型會先輸出 <think>…</think>；這裡一律取第一個 `{` 到最後一個 `}`。
pub fn extract_json(text: &str) -> Result<Value, LlmError> {
    let text = match text.rfind("</think>") {
        Some(end) => &text[end + "</think>".len()..],
        None => text,
    };
    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(text.trim()) {
        return Ok(v);
    }
    let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) else {
        return Err(LlmError::InvalidJson(snippet(text)));
    };
    if end < start {
        return Err(LlmError::InvalidJson(snippet(text)));
    }
    serde_json::from_str(&text[start..=end]).map_err(|e| LlmError::InvalidJson(format!("{e}：{}", snippet(text))))
}

fn snippet(text: &str) -> String {
    let s: String = text.chars().take(200).collect();
    if s.is_empty() {
        "（空白）".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_is_extracted_from_chatty_output() {
        assert_eq!(extract_json(r#"{"a": 1}"#).unwrap(), json!({"a": 1}));
        assert_eq!(
            extract_json("Sure!\n```json\n{\"a\": {\"b\": 2}}\n```\nDone.").unwrap(),
            json!({"a": {"b": 2}})
        );
        assert_eq!(
            extract_json("<think>maybe {x}</think>\n{\"a\": 3}").unwrap(),
            json!({"a": 3})
        );
        assert!(matches!(extract_json("no json here"), Err(LlmError::InvalidJson(_))));
        assert!(matches!(extract_json("} {"), Err(LlmError::InvalidJson(_))));
    }

    #[test]
    fn error_messages_are_readable() {
        assert_eq!(
            error_message(r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#),
            "invalid x-api-key"
        );
        assert_eq!(error_message("plain text"), "plain text");
        let e = LlmError::Http {
            status: 401,
            message: "bad".into(),
        };
        assert!(e.to_string().contains("API key"));
    }
}
