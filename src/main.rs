//! ai-usagebar-win — a 100% native Windows notification-area (tray) app that
//! shows how much of your AI plans you've used.
//!
//! It reads the same credential/key sources the Linux `ai-usagebar` uses, but
//! it is strictly read-only: it never refreshes OAuth tokens, so it can't log
//! you out of the `claude` / `codex` CLIs. Expired tokens are surfaced as a
//! "re-login" hint instead of being rotated.
//!
//! Architecture: a background thread polls every vendor on an interval and
//! ships the results to the main (UI) thread via the event loop proxy. The main
//! thread owns the tray icon + menu (Win32 requires this) and repaints on each
//! update.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod creds;
mod render;
mod tray;
mod usage;
mod vendors;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;

use chrono::Utc;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::config::Config;
use crate::render::Rendered;
use crate::usage::Severity;
use crate::vendors::{VendorId, VendorReport};

/// Events sent from the poll thread to the UI thread.
enum UserEvent {
    Update(Vec<VendorReport>),
}

fn main() {
    let cfg = Config::load();
    let primary = cfg.ui.primary.unwrap_or(VendorId::Anthropic);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Channel to ask the poll thread for an immediate refresh.
    let (refresh_tx, refresh_rx) = mpsc::channel::<()>();

    // Background poll thread.
    {
        let cfg = cfg.clone();
        let interval = cfg.poll_interval();
        thread::spawn(move || {
            let client = vendors::build_client();
            loop {
                let reports = vendors::fetch_all(&client, &cfg, Utc::now());
                if proxy.send_event(UserEvent::Update(reports)).is_err() {
                    break; // UI gone — stop polling.
                }
                match refresh_rx.recv_timeout(interval) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
    }

    let mut tray: Option<TrayIcon> = None;

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;

        // Drain menu clicks (Refresh / Quit).
        while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            if menu_event.id == "quit" {
                *control_flow = ControlFlow::Exit;
                return;
            }
            if menu_event.id == "refresh" {
                let _ = refresh_tx.send(());
            }
        }

        match event {
            Event::NewEvents(StartCause::Init) => {
                // Win32: build the tray on the UI thread once the loop is live.
                let menu = build_menu(&["Loading…".to_string()]);
                let mut builder = TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_tooltip("ai-usagebar — loading…");
                if let Some(icon) = tray::icon_for(Severity::Low) {
                    builder = builder.with_icon(icon);
                }
                match builder.build() {
                    Ok(t) => tray = Some(t),
                    Err(e) => {
                        eprintln!("failed to create tray icon: {e}");
                        *control_flow = ControlFlow::Exit;
                    }
                }
            }
            Event::UserEvent(UserEvent::Update(reports)) => {
                if let Some(tray) = tray.as_ref() {
                    apply(tray, render::render(&reports, primary, Utc::now()));
                }
            }
            _ => {}
        }
    });
}

fn apply(tray: &TrayIcon, r: Rendered) {
    if let Some(icon) = tray::icon_for(r.severity) {
        let _ = tray.set_icon(Some(icon));
    }
    let _ = tray.set_tooltip(Some(clamp_tooltip(&r.tooltip)));
    tray.set_menu(Some(Box::new(build_menu(&r.menu_lines))));
}

/// Win32 tray tooltips are length-limited (~127 chars). Trim defensively.
fn clamp_tooltip(s: &str) -> String {
    const MAX: usize = 120;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let mut out: String = s.chars().take(MAX - 1).collect();
    out.push('…');
    out
}

/// Build the context menu: info lines (disabled) + Refresh + Quit.
fn build_menu(lines: &[String]) -> Menu {
    let menu = Menu::new();
    for line in lines {
        if line.is_empty() {
            let _ = menu.append(&PredefinedMenuItem::separator());
        } else {
            // Disabled = shown as a non-clickable info row.
            let _ = menu.append(&MenuItem::with_id("noop", line, false, None));
        }
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("refresh", "Refresh now", true, None));
    let _ = menu.append(&MenuItem::with_id("quit", "Quit", true, None));
    menu
}
