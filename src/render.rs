//! Turn vendor reports into what the UI shows.
//!
//! Two consumers:
//!   * the **tray** wants the worst severity (icon color) + a compact tooltip;
//!   * the **WebView popup / settings** want structured, serializable models
//!     that the embedded HTML renders into cards + progress bars.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::config::Config;
use crate::usage::{fmt_reset, severity_for, Severity, UsageWindow, VendorSnapshot};
use crate::vendors::{VendorId, VendorReport, VendorState};

pub struct Rendered {
    /// Drives the tray icon color. `Low` when nothing reports a percent.
    pub severity: Severity,
    /// Short multi-line tooltip (kept compact for the Win32 tooltip limit).
    pub tooltip: String,
}

/// Tray icon color + hover tooltip. Like the popup, the tooltip lists only
/// vendors with an identified key/credential (see [`should_show`]).
pub fn render(
    reports: &[VendorReport],
    cfg: &Config,
    primary: VendorId,
    now: DateTime<Utc>,
) -> Rendered {
    let worst = reports
        .iter()
        .filter_map(|r| match &r.state {
            VendorState::Ok(s) => s.worst_pct(),
            _ => None,
        })
        .max();
    let severity = worst.map(severity_for).unwrap_or(Severity::Low);

    let mut ordered: Vec<&VendorReport> = reports.iter().collect();
    ordered.sort_by_key(|r| (r.id != primary, VendorId::ALL.iter().position(|v| *v == r.id)));

    let tip_lines: Vec<String> = ordered
        .iter()
        .filter(|r| should_show(&r.state, cfg, r.id))
        .map(|r| tooltip_line(r, now))
        .collect();
    let tooltip = if tip_lines.is_empty() {
        "ai-usagebar — no models configured".to_string()
    } else {
        tip_lines.join("\n")
    };

    Rendered { severity, tooltip }
}

/// Whether a vendor should surface in the popup/tooltip: it has an identified
/// key/credential. Unconfigured and login-needed vendors are hidden (they
/// belong in the settings window instead).
fn should_show(state: &VendorState, cfg: &Config, id: VendorId) -> bool {
    match state {
        VendorState::Ok(_) => true,
        VendorState::Error(_) => cfg.is_configured(id),
        VendorState::NeedsLogin(_) => false,
    }
}

/// One compact line, e.g. "cld 29% · 1h12m" or "gpt: login needed".
fn tooltip_line(r: &VendorReport, now: DateTime<Utc>) -> String {
    let tag = r.id.short();
    match &r.state {
        VendorState::NeedsLogin(_) => format!("{tag}: login needed"),
        VendorState::Error(_) => format!("{tag}: unavailable"),
        VendorState::Ok(snap) => match snap {
            VendorSnapshot::Anthropic(s) => format!(
                "{tag} {}% · {}",
                s.session.utilization_pct,
                fmt_reset(s.session.resets_at, now)
            ),
            VendorSnapshot::Openai(s) => format!(
                "{tag} {}% · {}",
                s.session.utilization_pct,
                fmt_reset(s.session.resets_at, now)
            ),
            VendorSnapshot::Zai(s) => {
                let p = s.session.as_ref().map(|w| w.utilization_pct).unwrap_or(0);
                format!("{tag} {p}%")
            }
            VendorSnapshot::Openrouter(s) => format!("{tag} {}", money(s.balance())),
            VendorSnapshot::Deepseek(s) => {
                format!("{tag} {}{}", currency_sym(&s.currency), trim(s.balance))
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Popup view-model — only vendors with an identified key (Ok, or configured
// but currently erroring). Login-needed / unconfigured vendors are hidden
// here and surfaced in the settings window instead.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PopupModel {
    pub vendors: Vec<VendorCard>,
}

#[derive(Serialize)]
pub struct VendorCard {
    pub id: String,
    pub name: String,
    pub plan: Option<String>,
    /// "ok" | "error"
    pub status: String,
    pub message: Option<String>,
    pub bars: Vec<Bar>,
    pub facts: Vec<Fact>,
}

#[derive(Serialize)]
pub struct Bar {
    pub label: String,
    pub pct: i32,
    pub reset: Option<String>,
    /// "low" | "mid" | "high" | "critical"
    pub level: String,
}

#[derive(Serialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

pub fn popup_model(
    reports: &[VendorReport],
    cfg: &Config,
    primary: VendorId,
    now: DateTime<Utc>,
) -> PopupModel {
    let mut ordered: Vec<&VendorReport> = reports.iter().collect();
    ordered.sort_by_key(|r| (r.id != primary, VendorId::ALL.iter().position(|v| *v == r.id)));

    let mut vendors = Vec::new();
    for r in ordered {
        if !should_show(&r.state, cfg, r.id) {
            continue;
        }
        match &r.state {
            VendorState::Ok(snap) => vendors.push(ok_card(r.id, snap, now)),
            // Configured-but-erroring vendors show so problems are visible.
            VendorState::Error(msg) => vendors.push(VendorCard {
                id: vendor_id_str(r.id),
                name: r.id.display().to_string(),
                plan: None,
                status: "error".into(),
                message: Some(msg.clone()),
                bars: Vec::new(),
                facts: Vec::new(),
            }),
            VendorState::NeedsLogin(_) => {}
        }
    }
    PopupModel { vendors }
}

fn ok_card(id: VendorId, snap: &VendorSnapshot, now: DateTime<Utc>) -> VendorCard {
    let mut bars = Vec::new();
    let mut facts = Vec::new();
    let mut plan = None;

    match snap {
        VendorSnapshot::Anthropic(s) => {
            plan = Some(s.plan.clone());
            bars.push(bar("Session (5h)", &s.session, now));
            bars.push(bar("Weekly", &s.weekly, now));
            if let Some(w) = &s.sonnet {
                bars.push(bar("Sonnet (weekly)", w, now));
            }
            if let Some(e) = s.extra {
                facts.push(Fact {
                    label: "Extra usage".into(),
                    value: format!("{} ({}%)", e.fmt(), e.percent()),
                });
            }
        }
        VendorSnapshot::Openai(s) => {
            plan = Some(s.plan.clone());
            bars.push(bar("Session (5h)", &s.session, now));
            bars.push(bar("Weekly", &s.weekly, now));
            if let Some(w) = &s.code_review {
                bars.push(bar("Code review", w, now));
            }
            if let Some(c) = &s.credits {
                facts.push(Fact {
                    label: "Credits".into(),
                    value: c.balance.clone(),
                });
            }
        }
        VendorSnapshot::Zai(s) => {
            plan = Some(s.plan.clone());
            if let Some(w) = &s.session {
                bars.push(bar("Session (5h)", w, now));
            }
            if let Some(w) = &s.weekly {
                bars.push(bar("Weekly", w, now));
            }
            if let Some(w) = &s.mcp {
                bars.push(bar("MCP (monthly)", w, now));
            }
        }
        VendorSnapshot::Openrouter(s) => {
            plan = Some(s.label.clone());
            bars.push(Bar {
                label: "Credits used".into(),
                pct: s.consumed_pct(),
                reset: None,
                level: level_str(s.consumed_pct()).into(),
            });
            facts.push(Fact {
                label: "Balance".into(),
                value: money(s.balance()),
            });
            facts.push(Fact {
                label: "Spent today".into(),
                value: money(s.usage_daily),
            });
            if let Some(lim) = s.limit {
                facts.push(Fact {
                    label: "Key limit".into(),
                    value: money(lim),
                });
            }
        }
        VendorSnapshot::Deepseek(s) => {
            facts.push(Fact {
                label: "Balance".into(),
                value: format!("{}{}", currency_sym(&s.currency), trim(s.balance)),
            });
            facts.push(Fact {
                label: "Status".into(),
                value: if s.is_available { "available" } else { "unavailable" }.into(),
            });
        }
    }

    VendorCard {
        id: vendor_id_str(id),
        name: id.display().to_string(),
        plan,
        status: "ok".into(),
        message: None,
        bars,
        facts,
    }
}

fn bar(label: &str, w: &UsageWindow, now: DateTime<Utc>) -> Bar {
    Bar {
        label: label.into(),
        pct: w.utilization_pct,
        reset: w.resets_at.map(|_| fmt_reset(w.resets_at, now)),
        level: level_str(w.utilization_pct).into(),
    }
}

// ---------------------------------------------------------------------------
// Settings view-model — every supported vendor, configured or not.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SettingsModel {
    pub poll_seconds: u64,
    pub primary: String,
    pub vendors: Vec<VendorSetting>,
}

#[derive(Serialize)]
pub struct VendorSetting {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub configured: bool,
    /// "oauth" (Anthropic/OpenAI) | "apikey" (Z.AI/OpenRouter/DeepSeek)
    pub kind: String,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub plan_tier: Option<String>,
    /// Path hint for OAuth vendors / where the key is read from.
    pub hint: Option<String>,
    pub status: Option<String>,
}

pub fn settings_model(cfg: &Config, reports: &[VendorReport]) -> SettingsModel {
    let primary = cfg.ui.primary.unwrap_or(VendorId::Anthropic);
    let vendors = VendorId::ALL
        .iter()
        .map(|&id| vendor_setting(id, cfg, reports))
        .collect();
    SettingsModel {
        poll_seconds: cfg.poll_seconds.unwrap_or(60).max(15),
        primary: vendor_id_str(primary),
        vendors,
    }
}

fn vendor_setting(id: VendorId, cfg: &Config, reports: &[VendorReport]) -> VendorSetting {
    let status = reports
        .iter()
        .find(|r| r.id == id)
        .map(|r| state_label(&r.state));

    let (kind, api_key_env, api_key, plan_tier, hint) = match id {
        VendorId::Anthropic => (
            "oauth",
            None,
            None,
            None,
            Some("Reads ~/.claude/.credentials.json — sign in with the `claude` CLI.".to_string()),
        ),
        VendorId::Openai => (
            "oauth",
            None,
            None,
            None,
            Some("Reads ~/.codex/auth.json — sign in with `codex login`.".to_string()),
        ),
        VendorId::Zai => (
            "apikey",
            Some(cfg.zai.api_key_env.clone()),
            cfg.zai.api_key.clone(),
            cfg.zai.plan_tier.clone(),
            None,
        ),
        VendorId::Openrouter => (
            "apikey",
            Some(cfg.openrouter.api_key_env.clone()),
            cfg.openrouter.api_key.clone(),
            None,
            None,
        ),
        VendorId::Deepseek => (
            "apikey",
            Some(cfg.deepseek.api_key_env.clone()),
            cfg.deepseek.api_key.clone(),
            None,
            None,
        ),
    };

    VendorSetting {
        id: vendor_id_str(id),
        name: id.display().to_string(),
        enabled: cfg.is_enabled(id),
        configured: cfg.is_configured(id),
        kind: kind.into(),
        api_key_env,
        api_key,
        plan_tier,
        hint,
        status,
    }
}

fn state_label(state: &VendorState) -> String {
    match state {
        VendorState::Ok(_) => "Connected".into(),
        VendorState::NeedsLogin(m) => format!("Login needed — {m}"),
        VendorState::Error(m) => format!("Error — {m}"),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers.
// ---------------------------------------------------------------------------

fn vendor_id_str(id: VendorId) -> String {
    match id {
        VendorId::Anthropic => "anthropic",
        VendorId::Openai => "openai",
        VendorId::Zai => "zai",
        VendorId::Openrouter => "openrouter",
        VendorId::Deepseek => "deepseek",
    }
    .to_string()
}

fn level_str(pct: i32) -> &'static str {
    match severity_for(pct) {
        Severity::Low => "low",
        Severity::Mid => "mid",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

fn money(v: f64) -> String {
    format!("${:.2}", v)
}

fn trim(v: f64) -> String {
    format!("{:.2}", v)
}

fn currency_sym(cur: &str) -> &str {
    match cur {
        "USD" => "$",
        "CNY" => "¥",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{AnthropicSnapshot, UsageWindow};

    fn anthropic_report(session: i32, weekly: i32) -> VendorReport {
        VendorReport {
            id: VendorId::Anthropic,
            state: VendorState::Ok(VendorSnapshot::Anthropic(AnthropicSnapshot {
                plan: "Max 5x".into(),
                session: UsageWindow {
                    utilization_pct: session,
                    resets_at: None,
                },
                weekly: UsageWindow {
                    utilization_pct: weekly,
                    resets_at: None,
                },
                sonnet: None,
                extra: None,
            })),
        }
    }

    #[test]
    fn severity_tracks_worst_window() {
        let reports = vec![anthropic_report(40, 95)];
        let r = render(&reports, &Config::default(), VendorId::Anthropic, Utc::now());
        assert_eq!(r.severity, Severity::Critical);
    }

    #[test]
    fn tooltip_has_compact_line() {
        let reports = vec![anthropic_report(29, 10)];
        let r = render(&reports, &Config::default(), VendorId::Anthropic, Utc::now());
        assert!(r.tooltip.contains("cld 29%"));
    }

    #[test]
    fn tooltip_hides_unconfigured_login_needed_vendor() {
        let reports = vec![VendorReport {
            id: VendorId::Openai,
            state: VendorState::NeedsLogin("run codex login".into()),
        }];
        let r = render(&reports, &Config::default(), VendorId::Anthropic, Utc::now());
        assert!(!r.tooltip.contains("gpt"));
        assert_eq!(r.tooltip, "ai-usagebar — no models configured");
        assert_eq!(r.severity, Severity::Low);
    }

    #[test]
    fn popup_includes_ok_vendor_with_bars() {
        let reports = vec![anthropic_report(62, 24)];
        let m = popup_model(&reports, &Config::default(), VendorId::Anthropic, Utc::now());
        assert_eq!(m.vendors.len(), 1);
        assert_eq!(m.vendors[0].status, "ok");
        assert_eq!(m.vendors[0].bars[0].pct, 62);
        assert_eq!(m.vendors[0].bars[0].level, "mid");
    }

    #[test]
    fn popup_hides_login_needed_vendor() {
        let reports = vec![VendorReport {
            id: VendorId::Openai,
            state: VendorState::NeedsLogin("x".into()),
        }];
        let m = popup_model(&reports, &Config::default(), VendorId::Anthropic, Utc::now());
        assert!(m.vendors.is_empty());
    }

    #[test]
    fn settings_lists_every_vendor() {
        let m = settings_model(&Config::default(), &[]);
        assert_eq!(m.vendors.len(), VendorId::ALL.len());
        assert_eq!(m.primary, "anthropic");
    }
}
