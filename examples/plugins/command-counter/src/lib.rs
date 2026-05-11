//! command-counter — OSC 133 セマンティックゾーンを利用してコマンド実行回数と
//! 直近の終了コードを追跡するサンプルプラグイン。
//!
//! ## ビルド
//! ```sh
//! cargo build --release --target wasm32-unknown-unknown
//! ```
//!
//! ## 登録 (nexterm.toml)
//! ```toml
//! [[plugins]]
//! path = "~/.config/nexterm/plugins/command_counter.wasm"
//! ```
//!
//! ## カスタムコマンド
//! - `:count-show`  — 現在の実行回数と最終終了コードを表示
//! - `:count-reset` — カウンターをリセット

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

/// コマンド実行回数
static CMD_COUNT: AtomicU32 = AtomicU32::new(0);
/// 直近の終了コード（-1 = 未記録）
static LAST_EXIT: AtomicI32 = AtomicI32::new(-1);

extern "C" {
    fn nexterm_log(ptr: *const u8, len: usize);
    fn nexterm_write_pane(pane_id: i32, ptr: *const u8, len: usize);
}

fn log(msg: &str) {
    let b = msg.as_bytes();
    // SAFETY: 有効なスライスポインタを渡す
    unsafe { nexterm_log(b.as_ptr(), b.len()) };
}

fn write_pane(pane_id: i32, msg: &str) {
    let b = msg.as_bytes();
    // SAFETY: 有効なスライスポインタを渡す
    unsafe { nexterm_write_pane(pane_id, b.as_ptr(), b.len()) };
}

/// Plugin ABI バージョンを宣言する（Sprint 5-4 / F1 で v2 化）。
///
/// v2 では `pane_id` 引数が有効値として渡され、`write_pane` は
/// `allowed_panes` リストに登録された pane のみ書き込み可能。
#[no_mangle]
pub extern "C" fn nexterm_api_version() -> i32 {
    2
}

#[no_mangle]
pub extern "C" fn nexterm_init() {
    log("[command-counter] 初期化完了");
}

/// PTY 出力フック
///
/// OSC 133 D シーケンス（`\x1b]133;D;exit=N\x07`）を検知して
/// 終了コードとコマンド回数を記録する。
#[no_mangle]
pub extern "C" fn nexterm_on_output(
    output_ptr: *const u8,
    output_len: usize,
    _pane_id: i32,
) -> i32 {
    // SAFETY: サーバーは有効な UTF-8 バッファを渡す
    let bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let text = std::str::from_utf8(bytes).unwrap_or("");

    // OSC 133 D シーケンス: \x1b]133;D;exit=N\x07 または \x1b]133;D\x07
    if text.contains("\x1b]133;D") {
        CMD_COUNT.fetch_add(1, Ordering::Relaxed);

        // exit=N を解析する
        if let Some(exit_start) = text.find("exit=") {
            let rest = &text[exit_start + 5..];
            let exit_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = exit_str.parse::<i32>() {
                LAST_EXIT.store(code, Ordering::Relaxed);
            }
        } else {
            LAST_EXIT.store(0, Ordering::Relaxed);
        }
    }

    0
}

/// カスタムコマンドフック
#[no_mangle]
pub extern "C" fn nexterm_on_command(cmd_ptr: *const u8, cmd_len: usize) -> i32 {
    // SAFETY: サーバーは有効な UTF-8 バッファを渡す
    let bytes = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len) };
    let cmd = std::str::from_utf8(bytes).unwrap_or("").trim();

    match cmd {
        ":count-show" => {
            let count = CMD_COUNT.load(Ordering::Relaxed);
            let exit = LAST_EXIT.load(Ordering::Relaxed);
            let msg = format!(
                "\x1b[36m[command-counter] 実行回数: {}  最終終了コード: {}\x1b[0m\r\n",
                count,
                if exit < 0 {
                    "未記録".to_string()
                } else {
                    exit.to_string()
                }
            );
            // pane_id=0 はフォーカスペインを意味する（ランタイムで解決）
            write_pane(0, &msg);
            0
        }
        ":count-reset" => {
            CMD_COUNT.store(0, Ordering::Relaxed);
            LAST_EXIT.store(-1, Ordering::Relaxed);
            log("[command-counter] カウンターをリセットしました");
            0
        }
        _ => 1,
    }
}
