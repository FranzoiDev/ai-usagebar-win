//! Native Windows UI drawn with raw Win32 controls (windows-sys): a frameless
//! popup shown near the tray (native progress bars + owner-drawn buttons) and a
//! decorated settings window. No web engine — the visible UI is OS-native.
//!
//! Threading: every function here runs on the UI thread. State lives in a
//! `thread_local` `Ui`. The golden rule: never hold the `UI` borrow across a
//! call that can re-enter our window procs (ShowWindow / SetForegroundWindow /
//! CreateWindow of a *visible child of a visible parent*). We snapshot the
//! handles we need, drop the borrow, then act.

// This module is a thin layer over raw Win32: several ABI constants are kept for
// completeness, and the control factories naturally take many geometry args.
#![allow(dead_code, clippy::too_many_arguments, clippy::missing_safety_doc)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use tao::event_loop::EventLoopProxy;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, DrawTextW, FillRect, GetStockObject, RoundRect, SelectObject,
    SetBkColor, SetBkMode, SetTextColor, HBRUSH, HDC, HFONT, HGDIOBJ,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    InitCommonControlsEx, SetWindowTheme, DRAWITEMSTRUCT, INITCOMMONCONTROLSEX,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, LoadCursorW, RegisterClassExW,
    SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, WNDCLASSEXW,
};

use crate::config::Config;
use crate::render::{self, VendorCard};
use crate::vendors::VendorId;
use crate::{UpdatePayload, UserEvent};

// ---------------------------------------------------------------------------
// Palette (COLORREF = 0x00BBGGRR).
// ---------------------------------------------------------------------------

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

const BG: u32 = rgb(0x16, 0x17, 0x1d);
const CARD: u32 = rgb(0x26, 0x29, 0x34);
const LINE: u32 = rgb(0x2e, 0x31, 0x40);
const TEXT: u32 = rgb(0xe7, 0xe9, 0xf0);
const MUTED: u32 = rgb(0x9a, 0xa0, 0xb4);
const ACCENT: u32 = rgb(0x6d, 0x8b, 0xff);
const TRACK: u32 = rgb(0x33, 0x37, 0x45);
const QUIT_RED: u32 = rgb(0xff, 0x8a, 0x80);

fn bar_color(level: &str) -> u32 {
    match level {
        "mid" => rgb(0xff, 0xc1, 0x07),
        "high" => rgb(0xff, 0x98, 0x00),
        "critical" => rgb(0xf4, 0x43, 0x36),
        _ => rgb(0x4c, 0xaf, 0x50),
    }
}

// ---------------------------------------------------------------------------
// Win32 constants (ABI-stable values; defined locally to avoid relying on
// exact windows-sys const names).
// ---------------------------------------------------------------------------

const WS_POPUP: u32 = 0x8000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_CLIPCHILDREN: u32 = 0x0200_0000;

const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_TOPMOST: u32 = 0x0000_0008;

const SS_LEFT: u32 = 0x0000;
const SS_RIGHT: u32 = 0x0002;

const BS_OWNERDRAW: u32 = 0x0000_000B;
const BS_AUTOCHECKBOX: u32 = 0x0000_0003;

const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_PASSWORD: u32 = 0x0020;
const ES_NUMBER: u32 = 0x2000;

const CBS_DROPDOWNLIST: u32 = 0x0003;
const CBS_HASSTRINGS: u32 = 0x0200;

const PBS_SMOOTH: u32 = 0x01;

const CS_VREDRAW: u32 = 0x0001;
const CS_HREDRAW: u32 = 0x0002;

const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_SETFONT: u32 = 0x0030;
const WM_ACTIVATE: u32 = 0x0006;
const WM_COMMAND: u32 = 0x0111;
const WM_CTLCOLOREDIT: u32 = 0x0133;
const WM_CTLCOLORLISTBOX: u32 = 0x0134;
const WM_CTLCOLORBTN: u32 = 0x0135;
const WM_CTLCOLORSTATIC: u32 = 0x0138;
const WM_DRAWITEM: u32 = 0x002B;

const WA_INACTIVE: usize = 0;

const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;

const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOSIZE: u32 = 0x0001;

const GWLP_USERDATA: i32 = -21;

const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;

const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETCURSEL: u32 = 0x0147;

const PBM_SETPOS: u32 = 0x0402;
const PBM_SETRANGE32: u32 = 0x0406;
const PBM_SETBARCOLOR: u32 = 0x0409;
const PBM_SETBKCOLOR: u32 = 0x2001;

const DT_CENTER: u32 = 0x0001;
const DT_RIGHT: u32 = 0x0002;
const DT_VCENTER: u32 = 0x0004;
const DT_SINGLELINE: u32 = 0x0020;
const DT_WORDBREAK: u32 = 0x0010;
const DT_END_ELLIPSIS: u32 = 0x8000;

const TRANSPARENT: i32 = 1;
const OPAQUE: i32 = 2;
const NULL_PEN: i32 = 8;

const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_ROUND: i32 = 2;

const ICC_PROGRESS_CLASS: u32 = 0x20;
const ICC_STANDARD_CLASSES: u32 = 0x4000;

const FW_NORMAL: i32 = 400;
const FW_SEMIBOLD: i32 = 600;
const DEFAULT_CHARSET: u32 = 1;
const CLEARTYPE_QUALITY: u32 = 5;
const IDC_ARROW: u32 = 32512;
const ODS_SELECTED: u32 = 0x0001;

// Control IDs (WM_COMMAND / WM_DRAWITEM).
const ID_REFRESH: usize = 101;
const ID_SETTINGS: usize = 102;
const ID_QUIT: usize = 103;
const ID_SAVE: usize = 201;
const ID_CLOSE: usize = 202;

const POPUP_CLASS: &str = "AiUsagePopupWnd";
const SETTINGS_CLASS: &str = "AiUsageSettingsWnd";

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Res {
    hinst: HWND,
    font: HFONT,
    font_sb: HFONT,
    font_sm: HFONT,
    scale: f32,
}

struct VendorRow {
    id: VendorId,
    enable: HWND,
    env: Option<HWND>,
    key: Option<HWND>,
    tier: Option<HWND>,
}

struct SettingsFields {
    poll: HWND,
    primary: HWND,
    vendors: Vec<VendorRow>,
}

struct Ui {
    res: Res,
    bg_brush: HBRUSH,
    card_brush: HBRUSH,
    popup: HWND,
    popup_children: Vec<HWND>,
    popup_visible: bool,
    last_shown: Option<Instant>,
    last_hidden: Option<Instant>,
    anchor: Option<(i32, i32, i32, i32)>,
    settings: HWND,
    settings_children: Vec<HWND>,
    fields: Option<SettingsFields>,
    latest: Option<UpdatePayload>,
    refresh: Sender<()>,
    proxy: EventLoopProxy<UserEvent>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn ui_do<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with_borrow_mut(|o| o.as_mut().map(f))
}

// ---------------------------------------------------------------------------
// Public API (called from the tao event loop in main.rs).
// ---------------------------------------------------------------------------

pub fn init(proxy: EventLoopProxy<UserEvent>, refresh: Sender<()>) {
    unsafe {
        let hinst: HWND = GetModuleHandleW(core::ptr::null());

        let icc = INITCOMMONCONTROLSEX {
            dwSize: core::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_PROGRESS_CLASS | ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&icc);

        let scale = {
            let dpi = GetDpiForSystem();
            if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
        };

        register_class(hinst, POPUP_CLASS, Some(popup_proc));
        register_class(hinst, SETTINGS_CLASS, Some(settings_proc));

        let res = Res {
            hinst,
            font: make_font(scale, 15, FW_NORMAL),
            font_sb: make_font(scale, 15, FW_SEMIBOLD),
            font_sm: make_font(scale, 13, FW_NORMAL),
            scale,
        };
        let bg_brush = CreateSolidBrush(BG);
        let card_brush = CreateSolidBrush(CARD);

        let popup = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            wide(POPUP_CLASS).as_ptr(),
            wide("").as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            px(scale, 360),
            px(scale, 200),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            hinst,
            core::ptr::null(),
        );
        dwm_dark(popup);
        dwm_round(popup);

        let settings = CreateWindowExW(
            0,
            wide(SETTINGS_CLASS).as_ptr(),
            wide("AI Usage — Settings").as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
            i32::MIN, // CW_USEDEFAULT
            i32::MIN,
            px(scale, 580),
            px(scale, 780),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            hinst,
            core::ptr::null(),
        );
        dwm_dark(settings);

        UI.with_borrow_mut(|o| {
            *o = Some(Ui {
                res,
                bg_brush,
                card_brush,
                popup,
                popup_children: Vec::new(),
                popup_visible: false,
                last_shown: None,
                last_hidden: None,
                anchor: None,
                settings,
                settings_children: Vec::new(),
                fields: None,
                latest: None,
                refresh,
                proxy,
            });
        });
    }
}

pub fn set_data(payload: UpdatePayload) {
    let visible = ui_do(|ui| {
        ui.latest = Some(payload);
        ui.popup_visible
    })
    .unwrap_or(false);
    if visible {
        show_popup(false); // rebuild in place, don't steal focus
    }
}

pub fn toggle_popup(x: i32, y: i32, w: i32, h: i32) {
    let action = ui_do(|ui| {
        ui.anchor = Some((x, y, w, h));
        if ui.popup_visible {
            1 // hide
        } else if ui
            .last_hidden
            .map(|t| t.elapsed() < Duration::from_millis(300))
            .unwrap_or(false)
        {
            ui.last_hidden = None;
            0 // the click that just dismissed it — do nothing
        } else {
            2 // show
        }
    })
    .unwrap_or(0);
    match action {
        1 => hide_popup(),
        2 => show_popup(true),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Popup.
// ---------------------------------------------------------------------------

fn show_popup(activate: bool) {
    // Snapshot what we need, then build controls and show — all without
    // holding the UI borrow (CreateWindow/ShowWindow re-enter popup_proc).
    let Some((popup, res, old_children, cards)) = ui_do(|ui| {
        let cards = ui
            .latest
            .as_ref()
            .map(|p| render::popup_model(&p.reports, &p.cfg, primary_of(&p.cfg), chrono::Utc::now()).vendors)
            .unwrap_or_default();
        (
            ui.popup,
            ui.res,
            std::mem::take(&mut ui.popup_children),
            cards,
        )
    }) else {
        return;
    };

    unsafe {
        for h in old_children {
            DestroyWindow(h);
        }
        let (children, w, h) = build_popup_controls(popup, res, &cards);
        let (x, y) = ui_do(|ui| compute_pos(ui.anchor, w, h)).unwrap_or((0, 0));

        SetWindowPos(popup, hwnd_topmost(), x, y, w, h, SWP_NOACTIVATE);
        ShowWindow(popup, SW_SHOW);
        if activate {
            SetForegroundWindow(popup);
        }

        ui_do(|ui| {
            ui.popup_children = children;
            ui.popup_visible = true;
            if activate {
                ui.last_shown = Some(Instant::now());
            }
        });
    }
}

fn hide_popup() {
    let popup = ui_do(|ui| ui.popup);
    if let Some(popup) = popup {
        unsafe { ShowWindow(popup, SW_HIDE) };
    }
    ui_do(|ui| {
        if ui.popup_visible {
            ui.last_hidden = Some(Instant::now());
        }
        ui.popup_visible = false;
    });
}

/// Build the popup's child controls top-to-bottom; return them plus the window
/// size needed to fit. Runs with NO UI borrow held.
unsafe fn build_popup_controls(parent: HWND, res: Res, cards: &[VendorCard]) -> (Vec<HWND>, i32, i32) {
    let mut kids = Vec::new();
    let pad = px(res.scale, 14);
    let w = px(res.scale, 360);
    let inner = w - pad * 2;
    let mut y = pad;

    if cards.is_empty() {
        kids.push(mk_static(
            parent,
            res,
            "No models configured.\nOpen Settings to add an API key.",
            pad,
            y,
            inner,
            px(res.scale, 44),
            res.font,
            MUTED,
            false,
        ));
        y += px(res.scale, 52);
    }

    for (i, card) in cards.iter().enumerate() {
        if i > 0 {
            y += px(res.scale, 6);
        }
        // Vendor name + plan (right).
        kids.push(mk_static(parent, res, &card.name, pad, y, inner - px(res.scale, 120), px(res.scale, 20), res.font_sb, TEXT, false));
        if let Some(plan) = &card.plan {
            kids.push(mk_static(parent, res, plan, pad + inner - px(res.scale, 120), y, px(res.scale, 120), px(res.scale, 20), res.font_sm, MUTED, true));
        }
        y += px(res.scale, 24);

        if let Some(msg) = &card.message {
            kids.push(mk_static(parent, res, msg, pad, y, inner, px(res.scale, 18), res.font_sm, QUIT_RED, false));
            y += px(res.scale, 22);
        }

        for bar in &card.bars {
            kids.push(mk_static(parent, res, &bar.label, pad, y, inner / 2, px(res.scale, 16), res.font_sm, MUTED, false));
            let value = match &bar.reset {
                Some(r) => format!("{}%  ·  {}", bar.pct, r),
                None => format!("{}%", bar.pct),
            };
            kids.push(mk_static(parent, res, &value, pad + inner / 2, y, inner / 2, px(res.scale, 16), res.font_sm, MUTED, true));
            y += px(res.scale, 18);
            kids.push(mk_progress(parent, res, pad, y, inner, px(res.scale, 8), bar.pct, &bar.level));
            y += px(res.scale, 8) + px(res.scale, 10);
        }

        for fact in &card.facts {
            let text = format!("{}: {}", fact.label, fact.value);
            kids.push(mk_static(parent, res, &text, pad, y, inner, px(res.scale, 16), res.font_sm, MUTED, false));
            y += px(res.scale, 18);
        }
    }

    // Footer buttons.
    y += px(res.scale, 8);
    let btn_h = px(res.scale, 34);
    let gap = px(res.scale, 8);
    let refresh_w = px(res.scale, 42);
    let rest = inner - refresh_w - gap * 2;
    let half = rest / 2;
    let mut x = pad;
    kids.push(mk_button(parent, res, "⟳", x, y, refresh_w, btn_h, ID_REFRESH));
    x += refresh_w + gap;
    kids.push(mk_button(parent, res, "⚙  Settings", x, y, half, btn_h, ID_SETTINGS));
    x += half + gap;
    kids.push(mk_button(parent, res, "⏻  Quit", x, y, rest - half, btn_h, ID_QUIT));
    y += btn_h + pad;

    (kids, w, y)
}

fn compute_pos(anchor: Option<(i32, i32, i32, i32)>, w: i32, h: i32) -> (i32, i32) {
    let Some((ax, ay, aw, ah)) = anchor else {
        return (100, 100);
    };
    let margin = 8;
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let mut x = ax + aw / 2 - w / 2;
    let mut y = ay - h - margin;
    if x + w + margin > sw {
        x = sw - w - margin;
    }
    if x < margin {
        x = margin;
    }
    if y < margin {
        y = (ay + ah + margin).min(sh - h - margin);
    }
    (x, y)
}

// ---------------------------------------------------------------------------
// Settings.
// ---------------------------------------------------------------------------

fn show_settings() {
    let Some((settings, res, old_children, cfg, reports)) = ui_do(|ui| {
        let (cfg, reports) = match &ui.latest {
            Some(p) => (p.cfg.clone(), p.reports.clone()),
            None => (Config::load(), Vec::new()),
        };
        (
            ui.settings,
            ui.res,
            std::mem::take(&mut ui.settings_children),
            cfg,
            reports,
        )
    }) else {
        return;
    };

    unsafe {
        for h in old_children {
            DestroyWindow(h);
        }
        let model = render::settings_model(&cfg, &reports);
        let (children, fields) = build_settings_controls(settings, res, &model);

        ui_do(|ui| {
            ui.settings_children = children;
            ui.fields = Some(fields);
        });

        ShowWindow(settings, SW_SHOW);
        SetForegroundWindow(settings);
    }
}

unsafe fn build_settings_controls(
    parent: HWND,
    res: Res,
    model: &render::SettingsModel,
) -> (Vec<HWND>, SettingsFields) {
    let mut kids = Vec::new();
    let pad = px(res.scale, 18);
    let w = px(res.scale, 580) - pad * 2;
    let mut y = pad;
    let lh = px(res.scale, 16);
    let field_h = px(res.scale, 26);

    // Poll interval.
    kids.push(mk_static(parent, res, "Refresh interval (seconds)", pad, y, w, lh, res.font_sm, MUTED, false));
    y += lh + px(res.scale, 4);
    let poll = mk_edit(parent, res, &model.poll_seconds.to_string(), pad, y, px(res.scale, 120), field_h, ES_NUMBER);
    kids.push(poll);
    y += field_h + px(res.scale, 12);

    // Primary.
    kids.push(mk_static(parent, res, "Primary (tray tooltip)", pad, y, w, lh, res.font_sm, MUTED, false));
    y += lh + px(res.scale, 4);
    let primary = mk_combo(parent, res, pad, y, px(res.scale, 220), px(res.scale, 220));
    kids.push(primary);
    for (i, v) in model.vendors.iter().enumerate() {
        SendMessageW(primary, CB_ADDSTRING, 0, wide(&v.name).as_ptr() as LPARAM);
        if v.id == model.primary {
            SendMessageW(primary, CB_SETCURSEL, i, 0);
        }
    }
    y += field_h + px(res.scale, 16);

    // Vendors.
    let mut vendors = Vec::new();
    for v in &model.vendors {
        let id = parse_vendor(&v.id);
        let label = format!("{}{}", v.name, if v.configured { "   ✓ configured" } else { "" });
        let cb = mk_checkbox(parent, res, &label, pad, y, w, px(res.scale, 22), v.enabled);
        kids.push(cb);
        y += px(res.scale, 26);

        if let Some(status) = &v.status {
            kids.push(mk_static(parent, res, status, pad + px(res.scale, 12), y, w - px(res.scale, 12), lh, res.font_sm, MUTED, false));
            y += lh + px(res.scale, 4);
        }

        let mut row = VendorRow { id, enable: cb, env: None, key: None, tier: None };

        if v.kind == "apikey" {
            kids.push(mk_static(parent, res, "Environment variable", pad + px(res.scale, 12), y, w, lh, res.font_sm, MUTED, false));
            y += lh + px(res.scale, 2);
            let env = mk_edit(parent, res, v.api_key_env.as_deref().unwrap_or(""), pad + px(res.scale, 12), y, px(res.scale, 240), field_h, 0);
            kids.push(env);
            row.env = Some(env);
            y += field_h + px(res.scale, 6);

            kids.push(mk_static(parent, res, "API key (optional — overrides env var)", pad + px(res.scale, 12), y, w, lh, res.font_sm, MUTED, false));
            y += lh + px(res.scale, 2);
            let key = mk_edit(parent, res, v.api_key.as_deref().unwrap_or(""), pad + px(res.scale, 12), y, w - px(res.scale, 24), field_h, ES_PASSWORD);
            kids.push(key);
            row.key = Some(key);
            y += field_h + px(res.scale, 6);

            if v.id == "zai" {
                kids.push(mk_static(parent, res, "Plan tier (display only)", pad + px(res.scale, 12), y, w, lh, res.font_sm, MUTED, false));
                y += lh + px(res.scale, 2);
                let tier = mk_edit(parent, res, v.plan_tier.as_deref().unwrap_or(""), pad + px(res.scale, 12), y, px(res.scale, 160), field_h, 0);
                kids.push(tier);
                row.tier = Some(tier);
                y += field_h + px(res.scale, 6);
            }
        } else if let Some(hint) = &v.hint {
            kids.push(mk_static(parent, res, hint, pad + px(res.scale, 12), y, w - px(res.scale, 12), px(res.scale, 36), res.font_sm, MUTED, false));
            y += px(res.scale, 40);
        }

        y += px(res.scale, 10);
        vendors.push(row);
    }

    // Footer buttons.
    y += px(res.scale, 4);
    let btn_h = px(res.scale, 34);
    let bw = px(res.scale, 110);
    let gap = px(res.scale, 10);
    kids.push(mk_button(parent, res, "Close", pad + w - bw * 2 - gap, y, bw, btn_h, ID_CLOSE));
    kids.push(mk_button(parent, res, "Save", pad + w - bw, y, bw, btn_h, ID_SAVE));

    (kids, SettingsFields { poll, primary, vendors })
}

fn save_settings() {
    let Some((fields, refresh)) = ui_do(|ui| {
        let f = ui.fields.as_ref().map(|f| SettingsFields {
            poll: f.poll,
            primary: f.primary,
            vendors: f.vendors.iter().map(|r| VendorRow { id: r.id, enable: r.enable, env: r.env, key: r.key, tier: r.tier }).collect(),
        });
        (f, ui.refresh.clone())
    }) else {
        return;
    };
    let Some(fields) = fields else { return };

    let mut cfg = Config::default();
    unsafe {
        cfg.poll_seconds = Some(get_text(fields.poll).trim().parse::<u64>().unwrap_or(60));
        let idx = SendMessageW(fields.primary, CB_GETCURSEL, 0, 0);
        let primary = VendorId::ALL.get(idx.max(0) as usize).copied().unwrap_or(VendorId::Anthropic);
        cfg.ui.primary = Some(primary);

        for row in &fields.vendors {
            let enabled = SendMessageW(row.enable, BM_GETCHECK, 0, 0) as usize == BST_CHECKED;
            let env = row.env.map(|h| get_text(h));
            let key = row.key.map(|h| get_text(h)).filter(|s| !s.is_empty());
            let tier = row.tier.map(|h| get_text(h)).filter(|s| !s.is_empty());
            match row.id {
                VendorId::Anthropic => cfg.anthropic.enabled = enabled,
                VendorId::Openai => cfg.openai.enabled = enabled,
                VendorId::Zai => {
                    cfg.zai.enabled = enabled;
                    if let Some(e) = env { cfg.zai.api_key_env = e; }
                    cfg.zai.api_key = key;
                    cfg.zai.plan_tier = tier;
                }
                VendorId::Openrouter => {
                    cfg.openrouter.enabled = enabled;
                    if let Some(e) = env { cfg.openrouter.api_key_env = e; }
                    cfg.openrouter.api_key = key;
                }
                VendorId::Deepseek => {
                    cfg.deepseek.enabled = enabled;
                    if let Some(e) = env { cfg.deepseek.api_key_env = e; }
                    cfg.deepseek.api_key = key;
                }
            }
        }
    }

    if let Err(e) = cfg.sanitized().save() {
        eprintln!("failed to save config: {e}");
    }
    let _ = refresh.send(());
    // Rebuild settings so "configured" badges/status refresh after the poll.
    show_settings();
}

// ---------------------------------------------------------------------------
// Control factories.
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
unsafe fn mk_static(parent: HWND, res: Res, text: &str, x: i32, y: i32, w: i32, h: i32, font: HFONT, color: u32, right: bool) -> HWND {
    let style = WS_CHILD | WS_VISIBLE | if right { SS_RIGHT } else { SS_LEFT };
    let hw = CreateWindowExW(0, wide("STATIC").as_ptr(), wide(text).as_ptr(), style, x, y, w, h, parent, core::ptr::null_mut(), res.hinst, core::ptr::null());
    SendMessageW(hw, WM_SETFONT, font as usize, 1);
    SetWindowLongPtrW(hw, GWLP_USERDATA, color as isize);
    hw
}

unsafe fn mk_progress(parent: HWND, res: Res, x: i32, y: i32, w: i32, h: i32, pct: i32, level: &str) -> HWND {
    let hw = CreateWindowExW(0, wide("msctls_progress32").as_ptr(), wide("").as_ptr(), WS_CHILD | WS_VISIBLE | PBS_SMOOTH, x, y, w, h, parent, core::ptr::null_mut(), res.hinst, core::ptr::null());
    // Drop the theme so the custom bar/track colors take effect.
    SetWindowTheme(hw, wide("").as_ptr(), wide("").as_ptr());
    SendMessageW(hw, PBM_SETRANGE32, 0, 100);
    SendMessageW(hw, PBM_SETBARCOLOR, 0, bar_color(level) as LPARAM);
    SendMessageW(hw, PBM_SETBKCOLOR, 0, TRACK as LPARAM);
    SendMessageW(hw, PBM_SETPOS, pct.clamp(0, 100) as usize, 0);
    hw
}

unsafe fn mk_button(parent: HWND, res: Res, text: &str, x: i32, y: i32, w: i32, h: i32, id: usize) -> HWND {
    let hw = CreateWindowExW(0, wide("BUTTON").as_ptr(), wide(text).as_ptr(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW, x, y, w, h, parent, id as HWND, res.hinst, core::ptr::null());
    SendMessageW(hw, WM_SETFONT, res.font as usize, 1);
    hw
}

unsafe fn mk_edit(parent: HWND, res: Res, text: &str, x: i32, y: i32, w: i32, h: i32, extra: u32) -> HWND {
    let hw = CreateWindowExW(0, wide("EDIT").as_ptr(), wide(text).as_ptr(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL | extra, x, y, w, h, parent, core::ptr::null_mut(), res.hinst, core::ptr::null());
    SendMessageW(hw, WM_SETFONT, res.font as usize, 1);
    SetWindowTheme(hw, wide("DarkMode_CFD").as_ptr(), core::ptr::null());
    hw
}

unsafe fn mk_combo(parent: HWND, res: Res, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let hw = CreateWindowExW(0, wide("COMBOBOX").as_ptr(), wide("").as_ptr(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST | CBS_HASSTRINGS, x, y, w, h, parent, core::ptr::null_mut(), res.hinst, core::ptr::null());
    SendMessageW(hw, WM_SETFONT, res.font as usize, 1);
    SetWindowTheme(hw, wide("DarkMode_CFD").as_ptr(), core::ptr::null());
    hw
}

unsafe fn mk_checkbox(parent: HWND, res: Res, text: &str, x: i32, y: i32, w: i32, h: i32, checked: bool) -> HWND {
    let hw = CreateWindowExW(0, wide("BUTTON").as_ptr(), wide(text).as_ptr(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX, x, y, w, h, parent, core::ptr::null_mut(), res.hinst, core::ptr::null());
    SendMessageW(hw, WM_SETFONT, res.font_sb as usize, 1);
    SetWindowTheme(hw, wide("DarkMode_Explorer").as_ptr(), core::ptr::null());
    SetWindowLongPtrW(hw, GWLP_USERDATA, TEXT as isize);
    SendMessageW(hw, BM_SETCHECK, if checked { BST_CHECKED } else { 0 }, 0);
    hw
}

// ---------------------------------------------------------------------------
// Window procedures.
// ---------------------------------------------------------------------------

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            fill_bg(hwnd, wp as HDC);
            1
        }
        WM_CTLCOLORSTATIC => ctlcolor_static(wp as HDC, lp as HWND),
        WM_DRAWITEM => {
            draw_button(&*(lp as *const DRAWITEMSTRUCT));
            1
        }
        WM_COMMAND => {
            handle_command(wp & 0xFFFF);
            0
        }
        WM_ACTIVATE => {
            if wp & 0xFFFF == WA_INACTIVE {
                let grace = ui_do(|ui| {
                    ui.last_shown
                        .map(|t| t.elapsed() < Duration::from_millis(400))
                        .unwrap_or(false)
                })
                .unwrap_or(false);
                if !grace {
                    hide_popup();
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            fill_bg(hwnd, wp as HDC);
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => ctlcolor_static(wp as HDC, lp as HWND),
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let hdc = wp as HDC;
            SetTextColor(hdc, TEXT);
            SetBkColor(hdc, CARD);
            SetBkMode(hdc, OPAQUE);
            ui_do(|ui| ui.card_brush as LRESULT).unwrap_or(0)
        }
        WM_DRAWITEM => {
            draw_button(&*(lp as *const DRAWITEMSTRUCT));
            1
        }
        WM_COMMAND => {
            let id = wp & 0xFFFF;
            if id == ID_SAVE {
                save_settings();
            } else if id == ID_CLOSE {
                ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_DESTROY => 0,
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn handle_command(id: usize) {
    match id {
        ID_REFRESH => {
            if let Some(tx) = ui_do(|ui| ui.refresh.clone()) {
                let _ = tx.send(());
            }
        }
        ID_SETTINGS => {
            hide_popup();
            show_settings();
        }
        ID_QUIT => {
            if let Some(p) = ui_do(|ui| ui.proxy.clone()) {
                let _ = p.send_event(UserEvent::Quit);
            }
        }
        _ => {}
    }
}

unsafe fn fill_bg(hwnd: HWND, hdc: HDC) {
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(hwnd, &mut rc);
    if let Some(b) = ui_do(|ui| ui.bg_brush) {
        FillRect(hdc, &rc, b);
    }
}

unsafe fn ctlcolor_static(hdc: HDC, ctl: HWND) -> LRESULT {
    let stored = GetWindowLongPtrW(ctl, GWLP_USERDATA);
    let color = if stored == 0 { TEXT } else { stored as u32 };
    SetTextColor(hdc, color);
    SetBkColor(hdc, BG);
    SetBkMode(hdc, OPAQUE);
    ui_do(|ui| ui.bg_brush as LRESULT).unwrap_or(0)
}

unsafe fn draw_button(dis: &DRAWITEMSTRUCT) {
    let hdc = dis.hDC;
    let r = dis.rcItem;
    let pressed = dis.itemState & ODS_SELECTED != 0;
    let (mut fill, text_color) = match dis.CtlID as usize {
        ID_SETTINGS | ID_SAVE => (ACCENT, rgb(0xff, 0xff, 0xff)),
        ID_QUIT => (CARD, QUIT_RED),
        _ => (CARD, TEXT),
    };
    if pressed {
        fill = darken(fill);
    }

    let brush = CreateSolidBrush(fill);
    let old_brush = SelectObject(hdc, brush as HGDIOBJ);
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    let radius = 14;
    RoundRect(hdc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    delete_object(brush as HGDIOBJ);

    let label = get_text(dis.hwndItem);
    if let Some(font) = ui_do(|ui| ui.res.font) {
        let old_font = SelectObject(hdc, font as HGDIOBJ);
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, text_color);
        let mut rr = r;
        DrawTextW(hdc, wide(&label).as_ptr(), -1, &mut rr, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        SelectObject(hdc, old_font);
    }
}

unsafe fn delete_object(o: HGDIOBJ) {
    use windows_sys::Win32::Graphics::Gdi::DeleteObject;
    DeleteObject(o);
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

fn primary_of(cfg: &Config) -> VendorId {
    cfg.ui.primary.unwrap_or(VendorId::Anthropic)
}

fn parse_vendor(id: &str) -> VendorId {
    match id {
        "openai" => VendorId::Openai,
        "zai" => VendorId::Zai,
        "openrouter" => VendorId::Openrouter,
        "deepseek" => VendorId::Deepseek,
        _ => VendorId::Anthropic,
    }
}

fn px(scale: f32, v: i32) -> i32 {
    (v as f32 * scale).round() as i32
}

fn darken(c: u32) -> u32 {
    let r = ((c & 0xFF) * 8 / 10) & 0xFF;
    let g = (((c >> 8) & 0xFF) * 8 / 10) & 0xFF;
    let b = (((c >> 16) & 0xFF) * 8 / 10) & 0xFF;
    r | (g << 8) | (b << 16)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn get_text(h: HWND) -> String {
    let len = GetWindowTextLengthW(h);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let n = GetWindowTextW(h, buf.as_mut_ptr(), len + 1);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn hwnd_topmost() -> HWND {
    (-1isize) as *mut c_void
}

unsafe fn register_class(hinst: HWND, name: &str, proc: windows_sys::Win32::UI::WindowsAndMessaging::WNDPROC) {
    // Keep the class-name buffer alive until after RegisterClassExW returns.
    let class_name = wide(name);
    let wc = WNDCLASSEXW {
        cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: proc,
        hInstance: hinst,
        hCursor: LoadCursorW(core::ptr::null_mut(), IDC_ARROW as usize as *const u16),
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };
    RegisterClassExW(&wc);
}

unsafe fn make_font(scale: f32, pt: i32, weight: i32) -> HFONT {
    CreateFontW(
        -px(scale, pt),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        0,
        0,
        CLEARTYPE_QUALITY,
        0,
        wide("Segoe UI").as_ptr(),
    )
}

unsafe fn dwm_dark(hwnd: HWND) {
    let on: i32 = 1;
    DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const i32 as *const c_void, 4);
}

unsafe fn dwm_round(hwnd: HWND) {
    let pref: i32 = DWMWCP_ROUND;
    DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const i32 as *const c_void, 4);
}
