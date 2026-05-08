//! error-detector — PTY 出力中の "error" / "Error" / "ERROR" を検知して
//! ステータス行に通知を書き込むサンプルプラグイン。
//!
//! ## ビルド
//! ```sh
//! cargo build --release --target wasm32-unknown-unknown
//! # 出力: target/wasm32-unknown-unknown/release/error_detector.wasm
//! ```
//!
//! ## 登録 (nexterm.toml)
//! ```toml
//! [[plugins]]
//! path = "~/.config/nexterm/plugins/error_detector.wasm"
//! ```

use std::sync::atomic::{AtomicU32, Ordering};

/// 検知したエラー行数のカウンター
static ERROR_COUNT: AtomicU32 = AtomicU32::new(0);

// ---- ホストインポート関数 ---------------------------------------------------

extern "C" {
    /// Nexterm のログに文字列を書き込む
    fn nexterm_log(ptr: *const u8, len: usize);

    /// 指定ペインにテキストを書き込む
    fn nexterm_write_pane(pane_id: i32, ptr: *const u8, len: usize);
}

/// ホストのログ関数を安全にラップするヘルパー
fn log(msg: &str) {
    let bytes = msg.as_bytes();
    // SAFETY: ptr/len は bytes スライスから取得した有効な値
    unsafe { nexterm_log(bytes.as_ptr(), bytes.len()) };
}

/// 指定ペインにテキストを書き込むヘルパー
fn write_pane(pane_id: i32, msg: &str) {
    let bytes = msg.as_bytes();
    // SAFETY: ptr/len は bytes スライスから取得した有効な値
    unsafe { nexterm_write_pane(pane_id, bytes.as_ptr(), bytes.len()) };
}

// ---- エクスポート関数 -------------------------------------------------------

/// プラグイン初期化（オプション）
#[no_mangle]
pub extern "C" fn nexterm_init() {
    log("[error-detector] 初期化完了");
}

/// PTY 出力フック
///
/// "error" を含む行を検知したらカウンターをインクリメントし、
/// pane_id=0 に通知バナーを書き込む。
///
/// # Returns
/// 0 = 出力をそのまま通過させる（プラグインは表示を変えない）
/// 1 = 出力を抑制する（今回は常に 0）
#[no_mangle]
pub extern "C" fn nexterm_on_output(output_ptr: *const u8, output_len: usize, pane_id: i32) -> i32 {
    // SAFETY: サーバーは有効な UTF-8 バッファへのポインタを渡す
    let bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let text = std::str::from_utf8(bytes).unwrap_or("");

    // 大文字小文字を問わず "error" を検知する
    if text.to_ascii_lowercase().contains("error") {
        let count = ERROR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let notice = format!(
            "\x1b[33m[error-detector] エラーを検知しました（累計 {} 件） — pane {}\x1b[0m\r\n",
            count, pane_id
        );
        write_pane(pane_id, &notice);
        log(&format!(
            "[error-detector] エラー検知: pane={} count={}",
            pane_id, count
        ));
    }

    0 // 出力を通過させる
}

/// カスタムコマンドフック
///
/// `:error-reset` コマンドでカウンターをリセットする。
///
/// # Returns
/// 0 = コマンド処理済み
/// 1 = このプラグインでは未処理（他のプラグインに委譲）
#[no_mangle]
pub extern "C" fn nexterm_on_command(cmd_ptr: *const u8, cmd_len: usize) -> i32 {
    // SAFETY: サーバーは有効な UTF-8 バッファへのポインタを渡す
    let bytes = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len) };
    let cmd = std::str::from_utf8(bytes).unwrap_or("");

    if cmd.trim() == ":error-reset" {
        ERROR_COUNT.store(0, Ordering::Relaxed);
        log("[error-detector] エラーカウンターをリセットしました");
        return 0; // 処理済み
    }

    1 // 未処理
}
