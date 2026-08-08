//! Remembers which window to paste into.
//!
//! By the time the user picks an entry, Clippy owns the focus, so the window
//! that should receive the paste has to be recorded before Clippy is shown.

use std::sync::Mutex;

#[cfg(target_os = "linux")]
use crate::prelude::*;

static TARGET_WINDOW: Mutex<Option<u32>> = Mutex::new(None);

/// Records the focused window, unless it is one of Clippy's own.
pub fn capture_target_window() {
    let window = current_window();

    if window.is_none() {
        return;
    }

    *TARGET_WINDOW.lock().unwrap_or_else(|e| e.into_inner()) = window;
}

pub fn get_target_window() -> Option<u32> {
    *TARGET_WINDOW.lock().unwrap_or_else(|e| e.into_inner())
}

/// Raises the recorded window and asks the server to focus it.
#[cfg(target_os = "linux")]
pub fn raise_target_window() {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, InputFocus, StackMode};

    let Some(window) = get_target_window() else {
        return;
    };

    let Ok((conn, _)) = x11rb::connect(None) else {
        return;
    };

    let config = x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
    if conn.configure_window(window, &config).is_err() {
        return;
    }

    let _ = conn.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME);
    let _ = conn.flush();
}

#[cfg(not(target_os = "linux"))]
pub fn raise_target_window() {}

/// The window the display server currently considers focused.
#[cfg(target_os = "linux")]
pub fn current_window() -> Option<u32> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen_num)?.root;

    let atom = conn
        .intern_atom(true, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let active = conn
        .get_property(false, root, atom, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()?;

    if active == 0 {
        return None;
    }

    // Skip Clippy's own windows, otherwise opening the picker overwrites the
    // target with the picker itself.
    if is_own_window(&conn, active) {
        printlog!("target_window: ignoring own window {active}");
        return None;
    }

    Some(active)
}

/// Compares the window's PID against ours via _NET_WM_PID.
#[cfg(target_os = "linux")]
fn is_own_window(conn: &impl x11rb::connection::Connection, window: u32) -> bool {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let Ok(cookie) = conn.intern_atom(true, b"_NET_WM_PID") else {
        return false;
    };
    let Ok(atom) = cookie.reply() else {
        return false;
    };

    let Ok(cookie) = conn.get_property(false, window, atom.atom, AtomEnum::CARDINAL, 0, 1) else {
        return false;
    };
    let Ok(reply) = cookie.reply() else {
        return false;
    };

    reply
        .value32()
        .and_then(|mut v| v.next())
        .is_some_and(|pid| pid == std::process::id())
}

/// Wayland exposes no way to query the focused window, and the RemoteDesktop
/// portal injects into whatever holds focus rather than a named window.
#[cfg(not(target_os = "linux"))]
pub fn current_window() -> Option<u32> {
    None
}
