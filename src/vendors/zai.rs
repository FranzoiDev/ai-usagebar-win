//! Z.AI / BigModel — `GET /api/monitor/usage/quota/limit`.
//! Auth quirk: API key in `Authorization` WITHOUT the `Bearer ` prefix.

use serde::Deserialize;

use crate::config::{self, Config};
use crate::usage::{UsageWindow, VendorSnapshot, ZaiSnapshot};

use super::VendorState;

const QUOTA_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";

pub fn fetch(client: &reqwest::blocking::Client, cfg: &Config) -> VendorState {
    let Some(key) = config::resolve_api_key(&cfg.zai.api_key_env, cfg.zai.api_key.as_deref()) else {
        return VendorState::NeedsLogin(format!(
            "no API key — set {} or [zai] api_key",
            cfg.zai.api_key_env
        ));
    };
    let resp = match client
        .get(QUOTA_URL)
        .header("Authorization", &key) // NO "Bearer " prefix.
        .header("Accept-Language", "en-US,en")
        .header("Content-Type", "application/json")
        .send()
    {
        Ok(r) => r,
        Err(e) => return VendorState::Error(format!("network: {e}")),
    };
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return VendorState::NeedsLogin("API key rejected (401)".into());
    }
    if !status.is_success() {
        return VendorState::Error(format!("HTTP {}", status.as_u16()));
    }
    let env: Envelope = match resp.json() {
        Ok(e) => e,
        Err(e) => return VendorState::Error(format!("bad response: {e}")),
    };
    VendorState::Ok(VendorSnapshot::Zai(
        env.into_snapshot(cfg.zai.plan_tier.as_deref()),
    ))
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    data: Option<MonitorData>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MonitorData {
    limits: Vec<LimitEntry>,
    level: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LimitEntry {
    #[serde(rename = "type")]
    kind: String,
    percentage: f64,
    #[serde(rename = "nextResetTime", default, deserialize_with = "de_opt_ms")]
    next_reset_time: Option<i64>,
}

fn de_opt_ms<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    })
}

impl Envelope {
    fn into_snapshot(self, config_tier: Option<&str>) -> ZaiSnapshot {
        let data = self.data.unwrap_or_default();
        let mut tokens = data.limits.iter().filter(|l| l.kind == "TOKENS_LIMIT");
        let session = tokens.next().map(to_window);
        let weekly = tokens.next().map(to_window);
        let mcp = data.limits.iter().find(|l| l.kind == "TIME_LIMIT").map(to_window);
        let level = if !data.level.is_empty() {
            data.level
        } else {
            config_tier.unwrap_or("unknown").to_string()
        };
        ZaiSnapshot {
            plan: format!("GLM Coding {}", capitalize(&level)),
            session,
            weekly,
            mcp,
        }
    }
}

fn to_window(l: &LimitEntry) -> UsageWindow {
    UsageWindow {
        utilization_pct: l.percentage.round().clamp(0.0, 100.0) as i32,
        resets_at: l
            .next_reset_time
            .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis),
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
        let raw = r#"{"code":200,"data":{"limits":[
            {"type":"TOKENS_LIMIT","percentage":42},
            {"type":"TOKENS_LIMIT","percentage":15,"nextResetTime":1779792169974},
            {"type":"TIME_LIMIT","percentage":3}
        ],"level":"pro"},"success":true}"#;
        let env: Envelope = serde_json::from_str(raw).unwrap();
        let s = env.into_snapshot(None);
        assert_eq!(s.plan, "GLM Coding Pro");
        assert_eq!(s.session.unwrap().utilization_pct, 42);
        assert_eq!(s.weekly.unwrap().utilization_pct, 15);
        assert!(s.mcp.is_some());
    }

    #[test]
    fn config_tier_when_level_empty() {
        let raw = r#"{"data":{"limits":[],"level":""}}"#;
        let env: Envelope = serde_json::from_str(raw).unwrap();
        assert_eq!(env.into_snapshot(Some("max")).plan, "GLM Coding Max");
    }
}
