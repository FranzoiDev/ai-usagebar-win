//! Config at `%APPDATA%\ai-usagebar\config.toml` (resolved via `directories`).
//!
//! Mirrors the Linux crate's config layout so a user's existing file is
//! compatible. Missing file = defaults. API keys resolve env-var-first, then
//! inline config. OAuth-credential paths default to the Windows user profile.

use std::path::PathBuf;

use serde::Deserialize;

use crate::vendors::VendorId;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ui: UiConfig,
    pub anthropic: AnthropicConfig,
    pub openai: OpenAiConfig,
    pub zai: ZaiConfig,
    pub openrouter: OpenRouterConfig,
    pub deepseek: DeepseekConfig,
    /// Seconds between refreshes. Default 60.
    pub poll_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Which vendor leads the tray tooltip. Defaults to anthropic.
    pub primary: Option<VendorId>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AnthropicConfig {
    pub enabled: bool,
    pub credentials_path: Option<PathBuf>,
}
impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            credentials_path: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OpenAiConfig {
    pub enabled: bool,
    pub codex_auth_path: Option<PathBuf>,
}
impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            codex_auth_path: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ZaiConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
    pub plan_tier: Option<String>,
}
impl Default for ZaiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key_env: "ZAI_API_KEY".to_string(),
            api_key: None,
            plan_tier: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OpenRouterConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
}
impl Default for OpenRouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key_env: "OPENROUTER_API_KEY".to_string(),
            api_key: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DeepseekConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
}
impl Default for DeepseekConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key_env: "DEEPSEEK_API_KEY".to_string(),
            api_key: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let Some(path) = default_path() else {
            return Self::default();
        };
        Self::load_from(&path)
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn is_enabled(&self, id: VendorId) -> bool {
        match id {
            VendorId::Anthropic => self.anthropic.enabled,
            VendorId::Openai => self.openai.enabled,
            VendorId::Zai => self.zai.enabled,
            VendorId::Openrouter => self.openrouter.enabled,
            VendorId::Deepseek => self.deepseek.enabled,
        }
    }

    pub fn enabled_vendors(&self) -> Vec<VendorId> {
        VendorId::ALL
            .iter()
            .copied()
            .filter(|id| self.is_enabled(*id))
            .collect()
    }

    pub fn poll_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.poll_seconds.unwrap_or(60).max(15))
    }
}

/// Resolve an API key: env var wins, then inline config, else `None`.
pub fn resolve_api_key(env_var_name: &str, inline: Option<&str>) -> Option<String> {
    if !env_var_name.is_empty()
        && let Ok(v) = std::env::var(env_var_name)
        && !v.is_empty()
    {
        return Some(v);
    }
    inline.filter(|s| !s.is_empty()).map(|s| s.to_string())
}

pub fn default_path() -> Option<PathBuf> {
    let proj = directories::ProjectDirs::from("", "", "ai-usagebar")?;
    Some(proj.config_dir().join("config.toml"))
}

/// `%USERPROFILE%` (or the platform home), resolved via `directories`.
pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// Default Anthropic credentials path: `%USERPROFILE%\.claude\.credentials.json`.
pub fn anthropic_creds_path(cfg: &AnthropicConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.credentials_path {
        return Some(p.clone());
    }
    Some(home_dir()?.join(".claude").join(".credentials.json"))
}

/// Default OpenAI Codex auth path: `%USERPROFILE%\.codex\auth.json`.
pub fn openai_auth_path(cfg: &OpenAiConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.codex_auth_path {
        return Some(p.clone());
    }
    Some(home_dir()?.join(".codex").join("auth.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_enable_four_vendors() {
        let c = Config::default();
        assert!(c.is_enabled(VendorId::Anthropic));
        assert!(c.is_enabled(VendorId::Openai));
        assert!(c.is_enabled(VendorId::Zai));
        assert!(c.is_enabled(VendorId::Openrouter));
        assert!(!c.is_enabled(VendorId::Deepseek));
        assert_eq!(c.enabled_vendors().len(), 4);
    }

    #[test]
    fn parses_partial_config() {
        let c: Config = toml::from_str(
            r#"
            poll_seconds = 30
            [openai]
            enabled = false
            [zai]
            api_key = "sk-zai-inline"
            "#,
        )
        .unwrap();
        assert!(!c.is_enabled(VendorId::Openai));
        assert!(c.is_enabled(VendorId::Anthropic));
        assert_eq!(c.zai.api_key.as_deref(), Some("sk-zai-inline"));
        assert_eq!(c.poll_interval().as_secs(), 30);
    }

    #[test]
    fn poll_interval_floor_is_15() {
        let c: Config = toml::from_str("poll_seconds = 1").unwrap();
        assert_eq!(c.poll_interval().as_secs(), 15);
    }

    #[test]
    fn resolve_api_key_prefers_inline_when_env_absent() {
        assert_eq!(
            resolve_api_key("DEFINITELY_UNSET_ENV_XYZ", Some("inline")).as_deref(),
            Some("inline")
        );
        assert_eq!(resolve_api_key("DEFINITELY_UNSET_ENV_XYZ", None), None);
    }
}
