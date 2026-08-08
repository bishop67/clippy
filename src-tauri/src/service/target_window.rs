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

/// Asks the window manager to activate the recorded window.
///
/// Goes through the EWMH `_NET_ACTIVE_WINDOW` client message rather than
/// `SetInputFocus`, which errors with BadMatch on an unviewable window and
/// fights the window manager's own focus policy on non-reparenting WMs.
#[cfg(target_os = "linux")]
pub fn raise_target_window() {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        ClientMessageEvent, ConnectionExt, EventMask, InputFocus, MapState, StackMode,
    };

    let Some(window) = get_target_window() else {
        return;
    };

    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return;
    };

    let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else {
        return;
    };

    // A window that has since been unmapped or destroyed makes both the raise
    // and the focus request an error.
    let viewable = conn
        .get_window_attributes(window)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|attrs| attrs.map_state == MapState::VIEWABLE);

    if !viewable {
        printlog!("target_window: {window} is no longer viewable, not raising");
        return;
    }

    if let Ok(atom) = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW") {
        if let Ok(atom) = atom.reply() {
            // data[0] = 2 marks the request as coming from a pager, which window
            // managers honour without the focus-stealing heuristics they apply
            // to ordinary applications.
            let event = ClientMessageEvent::new(
                32,
                window,
                atom.atom,
                [2, x11rb::CURRENT_TIME, 0, 0, 0],
            );

            let _ = conn.send_event(
                false,
                root,
                EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
                event,
            );
            let _ = conn.flush();
            return;
        }
    }

    // No EWMH support: fall back to raising and focusing directly.
    let config = x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
    let _ = conn.configure_window(window, &config);
    let _ = conn.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME);
    let _ = conn.flush();
}

/// No-op until the platform can name the window to raise. Tauri only manages
/// its own windows, so this needs AXUIElement on macOS and SetForegroundWindow
/// on Windows, alongside a capture that records those handles.
#[cfg(not(target_os = "linux"))]
pub fn raise_target_window() {}

/// Whether any of Ctrl, Shift, Alt or Super is currently held.
#[cfg(target_os = "linux")]
pub fn modifiers_held() -> bool {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};

    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return false;
    };

    let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else {
        return false;
    };

    let held = conn
        .query_pointer(root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.mask);

    let Some(held) = held else {
        return false;
    };

    let modifiers = KeyButMask::CONTROL | KeyButMask::SHIFT | KeyButMask::MOD1 | KeyButMask::MOD4;

    held.intersects(modifiers)
}

/// Reading live modifier state needs GetKeyState on Windows and CGEventSource
/// on macOS; until then, assume nothing is held.
#[cfg(not(target_os = "linux"))]
pub fn modifiers_held() -> bool {
    false
}

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
