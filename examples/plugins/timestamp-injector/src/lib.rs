//! timestamp-injector — コマンド出力の先頭行にタイムスタンプを付加する
//! サンプルプラグイン（任意言語 WASM の例として WAT 形式で実装）。
//!
//! このファイルは Rust で書いた参照実装です。
//! 実際には任意の言語（C/Go/Python(wasm) 等）でコンパイルできます。
//!
//! ## ビルド
//! ```sh
//! cargo build --release --target wasm32-unknown-unknown
//! ```
//!
//! ## 登録 (nexterm.toml)
//! ```toml
//! [[plugins]]
//! path = "~/.config/nexterm/plugins/timestamp_injector.wasm"
//! ```
//!
//! ## カスタムコマンド
//! - `:ts-on`  — タイムスタンプ付加を有効化
//! - `:ts-off` — タイムスタンプ付加を無効化

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// タイムスタンプ付加が有効かどうか
static ENABLED: AtomicBool = AtomicBool::new(false);
/// 最後にタイムスタンプを付加した時刻（Unix ミリ秒）
/// WASM では std::time が使えないため nexterm_now インポートで取得する
static LAST_TS: AtomicU64 = AtomicU64::new(0);

extern "C" {
    fn nexterm_log(ptr: *const u8, len: usize);
    fn nexterm_write_pane(pane_id: i32, ptr: *const u8, len: usize);
    /// ホストから現在時刻（Unix ミリ秒）を取得するインポート関数
    /// nexterm-plugin ランタイムが提供する
    fn nexterm_now_ms() -> u64;
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

fn now_ms() -> u64 {
    // SAFETY: ホストが提供する副作用のない関数
    unsafe { nexterm_now_ms() }
}

/// Unix ミリ秒を "HH:MM:SS.mmm" 形式に変換するヘルパー
fn format_time(ms: u64) -> String {
    let total_sec = ms / 1000;
    let millis = ms % 1000;
    let sec = total_sec % 60;
    let min = (total_sec / 60) % 60;
    let hour = (total_sec / 3600) % 24;
    format!("{:02}:{:02}:{:02}.{:03}", hour, min, sec, millis)
}

#[no_mangle]
pub extern "C" fn nexterm_init() {
    log("[timestamp-injector] 初期化完了（デフォルト: 無効）");
}

/// PTY 出力フック
///
/// 有効な場合は各行の先頭に "HH:MM:SS.mmm | " を付加する。
/// 前回のタイムスタンプから 100ms 未満の場合は付加しない（ちらつき防止）。
#[no_mangle]
pub extern "C" fn nexterm_on_output(output_ptr: *const u8, output_len: usize, pane_id: i32) -> i32 {
    if !ENABLED.load(Ordering::Relaxed) {
        return 0;
    }

    // SAFETY: サーバーは有効な UTF-8 バッファを渡す
    let bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let text = std::str::from_utf8(bytes).unwrap_or("");

    let now = now_ms();
    let last = LAST_TS.load(Ordering::Relaxed);
    // 100ms 未満はスキップしてちらつきを抑える
    if now.saturating_sub(last) < 100 {
        return 0;
    }
    LAST_TS.store(now, Ordering::Relaxed);

    let ts = format_time(now);
    // 各行にタイムスタンプを付加する
    let annotated: String = text
        .lines()
        .map(|line| {
            if line.is_empty() {
                line.to_string()
            } else {
                format!("\x1b[90m{} |\x1b[0m {}", ts, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");

    if !annotated.is_empty() {
        write_pane(pane_id, &annotated);
        // 元の出力を抑制して置換する
        return 1;
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
        ":ts-on" => {
            ENABLED.store(true, Ordering::Relaxed);
            log("[timestamp-injector] タイムスタンプ付加を有効化しました");
            0
        }
        ":ts-off" => {
            ENABLED.store(false, Ordering::Relaxed);
            log("[timestamp-injector] タイムスタンプ付加を無効化しました");
            0
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_変換が正しい() {
        // 3661234 ms = 1時間1分1秒234ミリ秒
        assert_eq!(format_time(3_661_234), "01:01:01.234");
        assert_eq!(format_time(0), "00:00:00.000");
        assert_eq!(format_time(86_399_999), "23:59:59.999");
    }
}
