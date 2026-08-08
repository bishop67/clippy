//! Remembers which window to paste into.
//!
//! By the time the user picks an entry, Clippy owns the focus, so the window
//! that should receive the paste has to be recorded before Clippy is shown.

use std::sync::Mutex;

#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
use crate::prelude::*;

/// An X11 window id, a Windows HWND, or the pid of a macOS application, which
/// is what NSRunningApplication is looked up by.
pub type TargetWindow = isize;

static TARGET_WINDOW: Mutex<Option<TargetWindow>> = Mutex::new(None);

/// Records the focused window, unless it is one of Clippy's own.
pub fn capture_target_window() {
    let window = current_window();

    if window.is_none() {
        return;
    }

    *TARGET_WINDOW.lock().unwrap_or_else(|e| e.into_inner()) = window;
}

pub fn get_target_window() -> Option<TargetWindow> {
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
    let window = window as u32;

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

/// Brings the recorded window to the foreground.
///
/// Windows only lets the foreground process call `SetForegroundWindow`, so
/// borrow the foreground thread's input queue for the duration of the call to
/// satisfy the "received the last input event" rule. Even then the system may
/// refuse and merely flash the taskbar button.
#[cfg(windows)]
pub fn raise_target_window() {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
        SetWindowPos, HWND_TOP, SWP_DRAWFRAME, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    let Some(window) = get_target_window() else {
        return;
    };
    let window = window as HWND;

    unsafe {
        if IsWindowVisible(window) == 0 {
            printlog!("target_window: {window:?} is no longer visible, not raising");
            return;
        }

        let this_thread = GetCurrentThreadId();
        let foreground_thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());

        let attached =
            this_thread != foreground_thread && AttachThreadInput(this_thread, foreground_thread, 1) != 0;

        if SetForegroundWindow(window) == 0 {
            printlog!("target_window: SetForegroundWindow was refused");
        } else {
            SetWindowPos(
                window,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_DRAWFRAME | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
        }

        if attached {
            AttachThreadInput(this_thread, foreground_thread, 0);
        }
    }
}

/// Activates the recorded application.
///
/// macOS has no portable handle for another process's window, so the target is
/// the owning application and AppKit decides which of its windows comes
/// forward.
#[cfg(target_os = "macos")]
pub fn raise_target_window() {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

    let Some(pid) = get_target_window() else {
        return;
    };

    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32) else {
        printlog!("target_window: application {pid} is gone, not raising");
        return;
    };

    if !app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps) {
        printlog!("target_window: application {pid} refused activation");
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
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

/// Whether any of Ctrl, Shift, Alt or Win is currently held.
#[cfg(windows)]
pub fn modifiers_held() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
        VK_RSHIFT, VK_RWIN,
    };

    const KEYS: [i32; 9] = [
        VK_LWIN as i32,
        VK_RWIN as i32,
        VK_LCONTROL as i32,
        VK_RCONTROL as i32,
        VK_LSHIFT as i32,
        VK_RSHIFT as i32,
        VK_LMENU as i32,
        VK_RMENU as i32,
        VK_MENU as i32,
    ];

    // The high bit is set while the key is physically down.
    KEYS.iter()
        .any(|&key| unsafe { GetKeyState(key) } as u16 & 0x8000 != 0)
}

/// Whether any of Command, Control, Option or Shift is currently held.
#[cfg(target_os = "macos")]
pub fn modifiers_held() -> bool {
    use objc2_app_kit::{NSEvent, NSEventModifierFlags};

    let flags = NSEvent::modifierFlags_class();

    flags.intersects(
        NSEventModifierFlags::Command
            | NSEventModifierFlags::Control
            | NSEventModifierFlags::Option
            | NSEventModifierFlags::Shift,
    )
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub fn modifiers_held() -> bool {
    false
}

/// The window the display server currently considers focused.
#[cfg(target_os = "linux")]
pub fn current_window() -> Option<TargetWindow> {
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

    Some(active as TargetWindow)
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

/// The foreground window, unless it belongs to Clippy.
#[cfg(windows)]
pub fn current_window() -> Option<TargetWindow> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    let window = unsafe { GetForegroundWindow() };

    if window.is_null() {
        return None;
    }

    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };

    // Skip Clippy's own windows, otherwise opening the picker overwrites the
    // target with the picker itself.
    if pid == std::process::id() {
        printlog!("target_window: ignoring own window");
        return None;
    }

    Some(window as TargetWindow)
}

/// The frontmost application, unless it is Clippy.
///
/// Tracks the application rather than a window: AppKit exposes no handle for
/// another process's windows, and `NSRunningApplication` is what can be
/// reactivated later.
#[cfg(target_os = "macos")]
pub fn current_window() -> Option<TargetWindow> {
    use objc2_app_kit::NSWorkspace;

    let pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()?
        .processIdentifier();

    if pid <= 0 {
        return None;
    }

    if pid as u32 == std::process::id() {
        printlog!("target_window: ignoring own application");
        return None;
    }

    Some(pid as TargetWindow)
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub fn current_window() -> Option<TargetWindow> {
    None
}
