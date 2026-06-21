//! Canonical in-memory representation of "how much have I used my plan".
//!
//! Reverse-engineered from the Linux `ai-usagebar` crate (`src/usage.rs`). Each
//! vendor keeps its own snapshot variant because the providers expose genuinely
//! different shapes — forcing them into one struct would drop information.

use chrono::{DateTime, Utc};

/// A single usage window: percent used (0..=100) + when it resets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageWindow {
    pub utilization_pct: i32,
    pub resets_at: Option<DateTime<Utc>>,
}

/// Anthropic Claude — three rolling windows + optional pay-as-you-go credits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicSnapshot {
    pub plan: String,
    pub session: UsageWindow,
    pub weekly: UsageWindow,
    pub sonnet: Option<UsageWindow>,
    pub extra: Option<ExtraUsage>,
}

/// "Extra usage" pay-as-you-go block. Money in integer cents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtraUsage {
    pub limit_cents: i64,
    pub spent_cents: i64,
}

impl ExtraUsage {
    pub fn percent(self) -> i32 {
        if self.limit_cents <= 0 {
            0
        } else {
            ((self.spent_cents * 100) / self.limit_cents) as i32
        }
    }
    pub fn fmt(self) -> String {
        format!(
            "{} / {}",
            fmt_cents(self.spent_cents),
            fmt_cents(self.limit_cents)
        )
    }
}

fn fmt_cents(c: i64) -> String {
    let (sign, abs) = if c < 0 { ("-", -c) } else { ("", c) };
    format!("{sign}${}.{:02}", abs / 100, abs % 100)
}

/// OpenAI Codex OAuth — two windows + optional code-review bucket + credits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiSnapshot {
    pub plan: String,
    pub session: UsageWindow,
    pub weekly: UsageWindow,
    pub code_review: Option<UsageWindow>,
    pub credits: Option<OpenAiCredits>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCredits {
    pub balance: String,
    pub has_credits: bool,
    pub unlimited: bool,
}

/// Z.AI / BigModel — session/weekly token buckets + monthly MCP ceiling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZaiSnapshot {
    pub plan: String,
    pub session: Option<UsageWindow>,
    pub weekly: Option<UsageWindow>,
    pub mcp: Option<UsageWindow>,
}

/// OpenRouter — credit balance + daily/weekly/monthly spend.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRouterSnapshot {
    pub label: String,
    pub total_credits: f64,
    pub total_usage: f64,
    pub usage_daily: f64,
    pub usage_weekly: f64,
    pub usage_monthly: f64,
    pub is_free_tier: bool,
    pub limit: Option<f64>,
    pub limit_remaining: Option<f64>,
}

impl OpenRouterSnapshot {
    pub fn balance(&self) -> f64 {
        (self.total_credits - self.total_usage).max(0.0)
    }
    pub fn consumed_pct(&self) -> i32 {
        if self.total_credits <= 0.0 {
            return 0;
        }
        ((self.total_usage / self.total_credits) * 100.0)
            .round()
            .clamp(0.0, 100.0) as i32
    }
}

/// DeepSeek — credit balance from `/user/balance`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeepseekSnapshot {
    pub is_available: bool,
    pub balance: f64,
    pub granted: f64,
    pub topped_up: f64,
    pub currency: String,
}

/// Discriminated union of vendor snapshots.
#[derive(Debug, Clone, PartialEq)]
pub enum VendorSnapshot {
    Anthropic(AnthropicSnapshot),
    Openai(OpenAiSnapshot),
    Zai(ZaiSnapshot),
    Openrouter(OpenRouterSnapshot),
    Deepseek(DeepseekSnapshot),
}

impl VendorSnapshot {
    /// Worst-case utilization across the snapshot's windows, used to drive the
    /// tray icon color. `None` for balance-only vendors (OpenRouter/DeepSeek)
    /// that don't express a meaningful "percent of plan used".
    pub fn worst_pct(&self) -> Option<i32> {
        match self {
            VendorSnapshot::Anthropic(s) => {
                let mut m = s.session.utilization_pct.max(s.weekly.utilization_pct);
                if let Some(w) = &s.sonnet {
                    m = m.max(w.utilization_pct);
                }
                Some(m)
            }
            VendorSnapshot::Openai(s) => {
                Some(s.session.utilization_pct.max(s.weekly.utilization_pct))
            }
            VendorSnapshot::Zai(s) => [&s.session, &s.weekly, &s.mcp]
                .iter()
                .filter_map(|w| w.as_ref().map(|x| x.utilization_pct))
                .max(),
            VendorSnapshot::Openrouter(s) => Some(s.consumed_pct()),
            VendorSnapshot::Deepseek(_) => None,
        }
    }
}

/// Severity tiers (mirror the Linux widget's thresholds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Low,
    Mid,
    High,
    Critical,
}

pub fn severity_for(pct: i32) -> Severity {
    if pct >= 90 {
        Severity::Critical
    } else if pct >= 75 {
        Severity::High
    } else if pct >= 50 {
        Severity::Mid
    } else {
        Severity::Low
    }
}

/// Compact human countdown: "2h 13m", "3d 4h", "now", or "—" when unknown.
pub fn fmt_reset(reset: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(reset) = reset else {
        return "—".to_string();
    };
    let secs = reset.signed_duration_since(now).num_seconds();
    if secs <= 0 {
        return "now".to_string();
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn severity_thresholds() {
        assert_eq!(severity_for(10), Severity::Low);
        assert_eq!(severity_for(50), Severity::Mid);
        assert_eq!(severity_for(75), Severity::High);
        assert_eq!(severity_for(90), Severity::Critical);
    }

    #[test]
    fn fmt_reset_buckets() {
        let now = Utc::now();
        assert_eq!(fmt_reset(None, now), "—");
        assert_eq!(fmt_reset(Some(now - Duration::minutes(1)), now), "now");
        assert_eq!(fmt_reset(Some(now + Duration::minutes(45)), now), "45m");
        assert!(fmt_reset(Some(now + Duration::hours(2)), now).starts_with("2h"));
        assert!(fmt_reset(Some(now + Duration::days(3)), now).starts_with("3d"));
    }

    #[test]
    fn extra_usage_percent_and_fmt() {
        let e = ExtraUsage {
            limit_cents: 5000,
            spent_cents: 250,
        };
        assert_eq!(e.percent(), 5);
        assert_eq!(e.fmt(), "$2.50 / $50.00");
    }

    #[test]
    fn openrouter_consumed_pct_guards_zero() {
        let s = OpenRouterSnapshot {
            label: "x".into(),
            total_credits: 0.0,
            total_usage: 5.0,
            usage_daily: 0.0,
            usage_weekly: 0.0,
            usage_monthly: 0.0,
            is_free_tier: true,
            limit: None,
            limit_remaining: None,
        };
        assert_eq!(s.consumed_pct(), 0);
    }
}
