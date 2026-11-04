//! 設定都來自環境變數，全部有預設值；README 的「設定」一節列出每一個。

use std::{env, path::PathBuf, time::Duration};

use anyhow::{bail, Context, Result};

/// 舊版 Flask 服務用的就是 5000，沿用。
pub const DEFAULT_BIND: &str = "0.0.0.0:5000";
pub const DEFAULT_MAX_UPLOAD_MB: u64 = 200;

pub const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
/// 沒有指定 `ANTHROPIC_MODEL` 時用的 Claude 模型。
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";
/// 分類這種短任務用 low 就夠，見 README「AI 輔助標注」。
pub const DEFAULT_ANTHROPIC_EFFORT: &str = "low";
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub static_dir: PathBuf,
    pub max_upload_bytes: usize,
    pub llm: LlmSettings,
}

/// AI 輔助標注的設定。`provider` 是 None 時 AI 功能關閉，`disabled_reason` 說明原因。
#[derive(Clone, Debug)]
pub struct LlmSettings {
    pub provider: Option<ProviderConfig>,
    pub disabled_reason: Option<String>,
    pub timeout: Duration,
    /// 批次預標時同時送出幾個請求。
    pub concurrency: usize,
}

#[derive(Clone)]
pub enum ProviderConfig {
    Anthropic {
        base_url: String,
        api_key: String,
        model: String,
        /// None：不送 effort（給不支援 effort 的舊模型用，設定 ANTHROPIC_EFFORT=off）。
        effort: Option<String>,
        /// 模型拒答時讓 API 改用 Anthropic 建議的備用模型重跑（`fallbacks: "default"`）。
        server_fallbacks: bool,
    },
    OpenAi {
        base_url: String,
        api_key: Option<String>,
        model: String,
    },
}

/// 不把 API key 印進 log。
impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic {
                base_url,
                model,
                effort,
                server_fallbacks,
                ..
            } => f
                .debug_struct("Anthropic")
                .field("base_url", base_url)
                .field("model", model)
                .field("effort", effort)
                .field("server_fallbacks", server_fallbacks)
                .finish_non_exhaustive(),
            Self::OpenAi {
                base_url,
                api_key,
                model,
            } => f
                .debug_struct("OpenAi")
                .field("base_url", base_url)
                .field("model", model)
                .field("api_key_set", &api_key.is_some())
                .finish(),
        }
    }
}

impl ProviderConfig {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Anthropic { .. } => "anthropic",
            Self::OpenAi { .. } => "openai",
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Self::Anthropic { model, .. } | Self::OpenAi { model, .. } => model,
        }
    }

    pub fn base_url(&self) -> &str {
        match self {
            Self::Anthropic { base_url, .. } | Self::OpenAi { base_url, .. } => base_url,
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    /// 測試用：從任意的鍵值來源讀設定。
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let var = |key: &str| get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let data_dir = PathBuf::from(var("LABEL_DATA_DIR").unwrap_or_else(|| "data".into()));
        let database_path = var("LABEL_DATABASE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("labels.db"));
        let max_upload_mb: u64 = parse(&var, "LABEL_MAX_UPLOAD_MB", DEFAULT_MAX_UPLOAD_MB)?;
        Ok(Self {
            bind: var("LABEL_BIND").unwrap_or_else(|| DEFAULT_BIND.into()),
            data_dir,
            database_path,
            static_dir: PathBuf::from(var("LABEL_STATIC_DIR").unwrap_or_else(|| "src/web/dist".into())),
            max_upload_bytes: usize::try_from(max_upload_mb * 1024 * 1024).context("LABEL_MAX_UPLOAD_MB 太大")?,
            llm: LlmSettings::from_lookup(&var)?,
        })
    }

    pub fn media_dir(&self) -> PathBuf {
        self.data_dir.join("media")
    }
}

impl LlmSettings {
    fn from_lookup(var: &impl Fn(&str) -> Option<String>) -> Result<Self> {
        let timeout = Duration::from_secs(parse(var, "LLM_TIMEOUT_SECS", 120)?);
        let concurrency: usize = parse(var, "LLM_CONCURRENCY", 2)?;
        if concurrency == 0 {
            bail!("LLM_CONCURRENCY 至少要是 1");
        }
        let (provider, disabled_reason) = match var("LLM_PROVIDER").map(|p| p.to_lowercase()) {
            None => (None, Some("沒有設定 LLM_PROVIDER".to_string())),
            Some(p) if p == "anthropic" => match var("ANTHROPIC_API_KEY") {
                Some(api_key) => (
                    Some(ProviderConfig::Anthropic {
                        base_url: trim_slash(
                            var("ANTHROPIC_BASE_URL").unwrap_or_else(|| DEFAULT_ANTHROPIC_BASE_URL.into()),
                        ),
                        api_key,
                        model: var("ANTHROPIC_MODEL").unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.into()),
                        effort: match var("ANTHROPIC_EFFORT") {
                            Some(e) if e == "off" => None,
                            Some(e) => Some(e),
                            None => Some(DEFAULT_ANTHROPIC_EFFORT.into()),
                        },
                        server_fallbacks: var("ANTHROPIC_FALLBACKS").map(|v| v != "off").unwrap_or(true),
                    }),
                    None,
                ),
                None => (
                    None,
                    Some("LLM_PROVIDER=anthropic，但沒有設定 ANTHROPIC_API_KEY".into()),
                ),
            },
            Some(p) if p == "openai" => match var("OPENAI_MODEL") {
                Some(model) => (
                    Some(ProviderConfig::OpenAi {
                        base_url: trim_slash(var("OPENAI_BASE_URL").unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.into())),
                        api_key: var("OPENAI_API_KEY"),
                        model,
                    }),
                    None,
                ),
                None => (None, Some("LLM_PROVIDER=openai，但沒有設定 OPENAI_MODEL".into())),
            },
            Some(other) => bail!("LLM_PROVIDER 只能是 anthropic 或 openai，收到：{other}"),
        };
        Ok(Self {
            provider,
            disabled_reason,
            timeout,
            concurrency,
        })
    }
}

fn trim_slash(url: String) -> String {
    url.trim_end_matches('/').to_string()
}

fn parse<T: std::str::FromStr>(var: &impl Fn(&str) -> Option<String>, key: &str, default: T) -> Result<T> {
    match var(key) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| anyhow::anyhow!("{key} 不是有效的數字：{v}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Config> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn ai_is_off_without_provider() {
        let c = config(&[]).unwrap();
        assert!(c.llm.provider.is_none());
        assert_eq!(c.bind, DEFAULT_BIND);
        assert!(c.llm.disabled_reason.unwrap().contains("LLM_PROVIDER"));
    }

    #[test]
    fn anthropic_defaults() {
        let c = config(&[("LLM_PROVIDER", "anthropic"), ("ANTHROPIC_API_KEY", "k")]).unwrap();
        let p = c.llm.provider.unwrap();
        assert_eq!(p.name(), "anthropic");
        assert_eq!(p.model(), DEFAULT_ANTHROPIC_MODEL);
        assert_eq!(p.base_url(), DEFAULT_ANTHROPIC_BASE_URL);
        assert!(
            !format!("{p:?}").contains("\"k\""),
            "key must not appear in Debug output"
        );
    }

    #[test]
    fn provider_without_key_or_model_is_disabled_with_reason() {
        let c = config(&[("LLM_PROVIDER", "anthropic")]).unwrap();
        assert!(c.llm.provider.is_none());
        assert!(c.llm.disabled_reason.unwrap().contains("ANTHROPIC_API_KEY"));
        let c = config(&[("LLM_PROVIDER", "openai")]).unwrap();
        assert!(c.llm.disabled_reason.unwrap().contains("OPENAI_MODEL"));
    }

    #[test]
    fn openai_compatible_needs_no_key() {
        let c = config(&[
            ("LLM_PROVIDER", "OpenAI"),
            ("OPENAI_BASE_URL", "http://ollama:11434/v1/"),
            ("OPENAI_MODEL", "qwen3-vl:2b"),
        ])
        .unwrap();
        let p = c.llm.provider.unwrap();
        assert_eq!(p.base_url(), "http://ollama:11434/v1");
        assert!(matches!(p, ProviderConfig::OpenAi { api_key: None, .. }));
    }

    #[test]
    fn unknown_provider_is_an_error() {
        assert!(config(&[("LLM_PROVIDER", "gemini")]).is_err());
    }
}
