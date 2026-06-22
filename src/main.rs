//! ai-usagebar-win — a native Windows notification-area (tray) app that shows
//! how much of your AI plans you've used.
//!
//! It reads the same credential/key sources the Linux `ai-usagebar` uses, but
//! it is strictly read-only: it never refreshes OAuth tokens, so it can't log
//! you out of the `claude` / `codex` CLIs. Expired tokens are surfaced as a
//! "re-login" hint instead of being rotated.
//!
//! UI: a background thread polls every vendor on an interval and ships results
//! to the main (UI) thread via the event-loop proxy. The main thread owns the
//! tray icon; the popup and settings windows are drawn with raw Win32 controls
//! (native progress bars, buttons) — see `winui_win.rs`.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod creds;
mod render;
mod tray;
mod usage;
mod vendors;

#[cfg(windows)]
#[path = "winui_win.rs"]
mod winui;
#[cfg(not(windows))]
#[path = "winui_stub.rs"]
mod winui;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;

use chrono::Utc;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::{MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::config::Config;
use crate::usage::Severity;
use crate::vendors::{VendorId, VendorReport};

/// One poll result handed from the poll thread to the UI thread.
pub struct UpdatePayload {
    pub cfg: Config,
    pub reports: Vec<VendorReport>,
}

/// Events delivered to the UI thread via the event-loop proxy.
pub enum UserEvent {
    Update(Box<UpdatePayload>),
    /// A tray-icon click, forwarded so it wakes the (`Wait`) event loop.
    Tray(TrayIconEvent),
    /// The native Quit button was pressed.
    Quit,
}

fn main() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Forward tray-icon clicks into the event loop. With `ControlFlow::Wait`
    // the loop sleeps until an event arrives; tray clicks land on tray-icon's
    // own window, so without this the loop never wakes.
    {
        let proxy = proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(UserEvent::Tray(event));
        }));
    }

    // Channel to ask the poll thread for an immediate refresh.
    let (refresh_tx, refresh_rx) = mpsc::channel::<()>();

    // Background poll thread. Reloads config each cycle so settings changes
    // (and the resulting refresh ping) take effect without a restart.
    {
        let proxy = proxy.clone();
        thread::spawn(move || {
            let client = vendors::build_client();
            loop {
                let cfg = Config::load();
                let reports = vendors::fetch_all(&client, &cfg, Utc::now());
                let interval = cfg.poll_interval();
                let payload = Box::new(UpdatePayload { cfg, reports });
                if proxy.send_event(UserEvent::Update(payload)).is_err() {
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

        match event {
            Event::NewEvents(StartCause::Init) => {
                let mut builder = TrayIconBuilder::new().with_tooltip("ai-usagebar — loading…");
                if let Some(icon) = tray::icon_for(Severity::Low) {
                    builder = builder.with_icon(icon);
                }
                match builder.build() {
                    Ok(t) => tray = Some(t),
                    Err(e) => {
                        eprintln!("failed to create tray icon: {e}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                }
                winui::init(proxy.clone(), refresh_tx.clone());
            }

            Event::UserEvent(UserEvent::Update(payload)) => {
                let p = *payload;
                if let Some(tray) = tray.as_ref() {
                    let r = render::render(&p.reports, &p.cfg, primary_of(&p.cfg), Utc::now());
                    if let Some(icon) = tray::icon_for(r.severity) {
                        let _ = tray.set_icon(Some(icon));
                    }
                    let _ = tray.set_tooltip(Some(clamp_tooltip(&r.tooltip)));
                }
                winui::set_data(p);
            }

            Event::UserEvent(UserEvent::Tray(ev)) => {
                // Any click (left or right — there's no context menu) toggles
                // the popup. React on button-up so it fires once per click.
                let rect = match ev {
                    TrayIconEvent::Click {
                        rect,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                    | TrayIconEvent::DoubleClick { rect, .. } => Some(rect),
                    _ => None,
                };
                if let Some(rect) = rect {
                    winui::toggle_popup(
                        rect.position.x as i32,
                        rect.position.y as i32,
                        rect.size.width as i32,
                        rect.size.height as i32,
                    );
                }
            }

            Event::UserEvent(UserEvent::Quit) => {
                *control_flow = ControlFlow::Exit;
            }

            _ => {}
        }
    });
}

fn primary_of(cfg: &Config) -> VendorId {
    cfg.ui.primary.unwrap_or(VendorId::Anthropic)
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
