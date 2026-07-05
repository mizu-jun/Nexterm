//! screen-digest — demonstrates the v3 read API (`read_pane`, F3 / ADR-0008).
//!
//! On each command completion (OSC 133 `D` semantic mark) the plugin reads the
//! *visible* pane text via `read_pane` and logs a one-line digest: how many
//! non-blank lines and characters are currently on screen. It never writes to
//! the pane, so there is no output-feedback loop.
//!
//! ## Enabling the read API
//!
//! The v3 read API is gated by the server-side `plugin_read` consent policy,
//! which defaults to **`deny`** (fail-safe). To let this plugin read the pane,
//! opt in explicitly in your config:
//!
//! ```toml
//! [security]
//! plugin_read = "allow"
//! ```
//!
//! While `plugin_read` is `deny` (or `prompt`, which is currently treated as
//! `deny`), `read_pane` returns `-4` and the digest reports nothing.
//!
//! ## Build
//! ```sh
//! cargo build --release --target wasm32-unknown-unknown
//! # output: target/wasm32-unknown-unknown/release/screen_digest.wasm
//! ```
//!
//! ## Install
//! ```sh
//! nexterm-ctl plugin load ./target/wasm32-unknown-unknown/release/screen_digest.wasm
//! ```

// ---- Host imports -----------------------------------------------------------

#[link(wasm_import_module = "nexterm")]
extern "C" {
    /// Write a string to the nexterm log.
    fn log(ptr: *const u8, len: usize);

    /// v3 read API: copy the visible pane text (UTF-8) into `out_ptr`, writing
    /// at most `out_max` bytes. Returns the number of bytes written (>= 0) or a
    /// negative error code: -1 wrong ABI, -2 unknown/out-of-scope pane,
    /// -3 buffer too small, -4 disabled by the `plugin_read` policy.
    fn read_pane(pane_id: i32, out_ptr: *mut u8, out_max: usize) -> i32;
}

fn host_log(msg: &str) {
    let b = msg.as_bytes();
    // SAFETY: ptr/len come from a live byte slice.
    unsafe { log(b.as_ptr(), b.len()) };
}

// ---- Read buffer ------------------------------------------------------------

/// Scratch buffer for `read_pane` output, kept out of the 64 KiB region the
/// host uses to pass the hook payload. 32 KiB comfortably holds a screenful of
/// text; the host truncates anything larger at a UTF-8 boundary.
const READ_BUF_LEN: usize = 32 * 1024;
static mut READ_BUF: [u8; READ_BUF_LEN] = [0; READ_BUF_LEN];

// ---- Exports ----------------------------------------------------------------

/// Declare the plugin ABI version. v3 unlocks the read host imports
/// (`read_pane` / `read_grid` / `read_scrollback`).
#[no_mangle]
pub extern "C" fn nexterm_api_version() -> i32 {
    3
}

#[no_mangle]
pub extern "C" fn nexterm_init() {
    host_log("[screen-digest] initialized (requires security.plugin_read = \"allow\")");
}

/// Pane output hook.
///
/// ABI: the host passes `(pane_id, ptr, len)` — `pane_id` first (see
/// `PluginManager::on_output`). During this call `pane_id` is the only pane the
/// read API is scoped to.
///
/// # Returns
/// 0 = pass the output through unchanged (this plugin never suppresses).
// The WASM export ABI passes raw pointers, so the pointer args cannot be
// references; the host guarantees they are valid for `output_len` bytes.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nexterm_on_output(pane_id: i32, output_ptr: *const u8, output_len: usize) -> i32 {
    // SAFETY: the host passes a valid UTF-8 buffer of `output_len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let text = std::str::from_utf8(bytes).unwrap_or("");

    // Only act when a command finishes: OSC 133 D (`\x1b]133;D...`).
    if !text.contains("\x1b]133;D") {
        return 0;
    }

    // Read the visible pane text. `pane_id` is the pane this hook fired for, so
    // it is within the read API's per-hook allow-list. `&raw mut` yields a raw
    // pointer without forming a reference to the mutable static.
    let buf_ptr = (&raw mut READ_BUF) as *mut u8;
    // SAFETY: READ_BUF is a valid, statically-sized buffer; the host writes at
    // most READ_BUF_LEN bytes and returns the count.
    let n = unsafe { read_pane(pane_id, buf_ptr, READ_BUF_LEN) };

    match n {
        -4 => host_log("[screen-digest] read denied: set security.plugin_read = \"allow\""),
        n if n < 0 => host_log(&format!("[screen-digest] read_pane failed (code {n})")),
        n => {
            let len = n as usize;
            let read_ptr = (&raw const READ_BUF) as *const u8;
            // SAFETY: the host wrote exactly `len` valid bytes into READ_BUF.
            let screen = unsafe { std::slice::from_raw_parts(read_ptr, len) };
            let screen = std::str::from_utf8(screen).unwrap_or("");
            let non_blank = screen.lines().filter(|l| !l.trim().is_empty()).count();
            let chars = screen.chars().count();
            host_log(&format!(
                "[screen-digest] pane {pane_id}: {non_blank} non-blank lines, {chars} chars on screen"
            ));
        }
    }

    0
}

/// Custom command hook — this plugin has none.
#[no_mangle]
pub extern "C" fn nexterm_on_command(_cmd_ptr: *const u8, _cmd_len: usize) -> i32 {
    1 // not handled
}
