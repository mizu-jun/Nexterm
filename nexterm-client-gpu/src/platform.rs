//! Platform-specific window and OS integration utilities.
//!
//! - `apply_acrylic_blur`: enable the Windows 11 Acrylic (frosted glass) effect.
//! - `open_releases_url`: open the GitHub releases page in the default browser.
//! - `cursor_screen_pos`: get the global screen position of the mouse cursor (Phase 4-2).

/// Apply the Windows 11 Acrylic (frosted glass) effect to a window.
///
/// Calls `DwmSetWindowAttribute` with `DWMWA_SYSTEMBACKDROP_TYPE = 4` (`DWMWCP_ACRYLIC`).
/// On Windows 10 and earlier this attribute does not exist, so the call is a no-op.
#[cfg(windows)]
pub(crate) fn apply_acrylic_blur(window: &winit::window::Window) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    // In raw-window-handle 0.6, `hwnd` is a `NonZeroIsize` (= isize).
    // In windows-sys 0.59, `HWND = *mut c_void`, so convert from isize.
    let hwnd = h.hwnd.get() as *mut ::core::ffi::c_void;

    // DWMWA_SYSTEMBACKDROP_TYPE = 38; 4 = DWMWCP_ACRYLIC (Windows 11 22H2+).
    let backdrop_type: u32 = 4;
    // SAFETY: `hwnd` is a valid window handle obtained from winit.
    //         DwmSetWindowAttribute may fail, but we ignore the return value and continue.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            38,
            &backdrop_type as *const _ as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// Return the mouse cursor's global screen position (Sprint 5-8 Phase 4-2).
///
/// The result is in pixels and feeds the off-tab drop detection
/// (`drop_target::compute_drop_target`). Platform-specific behavior:
///
/// - **Windows**: queries `GetCursorPos` directly from the OS (works even when the
///   cursor is outside the window).
/// - **Other**: returns `None`; callers fall back to combining winit's
///   `window.outer_position()` with the window-local cursor position.
///
/// On Wayland `outer_position` itself is unavailable, so propagating `None`
/// effectively disables off-tab drop detection (covered by the four alternate UXs
/// from decision #4). Native implementations for macOS / X11
/// (`NSEvent.mouseLocation` / `XQueryPointer`) can be added in Phase 4-3 or later
/// as needed.
pub(crate) fn cursor_screen_pos() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        // SAFETY: GetCursorPos only writes through the POINT* it is given. `pt` is a
        //         valid local variable and nothing else references it during the call.
        //         A return value of 0 indicates failure.
        let ok = unsafe { GetCursorPos(&mut pt as *mut POINT) };
        if ok != 0 { Some((pt.x, pt.y)) } else { None }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Open the GitHub releases page in the default browser.
pub(crate) fn open_releases_url() {
    let url = "https://github.com/mizu-jun/nexterm/releases/latest";
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", url])
        .spawn();
}
