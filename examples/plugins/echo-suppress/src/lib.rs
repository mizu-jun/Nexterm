//! echo-suppress — `nexterm_meta` / `api_version` の実装例を示すサンプルプラグイン。
//!
//! このプラグインは以下をデモする:
//! - `nexterm_meta` エクスポートでプラグイン名とバージョンを公開する
//! - `nexterm.api_version()` インポートでホストとの互換性を確認する
//! - `nexterm_on_output` で `^` で始まる行（シェルの echo コマンドの典型出力）を抑制する
//!
//! ## ビルド
//! ```sh
//! cargo build --release --target wasm32-unknown-unknown
//! # 出力: target/wasm32-unknown-unknown/release/echo_suppress.wasm
//! ```
//!
//! ## インストール
//! ```sh
//! nexterm-ctl plugin load ./target/wasm32-unknown-unknown/release/echo_suppress.wasm
//! ```

// ---- ホストインポート -------------------------------------------------------

#[link(wasm_import_module = "nexterm")]
extern "C" {
    /// ホストのログに文字列を書き込む
    fn log(ptr: *const u8, len: usize);

    /// ホストの Plugin ABI バージョンを返す
    fn api_version() -> i32;
}

fn host_log(msg: &str) {
    let b = msg.as_bytes();
    // SAFETY: ptr/len は有効な bytes スライスから取得する
    unsafe { log(b.as_ptr(), b.len()) };
}

// ---- メタデータ文字列（静的定数） -------------------------------------------

static PLUGIN_NAME: &[u8] = b"echo-suppress\0";
static PLUGIN_VERSION: &[u8] = b"0.1.0\0";

// ---- エクスポート関数 -------------------------------------------------------

/// Plugin ABI バージョンを宣言する（Sprint 5-4 / F1 で v2 化）。
///
/// ホストはこの値を見て v1 / v2 を判別する。v2 では:
/// - `nexterm_on_output` への `pane_id` 引数が有効値（v1 では 0 で扱われる）
/// - クリップボード書き込み・通知発行はホスト側で許可リスト検証される
/// - `write_pane` 呼び出しは `allowed_panes` に明示登録された pane のみ許可される
#[no_mangle]
pub extern "C" fn nexterm_api_version() -> i32 {
    2
}

/// プラグイン名とバージョンをホストに公開する。
///
/// ホストは name_buf / ver_buf に書き込まれたヌル終端文字列を読み取る。
/// バッファ長は name_max / ver_max で制限される。
#[no_mangle]
pub extern "C" fn nexterm_meta(
    name_buf: *mut u8,
    name_max: usize,
    ver_buf: *mut u8,
    ver_max: usize,
) -> i32 {
    write_cstr(PLUGIN_NAME, name_buf, name_max);
    write_cstr(PLUGIN_VERSION, ver_buf, ver_max);
    0
}

/// ヌル終端バイト列を dst バッファに書き込む（max バイトまで）
fn write_cstr(src: &[u8], dst: *mut u8, max: usize) {
    if dst.is_null() || max == 0 {
        return;
    }
    let copy_len = src.len().min(max - 1);
    // SAFETY: dst はホストが確保した max バイトのバッファへの有効なポインタ
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), dst, copy_len);
        dst.add(copy_len).write(0); // ヌル終端
    }
}

/// プラグイン初期化 — ABI バージョンを確認する
#[no_mangle]
pub extern "C" fn nexterm_init() {
    // SAFETY: api_version() はホストが提供する安全な関数
    let ver = unsafe { api_version() };
    host_log(&format!(
        "[echo-suppress] 初期化完了 (ホスト API バージョン: {})",
        ver
    ));
}

/// PTY 出力フック — `^` で始まる行をすべて抑制する（シェル echo の自動補完表示を除去）
///
/// # Returns
/// 0 = 通過, 1 = 抑制
#[no_mangle]
pub extern "C" fn nexterm_on_output(
    output_ptr: *const u8,
    output_len: usize,
    _pane_id: i32,
) -> i32 {
    // SAFETY: サーバーは有効な UTF-8 バッファへのポインタを渡す
    let bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let text = std::str::from_utf8(bytes).unwrap_or("");

    if text.starts_with('^') {
        host_log("[echo-suppress] ^ で始まる出力を抑制しました");
        return 1; // 抑制
    }

    0 // 通過
}

/// カスタムコマンドフック（このプラグインは独自コマンドなし）
#[no_mangle]
pub extern "C" fn nexterm_on_command(_cmd_ptr: *const u8, _cmd_len: usize) -> i32 {
    1 // 未処理
}
