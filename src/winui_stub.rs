//! Non-Windows stub for the native UI module. Keeps the crate (and its data
//! layer + tests) compiling and runnable on other platforms; the real
//! implementation lives in `winui_win.rs` and is Windows-only.

use std::sync::mpsc::Sender;

use tao::event_loop::EventLoopProxy;

use crate::{UpdatePayload, UserEvent};

pub fn init(_proxy: EventLoopProxy<UserEvent>, _refresh: Sender<()>) {}

pub fn set_data(_payload: UpdatePayload) {}

pub fn toggle_popup(_x: i32, _y: i32, _w: i32, _h: i32) {}
