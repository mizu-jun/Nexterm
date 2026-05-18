//! プラットフォーム依存のウィンドウ・OS 連携ユーティリティ
//!
//! - `apply_acrylic_blur`: Windows 11 の Acrylic（すりガラス）効果適用
//! - `open_releases_url`: GitHub リリースページをデフォルトブラウザで開く
//! - `cursor_screen_pos`: マウスカーソルのグローバルスクリーン座標を取得（Phase 4-2）

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

/// マウスカーソルのグローバルスクリーン座標を取得する（Sprint 5-8 Phase 4-2）。
///
/// 戻り値の単位はピクセル。タブ外ドロップ検出（`drop_target::compute_drop_target`）の
/// 入力として使用する。プラットフォーム別実装:
///
/// - **Windows**: `GetCursorPos` で OS から直接取得（カーソルがウィンドウ外でも正確）
/// - **その他**: `None` を返す。呼び出し側が winit の `window.outer_position()` と
///   ウィンドウローカルなカーソル位置を加算してフォールバック計算する
///
/// Wayland では `outer_position` 自体が取得不能なため、`None` のまま伝播することで
/// タブ外ドロップ判定を機能無効化する（決定 #4 の代替 UX 4 種でカバー）。
/// macOS / X11 でのネイティブ実装（`NSEvent.mouseLocation` / `XQueryPointer`）は
/// Phase 4-3 以降で必要に応じて追加する。
pub(crate) fn cursor_screen_pos() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        // SAFETY: GetCursorPos は POINT* に書き込むのみ。pt は有効なローカル変数で、
        //         関数呼び出し中に他から参照されない。戻り値 0 は失敗を示す。
        let ok = unsafe { GetCursorPos(&mut pt as *mut POINT) };
        if ok != 0 { Some((pt.x, pt.y)) } else { None }
    }
    #[cfg(not(windows))]
    {
        None
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
