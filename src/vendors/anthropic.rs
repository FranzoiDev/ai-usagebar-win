//! Anthropic Claude — `GET /api/oauth/usage` with the CLI's access token.
//! Read-only: we never refresh the token (see `creds.rs`).

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::config::{self, Config};
use crate::creds::{self, CredState};
use crate::usage::{AnthropicSnapshot, ExtraUsage, UsageWindow, VendorSnapshot};

use super::VendorState;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA_HEADER: &str = "oauth-2025-04-20";

pub fn fetch(client: &reqwest::blocking::Client, cfg: &Config, now: DateTime<Utc>) -> VendorState {
    let Some(path) = config::anthropic_creds_path(&cfg.anthropic) else {
        return VendorState::Error("could not resolve home directory".into());
    };
    let creds = match creds::read_anthropic(&path, now.timestamp()) {
        CredState::Valid(c) => c,
        CredState::Expired => {
            return VendorState::NeedsLogin("token expired — run `claude` to re-login".into());
        }
        CredState::Missing => {
            return VendorState::NeedsLogin("not logged in — run `claude`".into());
        }
        CredState::Malformed(e) => return VendorState::Error(format!("bad credentials file: {e}")),
    };

    let resp = match client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("anthropic-beta", BETA_HEADER)
        .send()
    {
        Ok(r) => r,
        Err(e) => return VendorState::Error(format!("network: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return VendorState::NeedsLogin("session invalid — run `claude` to re-login".into());
    }
    if !status.is_success() {
        return VendorState::Error(format!("HTTP {}", status.as_u16()));
    }
    let body: UsageResponse = match resp.json() {
        Ok(b) => b,
        Err(e) => return VendorState::Error(format!("bad response: {e}")),
    };
    VendorState::Ok(VendorSnapshot::Anthropic(body.into_snapshot(creds.plan_label)))
}

#[derive(Debug, Default, Deserialize)]
struct UsageResponse {
    #[serde(default)]
    five_hour: Option<Window>,
    #[serde(default)]
    seven_day: Option<Window>,
    #[serde(default)]
    seven_day_sonnet: Option<Window>,
    #[serde(default)]
    extra_usage: Option<ExtraUsageBlock>,
}

#[derive(Debug, Default, Deserialize)]
struct Window {
    #[serde(default)]
    utilization: f64,
    #[serde(default)]
    resets_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ExtraUsageBlock {
    #[serde(default)]
    is_enabled: bool,
    #[serde(default, deserialize_with = "de_int")]
    monthly_limit: i64,
    #[serde(default, deserialize_with = "de_int")]
    used_credits: i64,
}

fn de_int<'de, D>(d: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => {
            n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0)
        }
        _ => 0,
    })
}

impl UsageResponse {
    fn into_snapshot(self, plan: String) -> AnthropicSnapshot {
        let session = to_window(self.five_hour);
        let weekly = to_window(self.seven_day);
        let sonnet = self.seven_day_sonnet.map(|w| to_window(Some(w)));
        let extra = self
            .extra_usage
            .filter(|e| e.is_enabled)
            .map(|e| ExtraUsage {
                limit_cents: e.monthly_limit,
                spent_cents: e.used_credits,
            });
        AnthropicSnapshot {
            plan,
            session,
            weekly,
            sonnet,
            extra,
        }
    }
}

fn to_window(w: Option<Window>) -> UsageWindow {
    let Some(w) = w else {
        return UsageWindow {
            utilization_pct: 0,
            resets_at: None,
        };
    };
    UsageWindow {
        utilization_pct: w.utilization.round() as i32,
        resets_at: w
            .resets_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_response() {
        let raw = r#"{
            "five_hour":        {"utilization": 42.7, "resets_at": "2030-05-23T17:30:00Z"},
            "seven_day":        {"utilization": 27.0, "resets_at": "2030-05-30T12:00:00Z"},
            "seven_day_sonnet": {"utilization": 4.2,  "resets_at": "2030-05-30T12:00:00Z"},
            "extra_usage":      {"is_enabled": true, "monthly_limit": 5000, "used_credits": 250}
        }"#;
        let r: UsageResponse = serde_json::from_str(raw).unwrap();
        let s = r.into_snapshot("Max 5x".into());
        assert_eq!(s.session.utilization_pct, 43);
        assert_eq!(s.weekly.utilization_pct, 27);
        assert_eq!(s.sonnet.unwrap().utilization_pct, 4);
        assert_eq!(s.extra.unwrap().spent_cents, 250);
    }

    #[test]
    fn disabled_extra_is_none() {
        let raw = r#"{"five_hour":{"utilization":0},"seven_day":{"utilization":0},
            "extra_usage":{"is_enabled":false,"monthly_limit":5000,"used_credits":0}}"#;
        let r: UsageResponse = serde_json::from_str(raw).unwrap();
        assert!(r.into_snapshot("Pro".into()).extra.is_none());
    }
}
