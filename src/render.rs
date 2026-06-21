//! Turn vendor reports into what the tray shows: the worst severity (icon
//! color), a compact tooltip, and the detailed per-vendor menu lines.

use chrono::{DateTime, Utc};

use crate::usage::{fmt_reset, severity_for, Severity, VendorSnapshot};
use crate::vendors::{VendorId, VendorReport, VendorState};

pub struct Rendered {
    /// Drives the tray icon color. `Low` when nothing reports a percent.
    pub severity: Severity,
    /// Short multi-line tooltip (kept compact for the Win32 tooltip limit).
    pub tooltip: String,
    /// One block of detailed lines per vendor, for the context menu.
    pub menu_lines: Vec<String>,
}

pub fn render(reports: &[VendorReport], primary: VendorId, now: DateTime<Utc>) -> Rendered {
    // Worst percentage across all successful snapshots drives the icon.
    let worst = reports
        .iter()
        .filter_map(|r| match &r.state {
            VendorState::Ok(s) => s.worst_pct(),
            _ => None,
        })
        .max();
    let severity = worst.map(severity_for).unwrap_or(Severity::Low);

    // Tooltip: primary vendor first, then a one-liner per other vendor.
    let mut ordered: Vec<&VendorReport> = reports.iter().collect();
    ordered.sort_by_key(|r| (r.id != primary, VendorId::ALL.iter().position(|v| *v == r.id)));

    let mut tip_lines = Vec::new();
    let mut menu_lines = Vec::new();
    for r in &ordered {
        tip_lines.push(tooltip_line(r, now));
        menu_lines.extend(menu_block(r, now));
        menu_lines.push(String::new()); // blank separator between vendors
    }
    if menu_lines.last().map(|s| s.is_empty()).unwrap_or(false) {
        menu_lines.pop();
    }
    let tooltip = if tip_lines.is_empty() {
        "ai-usagebar — no vendors enabled".to_string()
    } else {
        tip_lines.join("\n")
    };

    Rendered {
        severity,
        tooltip,
        menu_lines,
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

/// Detailed multi-line block for the context menu.
fn menu_block(r: &VendorReport, now: DateTime<Utc>) -> Vec<String> {
    let mut out = Vec::new();
    match &r.state {
        VendorState::NeedsLogin(msg) => {
            out.push(format!("{}  —  {}", r.id.display(), msg));
        }
        VendorState::Error(msg) => {
            out.push(format!("{}  —  error: {}", r.id.display(), msg));
        }
        VendorState::Ok(snap) => match snap {
            VendorSnapshot::Anthropic(s) => {
                out.push(format!("{} ({})", r.id.display(), s.plan));
                out.push(win("Session (5h)", s.session.utilization_pct, s.session.resets_at, now));
                out.push(win("Weekly", s.weekly.utilization_pct, s.weekly.resets_at, now));
                if let Some(w) = &s.sonnet {
                    out.push(win("Sonnet (weekly)", w.utilization_pct, w.resets_at, now));
                }
                if let Some(e) = s.extra {
                    out.push(format!("    Extra usage: {} ({}%)", e.fmt(), e.percent()));
                }
            }
            VendorSnapshot::Openai(s) => {
                out.push(format!("{} ({})", r.id.display(), s.plan));
                out.push(win("Session (5h)", s.session.utilization_pct, s.session.resets_at, now));
                out.push(win("Weekly", s.weekly.utilization_pct, s.weekly.resets_at, now));
                if let Some(w) = &s.code_review {
                    out.push(win("Code review", w.utilization_pct, w.resets_at, now));
                }
                if let Some(c) = &s.credits {
                    out.push(format!("    Credits: {}", c.balance));
                }
            }
            VendorSnapshot::Zai(s) => {
                out.push(format!("{} ({})", r.id.display(), s.plan));
                if let Some(w) = &s.session {
                    out.push(win("Session (5h)", w.utilization_pct, w.resets_at, now));
                }
                if let Some(w) = &s.weekly {
                    out.push(win("Weekly", w.utilization_pct, w.resets_at, now));
                }
                if let Some(w) = &s.mcp {
                    out.push(win("MCP (monthly)", w.utilization_pct, w.resets_at, now));
                }
            }
            VendorSnapshot::Openrouter(s) => {
                out.push(s.label.clone());
                out.push(format!("    Balance: {}", money(s.balance())));
                out.push(format!(
                    "    Spent: {} total · {} today",
                    money(s.total_usage),
                    money(s.usage_daily)
                ));
                if let Some(lim) = s.limit {
                    out.push(format!("    Key limit: {}", money(lim)));
                }
            }
            VendorSnapshot::Deepseek(s) => {
                out.push(format!("{} balance", r.id.display()));
                out.push(format!(
                    "    {}{} ({})",
                    currency_sym(&s.currency),
                    trim(s.balance),
                    if s.is_available { "available" } else { "unavailable" }
                ));
            }
        },
    }
    out
}

fn win(label: &str, pct: i32, reset: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    format!("    {label}: {pct}% · resets {}", fmt_reset(reset, now))
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
        let r = render(&reports, VendorId::Anthropic, Utc::now());
        assert_eq!(r.severity, Severity::Critical);
    }

    #[test]
    fn tooltip_has_compact_line() {
        let reports = vec![anthropic_report(29, 10)];
        let r = render(&reports, VendorId::Anthropic, Utc::now());
        assert!(r.tooltip.contains("cld 29%"));
    }

    #[test]
    fn needs_login_renders_gracefully() {
        let reports = vec![VendorReport {
            id: VendorId::Openai,
            state: VendorState::NeedsLogin("run codex login".into()),
        }];
        let r = render(&reports, VendorId::Anthropic, Utc::now());
        assert!(r.tooltip.contains("gpt: login needed"));
        assert_eq!(r.severity, Severity::Low);
    }

    #[test]
    fn primary_vendor_leads_tooltip() {
        let reports = vec![
            anthropic_report(10, 10),
            VendorReport {
                id: VendorId::Openai,
                state: VendorState::NeedsLogin("x".into()),
            },
        ];
        let r = render(&reports, VendorId::Openai, Utc::now());
        assert!(r.tooltip.starts_with("gpt"));
    }
}
