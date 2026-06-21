//! Vendor dispatch. Each submodule reverse-engineers one provider's
//! usage endpoint from the Linux `ai-usagebar` crate, using **blocking**
//! reqwest (the tray app polls from a background thread — no async runtime).

pub mod anthropic;
pub mod deepseek;
pub mod openai;
pub mod openrouter;
pub mod zai;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::config::Config;
use crate::usage::VendorSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VendorId {
    Anthropic,
    Openai,
    Zai,
    Openrouter,
    Deepseek,
}

impl VendorId {
    pub const ALL: [VendorId; 5] = [
        VendorId::Anthropic,
        VendorId::Openai,
        VendorId::Zai,
        VendorId::Openrouter,
        VendorId::Deepseek,
    ];

    /// Short tag for the compact tooltip line, e.g. "cld".
    pub fn short(self) -> &'static str {
        match self {
            VendorId::Anthropic => "cld",
            VendorId::Openai => "gpt",
            VendorId::Zai => "zai",
            VendorId::Openrouter => "or",
            VendorId::Deepseek => "ds",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            VendorId::Anthropic => "Anthropic",
            VendorId::Openai => "OpenAI",
            VendorId::Zai => "Z.AI",
            VendorId::Openrouter => "OpenRouter",
            VendorId::Deepseek => "DeepSeek",
        }
    }
}

/// Result of polling one vendor.
#[derive(Debug, Clone)]
pub enum VendorState {
    Ok(VendorSnapshot),
    /// Credentials missing or expired — message tells the user what CLI to run.
    NeedsLogin(String),
    /// Network / HTTP / parse failure.
    Error(String),
}

#[derive(Debug, Clone)]
pub struct VendorReport {
    pub id: VendorId,
    pub state: VendorState,
}

/// HTTP timeout shared by every vendor request.
pub const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub fn build_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent("ai-usagebar-win")
        .build()
        .unwrap_or_default()
}

/// Poll every enabled vendor sequentially. `now` is injected for testability.
pub fn fetch_all(
    client: &reqwest::blocking::Client,
    cfg: &Config,
    now: DateTime<Utc>,
) -> Vec<VendorReport> {
    cfg.enabled_vendors()
        .into_iter()
        .map(|id| VendorReport {
            id,
            state: fetch_one(client, cfg, id, now),
        })
        .collect()
}

fn fetch_one(
    client: &reqwest::blocking::Client,
    cfg: &Config,
    id: VendorId,
    now: DateTime<Utc>,
) -> VendorState {
    match id {
        VendorId::Anthropic => anthropic::fetch(client, cfg, now),
        VendorId::Openai => openai::fetch(client, cfg, now),
        VendorId::Zai => zai::fetch(client, cfg),
        VendorId::Openrouter => openrouter::fetch(client, cfg),
        VendorId::Deepseek => deepseek::fetch(client, cfg),
    }
}
