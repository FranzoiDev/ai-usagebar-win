//! Read-only readers for the OAuth credential files the official Claude and
//! Codex CLIs maintain.
//!
//! DESIGN RULE: this app never writes these files and never refreshes tokens.
//! Refreshing would rotate the refresh-token out from under the user's CLI and
//! risk logging them out of their AI tools. We only *read* the access token; if
//! it has already expired we report [`CredState::Expired`] and let the user
//! re-authenticate with their own CLI (`claude` / `codex login`).

use std::path::Path;

use base64::Engine;
use serde::Deserialize;

/// Outcome of reading a credential file.
pub enum CredState<T> {
    /// Valid, non-expired token.
    Valid(T),
    /// Token present but already expired — user must re-login via their CLI.
    Expired,
    /// File missing.
    Missing,
    /// File present but unparseable.
    Malformed(String),
}

// ---------------------------------------------------------------------------
// Anthropic — ~/.claude/.credentials.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AnthropicCreds {
    pub access_token: String,
    pub plan_label: String,
}

#[derive(Deserialize)]
struct AnthropicFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: AnthropicOauth,
}

#[derive(Deserialize)]
struct AnthropicOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt", default, deserialize_with = "de_num")]
    expires_at_ms: i64,
    #[serde(rename = "subscriptionType", default)]
    subscription_type: String,
    #[serde(rename = "rateLimitTier", default)]
    rate_limit_tier: String,
}

/// Read Anthropic credentials. `now_secs` is injected for testability.
pub fn read_anthropic(path: &Path, now_secs: i64) -> CredState<AnthropicCreds> {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return CredState::Missing,
        Err(e) => return CredState::Malformed(e.to_string()),
    };
    let file: AnthropicFile = match serde_json::from_str(&raw) {
        Ok(f) => f,
        Err(e) => return CredState::Malformed(e.to_string()),
    };
    let o = file.claude_ai_oauth;
    // Strict expiry: only flag once actually past expiry (we never refresh).
    if o.expires_at_ms > 0 && o.expires_at_ms / 1000 <= now_secs {
        return CredState::Expired;
    }
    CredState::Valid(AnthropicCreds {
        access_token: o.access_token,
        plan_label: anthropic_plan_label(&o.subscription_type, &o.rate_limit_tier),
    })
}

fn anthropic_plan_label(sub: &str, tier: &str) -> String {
    let mut name = capitalize(sub);
    if name.is_empty() {
        name = "Unknown".into();
    }
    if tier.contains("20x") {
        name.push_str(" 20x");
    } else if tier.contains("5x") {
        name.push_str(" 5x");
    }
    name
}

// ---------------------------------------------------------------------------
// OpenAI Codex — ~/.codex/auth.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OpenAiCreds {
    pub access_token: String,
    pub account_id: Option<String>,
    pub plan_hint: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiFile {
    tokens: OpenAiTokens,
}

#[derive(Deserialize)]
struct OpenAiTokens {
    access_token: String,
    id_token: String,
    #[serde(default)]
    account_id: Option<String>,
}

/// Read Codex credentials. Expiry + plan tier come from the id_token JWT.
pub fn read_openai(path: &Path, now_secs: i64) -> CredState<OpenAiCreds> {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return CredState::Missing,
        Err(e) => return CredState::Malformed(e.to_string()),
    };
    let file: OpenAiFile = match serde_json::from_str(&raw) {
        Ok(f) => f,
        Err(e) => return CredState::Malformed(e.to_string()),
    };
    let t = file.tokens;
    let claims = parse_jwt_claims(&t.id_token);
    let exp = claims
        .as_ref()
        .and_then(|c| c.get("exp"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    // exp == 0 means unparseable — attempt the fetch anyway; a 401 will then
    // surface as a re-login prompt. Only flag Expired when we KNOW it's past.
    if exp > 0 && exp <= now_secs {
        return CredState::Expired;
    }
    let plan_hint = claims.as_ref().and_then(|c| {
        c.get("https://api.openai.com/auth")
            .and_then(|v| v.get("chatgpt_plan_type"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });
    CredState::Valid(OpenAiCreds {
        access_token: t.access_token,
        account_id: t.account_id,
        plan_hint,
    })
}

fn parse_jwt_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}

// ---------------------------------------------------------------------------

fn de_num<'de, D>(d: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        _ => 0,
    })
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
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn tmp(s: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    fn jwt(claims: serde_json::Value) -> String {
        let h = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{}");
        let p =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
        format!("{h}.{p}.sig")
    }

    #[test]
    fn anthropic_valid_token() {
        let f = tmp(
            r#"{"claudeAiOauth":{"accessToken":"AT","refreshToken":"RT",
                "expiresAt":2000000000000,"subscriptionType":"max",
                "rateLimitTier":"default_claude_max_20x"}}"#,
        );
        match read_anthropic(f.path(), 1_000_000_000) {
            CredState::Valid(c) => {
                assert_eq!(c.access_token, "AT");
                assert_eq!(c.plan_label, "Max 20x");
            }
            _ => panic!("expected valid"),
        }
    }

    #[test]
    fn anthropic_expired_is_flagged_not_refreshed() {
        // expiresAt in the past → Expired, never a network refresh.
        let f = tmp(
            r#"{"claudeAiOauth":{"accessToken":"AT","refreshToken":"RT",
                "expiresAt":1000,"subscriptionType":"pro","rateLimitTier":""}}"#,
        );
        assert!(matches!(
            read_anthropic(f.path(), 1_000_000_000),
            CredState::Expired
        ));
    }

    #[test]
    fn anthropic_missing_file() {
        let p = std::path::Path::new("/nonexistent/aub-win/.credentials.json");
        assert!(matches!(read_anthropic(p, 0), CredState::Missing));
    }

    #[test]
    fn openai_valid_with_plan_hint() {
        let id = jwt(serde_json::json!({
            "exp": 2_000_000_000,
            "https://api.openai.com/auth": {"chatgpt_plan_type": "plus"}
        }));
        let body = format!(
            r#"{{"tokens":{{"access_token":"AT","refresh_token":"RT","id_token":"{id}","account_id":"acc"}}}}"#
        );
        let f = tmp(&body);
        match read_openai(f.path(), 1_000_000_000) {
            CredState::Valid(c) => {
                assert_eq!(c.access_token, "AT");
                assert_eq!(c.account_id.as_deref(), Some("acc"));
                assert_eq!(c.plan_hint.as_deref(), Some("plus"));
            }
            _ => panic!("expected valid"),
        }
    }

    #[test]
    fn openai_expired() {
        let id = jwt(serde_json::json!({"exp": 1000}));
        let body =
            format!(r#"{{"tokens":{{"access_token":"AT","refresh_token":"RT","id_token":"{id}"}}}}"#);
        let f = tmp(&body);
        assert!(matches!(
            read_openai(f.path(), 1_000_000_000),
            CredState::Expired
        ));
    }

    #[test]
    fn openai_unparseable_exp_attempts_anyway() {
        let body =
            r#"{"tokens":{"access_token":"AT","refresh_token":"RT","id_token":"not.a.jwt"}}"#;
        let f = tmp(body);
        // exp unknown → Valid (we try; a live 401 would prompt re-login).
        assert!(matches!(
            read_openai(f.path(), 1_000_000_000),
            CredState::Valid(_)
        ));
    }
}
