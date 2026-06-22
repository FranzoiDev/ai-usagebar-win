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
//! tray icon plus two WebView windows — a frameless popup shown near the tray
//! (usage cards + progress bars) and a regular OS window for settings.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod creds;
mod render;
mod tray;
mod ui;
mod usage;
mod vendors;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};
use tray_icon::{MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wry::{WebView, WebViewBuilder};

use crate::config::Config;
use crate::usage::Severity;
use crate::vendors::{VendorId, VendorReport};

/// Popup width (logical px). Height is content-driven via a `resize` message.
const POPUP_W: f64 = 380.0;

/// One poll result handed from the poll thread to the UI thread.
struct UpdatePayload {
    cfg: Config,
    reports: Vec<VendorReport>,
}

/// Events sent to the UI thread.
enum UserEvent {
    Update(Box<UpdatePayload>),
    /// Raw JSON message from a WebView's `window.ipc.postMessage`.
    Ipc(String),
    /// A tray-icon click, forwarded so it wakes the (`Wait`) event loop.
    Tray(TrayIconEvent),
}

/// Physical anchor of the tray icon (position + size) for popup placement.
type Anchor = (PhysicalPosition<f64>, PhysicalSize<u32>);

fn main() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Forward tray-icon clicks into the event loop. With `ControlFlow::Wait`
    // the loop sleeps until an event arrives; tray clicks land on tray-icon's
    // own window, so without this the loop never wakes and nothing happens.
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
    let mut popup: Option<(Window, WebView)> = None;
    let mut settings: Option<(Window, WebView)> = None;
    let mut latest: Option<UpdatePayload> = None;
    let mut popup_visible = false;
    let mut anchor: Option<Anchor> = None;
    // Debounce: a tray click that blurs an open popup must not reopen it.
    let mut last_hidden: Option<Instant> = None;

    event_loop.run(move |event, target, control_flow| {
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
                    }
                }
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
                if popup_visible && let Some((_, wv)) = &popup {
                    push_popup(wv, &p);
                }
                latest = Some(p);
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
                    anchor = Some((rect.position, rect.size));
                    if popup_visible {
                        hide_popup(&popup, &mut popup_visible, &mut last_hidden);
                    } else if last_hidden
                        .map(|t| t.elapsed() < Duration::from_millis(300))
                        .unwrap_or(false)
                    {
                        // This click is the one that just blurred the popup.
                        last_hidden = None;
                    } else {
                        if popup.is_none() {
                            popup = create_popup(target, &proxy);
                        }
                        if let Some((w, wv)) = &popup {
                            position_popup(w, &anchor);
                            w.set_visible(true);
                            w.set_focus();
                            if let Some(p) = &latest {
                                push_popup(wv, p);
                            }
                            popup_visible = true;
                        }
                    }
                }
            }

            Event::UserEvent(UserEvent::Ipc(msg)) => {
                let v: serde_json::Value = serde_json::from_str(&msg).unwrap_or_default();
                match v.get("cmd").and_then(|c| c.as_str()).unwrap_or("") {
                    "refresh" => {
                        let _ = refresh_tx.send(());
                    }
                    "popupReady" => {
                        if let (Some((_, wv)), Some(p)) = (&popup, &latest) {
                            push_popup(wv, p);
                        }
                    }
                    "resize" => {
                        if let (Some(h), Some((w, _))) =
                            (v.get("h").and_then(|h| h.as_f64()), &popup)
                        {
                            w.set_inner_size(LogicalSize::new(POPUP_W, h.clamp(80.0, 760.0)));
                            position_popup(w, &anchor);
                        }
                    }
                    "hide" => hide_popup(&popup, &mut popup_visible, &mut last_hidden),
                    "settings" => {
                        hide_popup(&popup, &mut popup_visible, &mut last_hidden);
                        if settings.is_none() {
                            settings = create_settings(target, &proxy);
                        }
                        if let Some((w, _)) = &settings {
                            w.set_visible(true);
                            w.set_focus();
                        }
                        push_settings_now(&settings, &latest);
                    }
                    "settingsReady" => push_settings_now(&settings, &latest),
                    "closeSettings" => {
                        if let Some((w, _)) = &settings {
                            w.set_visible(false);
                        }
                    }
                    "save" => {
                        if let Some(cfg_val) = v.get("config") {
                            match serde_json::from_value::<Config>(cfg_val.clone()) {
                                Ok(cfg) => {
                                    let cfg = cfg.sanitized();
                                    if let Err(e) = cfg.save() {
                                        eprintln!("failed to save config: {e}");
                                    }
                                    // Reflect new "configured" state immediately.
                                    if let Some((_, swv)) = &settings {
                                        let reports = latest
                                            .as_ref()
                                            .map(|p| p.reports.clone())
                                            .unwrap_or_default();
                                        let model = render::settings_model(&cfg, &reports);
                                        if let Ok(json) = serde_json::to_string(&model) {
                                            let _ = swv.evaluate_script(&format!(
                                                "window.__config && window.__config({json})"
                                            ));
                                        }
                                    }
                                    let _ = refresh_tx.send(()); // repoll with new config
                                }
                                Err(e) => eprintln!("invalid settings payload: {e}"),
                            }
                        }
                    }
                    "quit" => {
                        *control_flow = ControlFlow::Exit;
                    }
                    _ => {}
                }
            }

            Event::WindowEvent { window_id, event, .. } => {
                if let Some((w, _)) = &popup
                    && window_id == w.id()
                    && matches!(event, WindowEvent::Focused(false))
                {
                    hide_popup(&popup, &mut popup_visible, &mut last_hidden);
                }
                if let Some((w, _)) = &settings
                    && window_id == w.id()
                    && matches!(event, WindowEvent::CloseRequested)
                {
                    w.set_visible(false);
                }
            }

            _ => {}
        }
    });
}

fn primary_of(cfg: &Config) -> VendorId {
    cfg.ui.primary.unwrap_or(VendorId::Anthropic)
}

fn hide_popup(
    popup: &Option<(Window, WebView)>,
    visible: &mut bool,
    last_hidden: &mut Option<Instant>,
) {
    if let Some((w, _)) = popup {
        w.set_visible(false);
    }
    if *visible {
        *last_hidden = Some(Instant::now());
    }
    *visible = false;
}

fn push_popup(wv: &WebView, p: &UpdatePayload) {
    let model = render::popup_model(&p.reports, &p.cfg, primary_of(&p.cfg), Utc::now());
    if let Ok(json) = serde_json::to_string(&model) {
        let _ = wv.evaluate_script(&format!("window.__data && window.__data({json})"));
    }
}

fn push_settings_now(settings: &Option<(Window, WebView)>, latest: &Option<UpdatePayload>) {
    let Some((_, wv)) = settings else { return };
    let (cfg, reports) = match latest {
        Some(p) => (p.cfg.clone(), p.reports.clone()),
        None => (Config::load(), Vec::new()),
    };
    let model = render::settings_model(&cfg, &reports);
    if let Ok(json) = serde_json::to_string(&model) {
        let _ = wv.evaluate_script(&format!("window.__config && window.__config({json})"));
    }
}

/// Place the popup centered above the tray icon, clamped to the monitor.
fn position_popup(w: &Window, anchor: &Option<Anchor>) {
    let Some((pos, isz)) = anchor else { return };
    let size = w.outer_size();
    let margin = 10.0;
    let mut x = pos.x + isz.width as f64 / 2.0 - size.width as f64 / 2.0;
    let mut y = pos.y - size.height as f64 - margin;
    if let Some(mon) = w.current_monitor() {
        let ms = mon.size();
        let mp = mon.position();
        let max_x = mp.x as f64 + ms.width as f64 - size.width as f64 - margin;
        x = x.min(max_x);
        // Tray at the top of the screen → drop the popup below the icon.
        if y < mp.y as f64 + margin {
            y = pos.y + isz.height as f64 + margin;
        }
    }
    w.set_outer_position(PhysicalPosition::new(x.max(10.0), y.max(10.0)));
}

fn create_popup(
    target: &EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
) -> Option<(Window, WebView)> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut builder = WindowBuilder::new()
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top(true)
        .with_transparent(true)
        .with_visible(false)
        .with_inner_size(LogicalSize::new(POPUP_W, 200.0));
    #[cfg(windows)]
    {
        use tao::platform::windows::WindowBuilderExtWindows;
        builder = builder.with_skip_taskbar(true).with_undecorated_shadow(true);
    }
    let window = match builder.build(target) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("failed to create popup window: {e}");
            return None;
        }
    };
    let webview = match webview_builder(proxy)
        .with_transparent(true)
        .with_html(ui::POPUP_HTML)
        .build(&window)
    {
        Ok(wv) => wv,
        Err(e) => {
            eprintln!("failed to create popup webview: {e}");
            return None;
        }
    };
    Some((window, webview))
}

fn create_settings(
    target: &EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
) -> Option<(Window, WebView)> {
    let window = match WindowBuilder::new()
        .with_title("AI Usage — Settings")
        .with_inner_size(LogicalSize::new(560.0, 640.0))
        .with_min_inner_size(LogicalSize::new(460.0, 420.0))
        .with_visible(false)
        .build(target)
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("failed to create settings window: {e}");
            return None;
        }
    };
    let webview = match webview_builder(proxy)
        .with_html(ui::SETTINGS_HTML)
        .build(&window)
    {
        Ok(wv) => wv,
        Err(e) => {
            eprintln!("failed to create settings webview: {e}");
            return None;
        }
    };
    Some((window, webview))
}

/// A WebViewBuilder whose IPC handler forwards messages to the event loop.
fn webview_builder<'a>(proxy: &EventLoopProxy<UserEvent>) -> WebViewBuilder<'a> {
    let proxy = proxy.clone();
    WebViewBuilder::new().with_ipc_handler(move |req| {
        let _ = proxy.send_event(UserEvent::Ipc(req.into_body()));
    })
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
