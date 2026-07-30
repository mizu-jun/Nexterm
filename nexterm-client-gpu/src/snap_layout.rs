//! Windows 11 snap-layout support for the custom title bar
//! (`window.decorations = "notitle"`).
//!
//! DWM shows the snap-layout flyout only while it believes the cursor rests
//! on the window's maximize button, which it learns from `WM_NCHITTEST`
//! returning `HTMAXBUTTON`. A borderless winit window answers `HTCLIENT`
//! everywhere, so the tab bar's custom maximize button never gets the
//! flyout. This module subclasses the winit window procedure
//! (`SetWindowSubclass`) and answers `HTMAXBUTTON` for the button's
//! rectangle, Windows Terminal-style.
//!
//! Returning `HTMAXBUTTON` moves the button into the non-client area, so
//! winit stops delivering mouse events there. Hover and click are
//! reconstructed from the `WM_NC*` messages and forwarded to the event loop
//! as `UserEvent::SnapMaximizeHover` / `UserEvent::SnapMaximizeToggle`; the
//! snap-layout flyout's own zone buttons are handled entirely by DWM and
//! need no code here.
//!
//! The subclass is installed unconditionally on Windows and stays dormant
//! (hit-test falls through to the default procedure) until the renderer
//! registers a button rectangle via [`update_max_button_rect`], which only
//! happens while the custom title bar is active.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::renderer::UserEvent;

/// Subclass ID for `SetWindowSubclass`; must only be unique within this
/// process ("NxSL" in ASCII). One subclass per window.
const SUBCLASS_ID: usize = 0x4e78_534c;

/// Per-window state shared with the subclass procedure.
///
/// The procedure runs on the event-loop thread (non-client messages are sent,
/// not posted), but it can fire while `EventHandler` holds `&mut self`, so
/// state is exchanged through this registry instead of reaching into the
/// handler.
struct SnapState {
    proxy: EventLoopProxy<UserEvent>,
    window_id: winit::window::WindowId,
    /// Maximize-button rectangle in physical client pixels `(x0, y0, x1, y1)`,
    /// half-open on the right/bottom edges like every tab-bar hit rect.
    /// `None` while the custom title bar is inactive.
    max_button_rect: Option<(f32, f32, f32, f32)>,
    /// Last hover state forwarded to the event loop (dedupes UserEvents).
    hovered: bool,
}

static REGISTRY: LazyLock<Mutex<HashMap<isize, SnapState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn hwnd_of(window: &Window) -> Option<isize> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return None;
    };
    Some(h.hwnd.get())
}

/// Install the snap-layout subclass on `window`. Call once per window,
/// right after creation. Failure is non-fatal: the custom maximize button
/// keeps working through the regular client-area mouse path, only the
/// snap-layout flyout is lost.
pub(crate) fn install(window: &Window, proxy: EventLoopProxy<UserEvent>) {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;

    let Some(hwnd) = hwnd_of(window) else {
        return;
    };
    REGISTRY
        .lock()
        .expect("snap-layout registry lock poisoned")
        .insert(
            hwnd,
            SnapState {
                proxy,
                window_id: window.id(),
                max_button_rect: None,
                hovered: false,
            },
        );
    // SAFETY: `hwnd` is a live handle owned by winit on this thread.
    //         `subclass_proc` only touches the process-global REGISTRY and
    //         removes itself on WM_NCDESTROY, before the handle is recycled.
    let ok = unsafe { SetWindowSubclass(hwnd as _, Some(subclass_proc), SUBCLASS_ID, 0) };
    if ok == 0 {
        tracing::warn!("SetWindowSubclass failed; snap layouts will not be offered");
        REGISTRY
            .lock()
            .expect("snap-layout registry lock poisoned")
            .remove(&hwnd);
    }
}

/// Publish the maximize button's current rectangle (physical client pixels)
/// to the hit-test hook. The renderer calls this after every frame; `None`
/// (native title bar, hidden tab bar) puts the hook back to sleep.
pub(crate) fn update_max_button_rect(window: &Window, rect: Option<(f32, f32, f32, f32)>) {
    let Some(hwnd) = hwnd_of(window) else {
        return;
    };
    if let Some(state) = REGISTRY
        .lock()
        .expect("snap-layout registry lock poisoned")
        .get_mut(&hwnd)
    {
        state.max_button_rect = rect;
    }
}

/// Split a `WM_NCHITTEST` lparam into signed screen coordinates
/// (`GET_X_LPARAM` / `GET_Y_LPARAM`). The `i16` round-trip is what keeps
/// monitors positioned left of / above the primary (negative coordinates)
/// working.
fn decode_screen_point(lparam: isize) -> (i32, i32) {
    let x = (lparam as u32 & 0xffff) as u16 as i16 as i32;
    let y = ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as i32;
    (x, y)
}

/// Half-open containment test matching the tab bar's hit-rect convention
/// (`x0 <= x < x1`, `y0 <= y < y1`).
fn rect_contains(rect: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    let (x0, y0, x1, y1) = rect;
    x >= x0 && x < x1 && y >= y0 && y < y1
}

/// Look up the registered rectangle and report whether the given physical
/// client coordinates land on the maximize button.
fn hit_max_button(hwnd: isize, x: f32, y: f32) -> bool {
    REGISTRY
        .lock()
        .expect("snap-layout registry lock poisoned")
        .get(&hwnd)
        .and_then(|s| s.max_button_rect)
        .is_some_and(|rect| rect_contains(rect, x, y))
}

/// Forward a hover transition to the event loop (deduped). Runs outside the
/// registry lock so `send_event` cannot contend with it.
fn set_hover(hwnd: isize, hovered: bool) {
    let target = {
        let mut reg = REGISTRY.lock().expect("snap-layout registry lock poisoned");
        let Some(state) = reg.get_mut(&hwnd) else {
            return;
        };
        if state.hovered == hovered {
            return;
        }
        state.hovered = hovered;
        (state.proxy.clone(), state.window_id)
    };
    if let Err(e) = target.0.send_event(UserEvent::SnapMaximizeHover {
        window_id: target.1,
        hovered,
    }) {
        tracing::warn!("failed to send SnapMaximizeHover UserEvent: {}", e);
    }
}

/// Forward a completed non-client click on the maximize button.
fn send_toggle(hwnd: isize) {
    let target = {
        let reg = REGISTRY.lock().expect("snap-layout registry lock poisoned");
        let Some(state) = reg.get(&hwnd) else {
            return;
        };
        (state.proxy.clone(), state.window_id)
    };
    if let Err(e) = target.0.send_event(UserEvent::SnapMaximizeToggle {
        window_id: target.1,
    }) {
        tracing::warn!("failed to send SnapMaximizeToggle UserEvent: {}", e);
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    umsg: u32,
    wparam: usize,
    lparam: isize,
    _uidsubclass: usize,
    _dwrefdata: usize,
) -> isize {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HTMAXBUTTON, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN,
        WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
    };

    let key = hwnd as isize;
    match umsg {
        WM_NCHITTEST => {
            let (sx, sy) = decode_screen_point(lparam);
            let mut pt = POINT { x: sx, y: sy };
            // SAFETY: `pt` is a valid local POINT; ScreenToClient only writes
            //         through the pointer it is given.
            let ok = unsafe { ScreenToClient(hwnd, &mut pt) };
            if ok != 0 && hit_max_button(key, pt.x as f32, pt.y as f32) {
                return HTMAXBUTTON as isize;
            }
        }
        WM_NCMOUSEMOVE => {
            let on_button = wparam == HTMAXBUTTON as usize;
            set_hover(key, on_button);
            if on_button {
                // Request WM_NCMOUSELEAVE so the hover highlight also clears
                // when the cursor exits the window through the button.
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: `tme` is a valid, fully initialised local struct.
                unsafe { TrackMouseEvent(&mut tme) };
            }
        }
        WM_NCMOUSELEAVE => set_hover(key, false),
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if wparam == HTMAXBUTTON as usize => {
            // Swallow the press: DefWindowProc has no sane legacy handling
            // for HTMAXBUTTON on a borderless window. The click completes on
            // WM_NCLBUTTONUP below (double-clicks toggle twice, matching a
            // native maximize box).
            return 0;
        }
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as usize => {
            send_toggle(key);
            return 0;
        }
        WM_NCDESTROY => {
            // SAFETY: removing this very subclass while handling the last
            //         message the window will ever receive.
            unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
            REGISTRY
                .lock()
                .expect("snap-layout registry lock poisoned")
                .remove(&key);
        }
        _ => {}
    }
    // SAFETY: plain pass-through to the next procedure in the chain with the
    //         unmodified arguments.
    unsafe { DefSubclassProc(hwnd, umsg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_screen_point_splits_positive_coords() {
        // x = 300 (0x012c), y = 200 (0x00c8).
        let lparam = ((200_i32 << 16) | 300_i32) as isize;
        assert_eq!(decode_screen_point(lparam), (300, 200));
    }

    #[test]
    fn decode_screen_point_keeps_negative_coords_signed() {
        // A monitor left of / above the primary yields negative screen
        // coordinates; the packed u16 halves must round-trip through i16.
        let x: i16 = -120;
        let y: i16 = -45;
        let lparam = (((y as u16 as u32) << 16) | (x as u16 as u32)) as i32 as isize;
        assert_eq!(decode_screen_point(lparam), (-120, -45));
    }

    #[test]
    fn rect_contains_is_half_open() {
        let rect = (100.0, 0.0, 164.0, 32.0);
        assert!(rect_contains(rect, 100.0, 0.0), "left/top edges inclusive");
        assert!(rect_contains(rect, 163.9, 31.9));
        assert!(!rect_contains(rect, 164.0, 10.0), "right edge exclusive");
        assert!(!rect_contains(rect, 120.0, 32.0), "bottom edge exclusive");
        assert!(!rect_contains(rect, 99.9, 10.0));
    }
}
