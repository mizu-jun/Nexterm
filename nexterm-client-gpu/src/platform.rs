//! Platform-specific window and OS integration utilities.
//!
//! - `apply_backdrop`: apply the configured OS-native window backdrop material.
//! - `open_releases_url`: open the GitHub releases page in the default browser.
//! - `cursor_screen_pos`: get the global screen position of the mouse cursor (Phase 4-2).

use nexterm_config::ResolvedBackdrop;

/// The `DWMWA_SYSTEMBACKDROP_TYPE` value for a resolved backdrop.
///
/// The documented enum is `DWMSBT_AUTO = 0`, `DWMSBT_NONE = 1`,
/// `DWMSBT_MAINWINDOW = 2` (Mica), `DWMSBT_TRANSIENTWINDOW = 3` (Acrylic) and
/// `DWMSBT_TABBEDWINDOW = 4` (Mica Alt). Returns `None` for
/// [`ResolvedBackdrop::Vibrancy`], which is a macOS material with no DWM
/// equivalent.
///
/// Compiled on every platform on purpose: the mapping is then testable without
/// a Windows machine, and the Windows CI job is not the only thing standing
/// between a wrong constant and a release.
///
/// Its only non-test caller is Windows-only (`apply_backdrop_windows`), so on
/// every other target it would otherwise be flagged as dead code despite
/// being exercised by `backdrop_tests` below.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const fn dwm_backdrop_value(resolved: ResolvedBackdrop) -> Option<u32> {
    match resolved {
        ResolvedBackdrop::None => Some(1),
        ResolvedBackdrop::Mica => Some(2),
        ResolvedBackdrop::Acrylic => Some(3),
        ResolvedBackdrop::MicaAlt => Some(4),
        ResolvedBackdrop::Vibrancy => None,
    }
}

/// Apply the OS-native backdrop material to a window.
///
/// - **Windows**: `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)`. Requires
///   Windows 11 build 22621 (22H2); on older builds the attribute does not
///   exist and the call fails harmlessly.
/// - **macOS**: `NSVisualEffectView` vibrancy, via `window-vibrancy`.
/// - **Everything else**: nothing. Linux has no cross-compositor equivalent;
///   `window.in_app_blur_enabled` (P2b) is the in-app substitute.
///
/// A backdrop that cannot be applied must never stop a window from opening, so
/// every failure here is logged and swallowed.
pub(crate) fn apply_backdrop(window: &winit::window::Window, resolved: ResolvedBackdrop) {
    #[cfg(windows)]
    apply_backdrop_windows(window, resolved);
    #[cfg(target_os = "macos")]
    apply_backdrop_macos(window, resolved);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (window, resolved);
    }
}

#[cfg(target_os = "macos")]
fn apply_backdrop_macos(window: &winit::window::Window, resolved: ResolvedBackdrop) {
    use window_vibrancy::{NSVisualEffectMaterial, apply_vibrancy, clear_vibrancy};

    let result = match resolved {
        ResolvedBackdrop::None => clear_vibrancy(window).map(|_| ()),
        // AppKit has a single material family, so Mica, Mica Alt and Acrylic
        // all land here (see `WindowBackdrop::resolve`).
        //
        // `UnderWindowBackground` is AppKit's material for window backgrounds.
        // It is an unmeasured initial recipe, in the same class as P2a's
        // `shadow_params` and P2b's `ACRYLIC_TINT_OPACITY`: expected to need
        // tuning against real hardware rather than merely confirming, because
        // nobody on this project can run macOS.
        _ => apply_vibrancy(
            window,
            NSVisualEffectMaterial::UnderWindowBackground,
            None,
            None,
        )
        .map(|_| ()),
    };
    if let Err(e) = result {
        tracing::warn!("failed to apply the macOS window backdrop: {e}");
    }
}

#[cfg(windows)]
fn apply_backdrop_windows(window: &winit::window::Window, resolved: ResolvedBackdrop) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Some(backdrop_type) = dwm_backdrop_value(resolved) else {
        return;
    };
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    // In raw-window-handle 0.6, `hwnd` is a `NonZeroIsize` (= isize).
    // In windows-sys 0.59, `HWND = *mut c_void`, so convert from isize.
    let hwnd = h.hwnd.get() as *mut ::core::ffi::c_void;

    // DWMWA_SYSTEMBACKDROP_TYPE = 38.
    // SAFETY: `hwnd` is a valid window handle obtained from winit, and
    //         `backdrop_type` is a live local `u32` for the duration of the
    //         call, matching the 4-byte size passed alongside it. The
    //         attribute only exists on Windows 11 build 22621 and later; below
    //         that the call returns a failure HRESULT, which is logged rather
    //         than acted on.
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            38,
            &backdrop_type as *const u32 as *const ::core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if hr != 0 {
        tracing::debug!(
            "DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, {backdrop_type}) returned \
             0x{hr:08x}; expected on Windows 10 and on Windows 11 before build 22621"
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

/// Open `config.toml` with the OS default handler (P4, WT's "Open JSON
/// file" equivalent). Creates an empty file first when it does not exist
/// yet, so the editor does not error out on a missing path.
pub(crate) fn open_config_file() {
    let path = nexterm_config::toml_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, "") {
            tracing::warn!("failed to create {}: {}", path.display(), e);
            return;
        }
    }
    let path_str = path.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&path_str).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open")
        .arg(&path_str)
        .spawn();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        // Without CREATE_NO_WINDOW a GUI-subsystem process spawning a console
        // app briefly flashes a black console window. `start ""` keeps paths
        // with spaces from being consumed as the window title.
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", &path_str])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
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
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        // Without CREATE_NO_WINDOW a GUI-subsystem process spawning a console
        // app briefly flashes a black console window.
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

#[cfg(test)]
mod backdrop_tests {
    use super::*;
    use nexterm_config::{BackdropTarget, ResolvedBackdrop, WindowBackdrop};

    /// Pinned against the documented `DWM_SYSTEMBACKDROP_TYPE` enum:
    /// `DWMSBT_AUTO = 0`, `DWMSBT_NONE = 1`, `DWMSBT_MAINWINDOW = 2` (Mica),
    /// `DWMSBT_TRANSIENTWINDOW = 3` (Acrylic), `DWMSBT_TABBEDWINDOW = 4`
    /// (Mica Alt).
    #[test]
    fn dwm_values_match_the_documented_enum() {
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::None), Some(1));
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::Mica), Some(2));
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::Acrylic), Some(3));
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::MicaAlt), Some(4));
    }

    /// Vibrancy is a macOS material. Returning `None` rather than a fallback
    /// number keeps "there is no DWM value for this" in the type instead of in
    /// a comment.
    #[test]
    fn vibrancy_has_no_dwm_value() {
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::Vibrancy), None);
    }

    /// The pre-P2c client hard-coded the literal `4` under the name
    /// `apply_acrylic_blur`, and the doc comment, the crate CLAUDE.md and
    /// CHANGELOG.md:2898 all called it "Acrylic". It is Mica Alt. This pins
    /// the default end-to-end so the correction cannot drift back.
    #[test]
    fn the_default_windows_backdrop_is_mica_alt() {
        let resolved = WindowBackdrop::Auto.resolve(BackdropTarget::Windows);
        assert_eq!(resolved, ResolvedBackdrop::MicaAlt);
        assert_eq!(dwm_backdrop_value(resolved), Some(4));
    }
}
