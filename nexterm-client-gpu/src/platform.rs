//! プラットフォーム依存のウィンドウ・OS 連携ユーティリティ
//!
//! - `apply_acrylic_blur`: Windows 11 の Acrylic（すりガラス）効果適用
//! - `open_releases_url`: GitHub リリースページをデフォルトブラウザで開く

/// Windows 11 の Acrylic（すりガラス）効果をウィンドウに適用する
///
/// DwmSetWindowAttribute で DWMWA_SYSTEMBACKDROP_TYPE = 4 (DWMWCP_ACRYLIC) を指定する。
/// Windows 10 や旧バージョンでは API が存在しないため何も起きない。
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
    // raw-window-handle 0.6 の hwnd は NonZeroIsize (= isize)。
    // windows-sys 0.59 では HWND = *mut c_void なので isize から変換する。
    let hwnd = h.hwnd.get() as *mut ::core::ffi::c_void;

    // DWMWA_SYSTEMBACKDROP_TYPE = 38; 4 = DWMWCP_ACRYLIC（Windows 11 22H2+）
    let backdrop_type: u32 = 4;
    // SAFETY: hwnd は winit から取得した有効なウィンドウハンドル。
    //         DwmSetWindowAttribute は失敗しても戻り値を無視して続行する。
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            38,
            &backdrop_type as *const _ as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// GitHub リリースページをデフォルトブラウザで開く
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
