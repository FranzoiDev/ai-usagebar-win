//! DeepSeek — `GET /user/balance`.

use serde::Deserialize;

use crate::config::{self, Config};
use crate::usage::{DeepseekSnapshot, VendorSnapshot};

use super::VendorState;

const BALANCE_URL: &str = "https://api.deepseek.com/user/balance";

pub fn fetch(client: &reqwest::blocking::Client, cfg: &Config) -> VendorState {
    let Some(key) =
        config::resolve_api_key(&cfg.deepseek.api_key_env, cfg.deepseek.api_key.as_deref())
    else {
        return VendorState::NeedsLogin(format!(
            "no API key — set {} or [deepseek] api_key",
            cfg.deepseek.api_key_env
        ));
    };
    let resp = match client
        .get(BALANCE_URL)
        .header("Authorization", format!("Bearer {key}"))
        .header("Accept", "application/json")
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
    let body: BalanceResponse = match resp.json() {
        Ok(b) => b,
        Err(e) => return VendorState::Error(format!("bad response: {e}")),
    };
    VendorState::Ok(VendorSnapshot::Deepseek(body.into_snapshot()))
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct BalanceResponse {
    is_available: bool,
    balance_infos: Vec<BalanceInfo>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct BalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

impl BalanceResponse {
    fn into_snapshot(self) -> DeepseekSnapshot {
        let info = self
            .balance_infos
            .iter()
            .find(|b| b.currency == "USD")
            .or_else(|| self.balance_infos.iter().find(|b| b.currency == "CNY"))
            .or_else(|| self.balance_infos.first())
            .cloned()
            .unwrap_or_default();
        DeepseekSnapshot {
            is_available: self.is_available,
            balance: parse_f64(&info.total_balance),
            granted: parse_f64(&info.granted_balance),
            topped_up: parse_f64(&info.topped_up_balance),
            currency: info.currency,
        }
    }
}

fn parse_f64(s: &str) -> f64 {
    s.trim().parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_usd() {
        let raw = r#"{"is_available":true,"balance_infos":[
            {"currency":"CNY","total_balance":"10.00","granted_balance":"10.00","topped_up_balance":"0.00"},
            {"currency":"USD","total_balance":"1.50","granted_balance":"1.50","topped_up_balance":"0.00"}
        ]}"#;
        let r: BalanceResponse = serde_json::from_str(raw).unwrap();
        let s = r.into_snapshot();
        assert_eq!(s.currency, "USD");
        assert!((s.balance - 1.5).abs() < 1e-9);
    }
}
