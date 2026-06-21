//! OpenRouter — combines `/api/v1/credits` and `/api/v1/key`.

use serde::Deserialize;

use crate::config::{self, Config};
use crate::usage::{OpenRouterSnapshot, VendorSnapshot};

use super::VendorState;

const CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";

pub fn fetch(client: &reqwest::blocking::Client, cfg: &Config) -> VendorState {
    let Some(key) = config::resolve_api_key(
        &cfg.openrouter.api_key_env,
        cfg.openrouter.api_key.as_deref(),
    ) else {
        return VendorState::NeedsLogin(format!(
            "no API key — set {} or [openrouter] api_key",
            cfg.openrouter.api_key_env
        ));
    };

    let credits: CreditsData = match get_data(client, CREDITS_URL, &key) {
        Ok(d) => d,
        Err(s) => return s,
    };
    let keyd: KeyData = match get_data(client, KEY_URL, &key) {
        Ok(d) => d,
        Err(s) => return s,
    };
    VendorState::Ok(VendorSnapshot::Openrouter(combine(credits, keyd)))
}

fn get_data<T: for<'de> Deserialize<'de>>(
    client: &reqwest::blocking::Client,
    url: &str,
    key: &str,
) -> Result<T, VendorState> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {key}"))
        .send()
        .map_err(|e| VendorState::Error(format!("network: {e}")))?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(VendorState::NeedsLogin("API key rejected (401)".into()));
    }
    if !status.is_success() {
        return Err(VendorState::Error(format!("HTTP {}", status.as_u16())));
    }
    let env: OrEnvelope<T> = resp
        .json()
        .map_err(|e| VendorState::Error(format!("bad response: {e}")))?;
    Ok(env.data)
}

#[derive(Deserialize)]
struct OrEnvelope<T> {
    data: T,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CreditsData {
    total_credits: f64,
    total_usage: f64,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct KeyData {
    label: String,
    limit: Option<f64>,
    limit_remaining: Option<f64>,
    usage_daily: f64,
    usage_weekly: f64,
    usage_monthly: f64,
    is_free_tier: bool,
}

fn combine(credits: CreditsData, key: KeyData) -> OpenRouterSnapshot {
    let label = if key.label.is_empty() {
        "OpenRouter".to_string()
    } else {
        format!("OpenRouter — {}", key.label)
    };
    OpenRouterSnapshot {
        label,
        total_credits: credits.total_credits,
        total_usage: credits.total_usage,
        usage_daily: key.usage_daily,
        usage_weekly: key.usage_weekly,
        usage_monthly: key.usage_monthly,
        is_free_tier: key.is_free_tier,
        limit: key.limit,
        limit_remaining: key.limit_remaining,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_builds_snapshot() {
        let c: OrEnvelope<CreditsData> =
            serde_json::from_str(r#"{"data":{"total_credits":100.0,"total_usage":30.0}}"#).unwrap();
        let k: OrEnvelope<KeyData> = serde_json::from_str(
            r#"{"data":{"label":"prod","usage_daily":1.0,"usage_weekly":5.0,
                "usage_monthly":30.0,"is_free_tier":false}}"#,
        )
        .unwrap();
        let s = combine(c.data, k.data);
        assert_eq!(s.label, "OpenRouter — prod");
        assert!((s.balance() - 70.0).abs() < 1e-9);
        assert_eq!(s.consumed_pct(), 30);
    }
}
