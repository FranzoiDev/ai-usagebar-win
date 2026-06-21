//! OpenAI Codex — `GET https://chatgpt.com/backend-api/wham/usage` with the
//! Codex CLI's access token. Read-only: no token refresh.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::config::{self, Config};
use crate::creds::{self, CredState};
use crate::usage::{OpenAiCredits, OpenAiSnapshot, UsageWindow, VendorSnapshot};

use super::VendorState;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub fn fetch(client: &reqwest::blocking::Client, cfg: &Config, now: DateTime<Utc>) -> VendorState {
    let Some(path) = config::openai_auth_path(&cfg.openai) else {
        return VendorState::Error("could not resolve home directory".into());
    };
    let creds = match creds::read_openai(&path, now.timestamp()) {
        CredState::Valid(c) => c,
        CredState::Expired => {
            return VendorState::NeedsLogin("token expired — run `codex login`".into());
        }
        CredState::Missing => {
            return VendorState::NeedsLogin("not logged in — run `codex login`".into());
        }
        CredState::Malformed(e) => return VendorState::Error(format!("bad auth file: {e}")),
    };

    let mut req = client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("User-Agent", "codex-cli");
    if let Some(aid) = &creds.account_id {
        req = req.header("ChatGPT-Account-Id", aid);
    }
    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => return VendorState::Error(format!("network: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return VendorState::NeedsLogin("session invalid — run `codex login`".into());
    }
    if !status.is_success() {
        return VendorState::Error(format!("HTTP {}", status.as_u16()));
    }
    let body: UsageResponse = match resp.json() {
        Ok(b) => b,
        Err(e) => return VendorState::Error(format!("bad response: {e}")),
    };
    VendorState::Ok(VendorSnapshot::Openai(
        body.into_snapshot(creds.plan_hint.as_deref()),
    ))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UsageResponse {
    plan_type: Option<String>,
    rate_limit: Option<RateLimit>,
    code_review_rate_limit: Option<RateLimit>,
    credits: Option<CreditsBlock>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RateLimit {
    primary_window: Option<Window>,
    secondary_window: Option<Window>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Window {
    #[serde(deserialize_with = "de_int")]
    used_percent: i64,
    #[serde(default, deserialize_with = "de_opt_int")]
    reset_at: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_int")]
    reset_after_seconds: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CreditsBlock {
    #[serde(deserialize_with = "de_money")]
    balance: String,
    has_credits: bool,
    unlimited: bool,
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

fn de_opt_int<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    })
}

fn de_money<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => format!("${:.2}", n.as_f64().unwrap_or(0.0)),
        _ => "$0.00".to_string(),
    })
}

impl UsageResponse {
    fn into_snapshot(self, plan_hint: Option<&str>) -> OpenAiSnapshot {
        let plan_type = self.plan_type.as_deref().or(plan_hint).unwrap_or("Unknown");
        let plan = format!("ChatGPT {}", capitalize(plan_type));
        let rl = self.rate_limit.unwrap_or_default();
        let session = window_or_default(rl.primary_window);
        let weekly = window_or_default(rl.secondary_window);
        let code_review = self
            .code_review_rate_limit
            .and_then(|c| c.primary_window)
            .map(|w| to_window(&w));
        let credits = self.credits.map(|c| OpenAiCredits {
            balance: c.balance,
            has_credits: c.has_credits,
            unlimited: c.unlimited,
        });
        OpenAiSnapshot {
            plan,
            session,
            weekly,
            code_review,
            credits,
        }
    }
}

fn window_or_default(w: Option<Window>) -> UsageWindow {
    match w {
        Some(w) => to_window(&w),
        None => UsageWindow {
            utilization_pct: 0,
            resets_at: None,
        },
    }
}

fn to_window(w: &Window) -> UsageWindow {
    let resets_at = match w.reset_at {
        Some(secs) => DateTime::<Utc>::from_timestamp(secs, 0),
        None => w
            .reset_after_seconds
            .map(|s| Utc::now() + chrono::Duration::seconds(s)),
    };
    UsageWindow {
        utilization_pct: (w.used_percent as i32).clamp(0, 100),
        resets_at,
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_shape() {
        let raw = r#"{"plan_type":"plus","rate_limit":{
            "primary_window":{"used_percent":1,"limit_window_seconds":18000,"reset_at":1779597324},
            "secondary_window":{"used_percent":0,"limit_window_seconds":604800,"reset_at":1780184124}
        }}"#;
        let r: UsageResponse = serde_json::from_str(raw).unwrap();
        let s = r.into_snapshot(None);
        assert_eq!(s.plan, "ChatGPT Plus");
        assert_eq!(s.session.utilization_pct, 1);
        assert!(s.session.resets_at.is_some());
    }

    #[test]
    fn used_percent_clamps() {
        let raw = r#"{"rate_limit":{"primary_window":{"used_percent":250}}}"#;
        let r: UsageResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(r.into_snapshot(None).session.utilization_pct, 100);
    }

    #[test]
    fn plan_hint_used_when_absent() {
        let r: UsageResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(r.into_snapshot(Some("team")).plan, "ChatGPT Team");
    }
}
