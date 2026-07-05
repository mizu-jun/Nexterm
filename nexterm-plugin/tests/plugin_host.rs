//! Plugin host end-to-end integration tests (roadmap F2).
//!
//! These tests exercise the full `load -> dispatch -> unload` lifecycle of
//! [`PluginManager`] against **real WASM modules**. The existing in-crate unit
//! tests only cover invalid/empty modules and the no-plugin path, so the actual
//! hook execution surface (linear-memory writes, fuel metering, v2 input
//! sanitization, the `write_pane` allow list, API-version detection, the
//! suppress return value, and v1/v2 divergence) was previously untested.
//!
//! Fixtures are authored as WAT (WebAssembly Text) and compiled to real WASM
//! binaries at test time via `wat::parse_str`, so no wasm32 build toolchain is
//! required on the CI matrix.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nexterm_plugin::{PluginManager, ReadFn, ReadKind, ReadOutcome, WritePaneFn};

// ── WAT fixtures ────────────────────────────────────────────────────────────

/// v2 echo plugin: forwards the received bytes back to the **passed** pane via
/// `write_pane`, so the allow list permits the write. `on_command` returns
/// unhandled (1).
const ECHO_V2: &str = r#"
(module
  (import "nexterm" "write_pane" (func $wp (param i32 i32 i32)))
  (memory (export "memory") 2)
  (func (export "nexterm_api_version") (result i32) (i32.const 2))
  (func (export "nexterm_on_output") (param $pane i32) (param $ptr i32) (param $len i32) (result i32)
    (call $wp (local.get $pane) (local.get $ptr) (local.get $len))
    (i32.const 0))
  (func (export "nexterm_on_command") (param $ptr i32) (param $len i32) (result i32)
    (i32.const 1)))
"#;

/// v2 plugin whose `on_output` returns 1 (suppress).
const SUPPRESS_V2: &str = r#"
(module
  (memory (export "memory") 2)
  (func (export "nexterm_api_version") (result i32) (i32.const 2))
  (func (export "nexterm_on_output") (param i32 i32 i32) (result i32) (i32.const 1)))
"#;

/// v2 plugin whose `on_command` returns 0 (handled).
const COMMAND_V2: &str = r#"
(module
  (memory (export "memory") 2)
  (func (export "nexterm_api_version") (result i32) (i32.const 2))
  (func (export "nexterm_on_command") (param i32 i32) (result i32) (i32.const 0)))
"#;

/// v2 plugin whose `on_command` handles the command (returns 0) but also
/// attempts a `write_pane`. During a command hook the allow list is empty, so
/// the write must be denied regardless of the pane id.
const COMMAND_WRITE_V2: &str = r#"
(module
  (import "nexterm" "write_pane" (func $wp (param i32 i32 i32)))
  (memory (export "memory") 2)
  (func (export "nexterm_api_version") (result i32) (i32.const 2))
  (func (export "nexterm_on_command") (param $ptr i32) (param $len i32) (result i32)
    (call $wp (i32.const 1) (local.get $ptr) (local.get $len))
    (i32.const 0)))
"#;

/// v2 plugin that tries to write to a **different** pane (999) than the one it
/// was invoked for. Under v2 the allow list must deny this write.
const WRONG_PANE_V2: &str = r#"
(module
  (import "nexterm" "write_pane" (func $wp (param i32 i32 i32)))
  (memory (export "memory") 2)
  (func (export "nexterm_api_version") (result i32) (i32.const 2))
  (func (export "nexterm_on_output") (param $pane i32) (param $ptr i32) (param $len i32) (result i32)
    (call $wp (i32.const 999) (local.get $ptr) (local.get $len))
    (i32.const 0)))
"#;

/// v1 legacy plugin (no `nexterm_api_version` export). It echoes the received
/// bytes to an arbitrary pane (999); under v1 there is no allow-list check and
/// the data is delivered **unsanitized**.
const ECHO_V1: &str = r#"
(module
  (import "nexterm" "write_pane" (func $wp (param i32 i32 i32)))
  (memory (export "memory") 2)
  (func (export "nexterm_on_output") (param $pane i32) (param $ptr i32) (param $len i32) (result i32)
    (call $wp (i32.const 999) (local.get $ptr) (local.get $len))
    (i32.const 0)))
"#;

/// v2 plugin whose `on_output` loops forever, exhausting the per-call fuel
/// budget. The host must trap it and survive.
const INFINITE_LOOP_V2: &str = r#"
(module
  (memory (export "memory") 2)
  (func (export "nexterm_api_version") (result i32) (i32.const 2))
  (func (export "nexterm_on_output") (param i32 i32 i32) (result i32)
    (loop $l (br $l))
    (i32.const 0)))
"#;

/// v2 plugin that publishes name "demo" / version "1.0" via `nexterm_meta`.
/// The host zeroes the buffers before the call, so no trailing NUL is written.
const META_V2: &str = r#"
(module
  (memory (export "memory") 2)
  (func (export "nexterm_api_version") (result i32) (i32.const 2))
  (func (export "nexterm_meta") (param $np i32) (param $nl i32) (param $vp i32) (param $vl i32) (result i32)
    (i32.store8 (local.get $np) (i32.const 100))                          ;; 'd'
    (i32.store8 (i32.add (local.get $np) (i32.const 1)) (i32.const 101))  ;; 'e'
    (i32.store8 (i32.add (local.get $np) (i32.const 2)) (i32.const 109))  ;; 'm'
    (i32.store8 (i32.add (local.get $np) (i32.const 3)) (i32.const 111))  ;; 'o'
    (i32.store8 (local.get $vp) (i32.const 49))                           ;; '1'
    (i32.store8 (i32.add (local.get $vp) (i32.const 1)) (i32.const 46))   ;; '.'
    (i32.store8 (i32.add (local.get $vp) (i32.const 2)) (i32.const 48))   ;; '0'
    (i32.const 0)))
"#;

// ── Helpers ───────────────────────────────────────────────────────────────

/// Compile a WAT fixture to WASM and write it to `<dir>/<name>`, returning the path.
fn write_wasm(dir: &Path, name: &str, wat: &str) -> PathBuf {
    let bytes = wat::parse_str(wat).expect("fixture WAT must compile");
    let path = dir.join(name);
    std::fs::write(&path, &bytes).expect("write WASM fixture");
    path
}

/// A `write_pane` sink that records every `(pane_id, bytes)` call.
type Captured = Arc<Mutex<Vec<(u32, Vec<u8>)>>>;

fn capturing_write_pane() -> (WritePaneFn, Captured) {
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let f: WritePaneFn = Arc::new(move |pane_id: u32, data: &[u8]| {
        sink.lock().unwrap().push((pane_id, data.to_vec()));
    });
    (f, captured)
}

fn noop_write_pane() -> WritePaneFn {
    Arc::new(|_pane_id: u32, _data: &[u8]| {})
}

// ── Lifecycle: load -> list -> reload -> unload ───────────────────────────────

#[test]
fn lifecycle_load_reload_unload() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "echo.wasm", ECHO_V2);

    let mgr = PluginManager::new(noop_write_pane());
    assert_eq!(mgr.plugin_count(), 0);

    mgr.load(&path).expect("load echo plugin");
    assert_eq!(mgr.plugin_count(), 1);
    assert_eq!(mgr.plugin_paths(), vec![path.clone()]);

    // list_info reflects the declared API version.
    let info = mgr.list_info();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].api_version, 2);

    // Reload keeps a single instance registered.
    mgr.reload(&path).expect("reload echo plugin");
    assert_eq!(mgr.plugin_count(), 1);

    // Unload removes it; a second unload reports "not found".
    assert!(mgr.unload(&path).expect("unload"));
    assert_eq!(mgr.plugin_count(), 0);
    assert!(!mgr.unload(&path).expect("second unload"));
}

#[test]
fn load_dir_loads_all_wasm_fixtures() {
    let dir = tempfile::tempdir().unwrap();
    write_wasm(dir.path(), "a.wasm", ECHO_V2);
    write_wasm(dir.path(), "b.wasm", SUPPRESS_V2);
    // A non-wasm file must be ignored.
    std::fs::write(dir.path().join("notes.txt"), b"ignore me").unwrap();

    let mgr = PluginManager::new(noop_write_pane());
    let count = mgr.load_dir(dir.path()).expect("load_dir");
    assert_eq!(count, 2);
    assert_eq!(mgr.plugin_count(), 2);
}

// ── Dispatch: on_output reaches write_pane ────────────────────────────────────

#[test]
fn on_output_echoes_bytes_to_allowed_pane() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "echo.wasm", ECHO_V2);

    let (write_pane, captured) = capturing_write_pane();
    let mgr = PluginManager::new(write_pane);
    mgr.load(&path).unwrap();

    let suppressed = mgr.on_output(7, b"hello");
    assert!(!suppressed, "echo plugin returns 0 (pass through)");

    let calls = captured.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, 7, "write must target the invoked pane");
    assert_eq!(calls[0].1, b"hello");
}

// ── v2 input sanitization ─────────────────────────────────────────────────────

#[test]
fn v2_plugin_receives_sanitized_input() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "echo.wasm", ECHO_V2);

    let (write_pane, captured) = capturing_write_pane();
    let mgr = PluginManager::new(write_pane);
    mgr.load(&path).unwrap();

    // "a" + SGR red CSI + "b" + reset CSI. v2 strips the escape sequences.
    mgr.on_output(1, b"a\x1b[31mb\x1b[0m");

    let calls = captured.lock().unwrap();
    assert_eq!(calls[0].1, b"ab", "v2 delivers sanitized bytes only");
}

// ── v2 write_pane allow list ──────────────────────────────────────────────────

#[test]
fn v2_write_to_other_pane_is_denied() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "wrong.wasm", WRONG_PANE_V2);

    let (write_pane, captured) = capturing_write_pane();
    let mgr = PluginManager::new(write_pane);
    mgr.load(&path).unwrap();

    // Plugin was invoked for pane 1 but tries to write to pane 999.
    mgr.on_output(1, b"data");

    let calls = captured.lock().unwrap();
    assert!(
        calls.is_empty(),
        "v2 must deny writes to a pane outside the current allow list"
    );
}

// ── Suppress return value ─────────────────────────────────────────────────────

#[test]
fn on_output_returns_true_when_plugin_suppresses() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "suppress.wasm", SUPPRESS_V2);

    let mgr = PluginManager::new(noop_write_pane());
    mgr.load(&path).unwrap();

    assert!(
        mgr.on_output(1, b"x"),
        "on_output must return true (suppress) when the plugin returns 1"
    );
}

// ── Custom command hook ───────────────────────────────────────────────────────

#[test]
fn on_command_returns_true_when_handled() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "command.wasm", COMMAND_V2);

    let mgr = PluginManager::new(noop_write_pane());
    mgr.load(&path).unwrap();

    assert!(
        mgr.on_command(":greet world"),
        "on_command must return true when a plugin returns 0 (handled)"
    );
}

#[test]
fn on_command_returns_false_when_unhandled() {
    let dir = tempfile::tempdir().unwrap();
    // ECHO_V2's on_command returns 1 (unhandled).
    let path = write_wasm(dir.path(), "echo.wasm", ECHO_V2);

    let mgr = PluginManager::new(noop_write_pane());
    mgr.load(&path).unwrap();

    assert!(
        !mgr.on_command(":unknown"),
        "on_command must return false when every plugin returns 1 (unhandled)"
    );
}

#[test]
fn command_hook_cannot_write_to_any_pane() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "cmdwrite.wasm", COMMAND_WRITE_V2);

    let (write_pane, captured) = capturing_write_pane();
    let mgr = PluginManager::new(write_pane);
    mgr.load(&path).unwrap();

    // The command is handled (return 0), but the write must be denied because
    // the allow list is empty during a command hook.
    assert!(mgr.on_command(":do-something"));
    assert!(
        captured.lock().unwrap().is_empty(),
        "write_pane must be denied inside a command hook (empty allow list)"
    );
}

// ── v1 legacy behavior ────────────────────────────────────────────────────────

#[test]
fn v1_plugin_receives_raw_bytes_and_bypasses_allow_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "legacy.wasm", ECHO_V1);

    let (write_pane, captured) = capturing_write_pane();
    let mgr = PluginManager::new(write_pane);
    mgr.load(&path).unwrap();

    // v1 detection: no nexterm_api_version export -> api_version == 1.
    assert_eq!(mgr.list_info()[0].api_version, 1);

    let raw = b"a\x1b[31mb\x1b[0m";
    mgr.on_output(1, raw);

    let calls = captured.lock().unwrap();
    assert_eq!(calls.len(), 1, "v1 has no allow-list restriction");
    // Writes to pane 999 (not the invoked pane) are permitted under v1.
    assert_eq!(calls[0].0, 999);
    // v1 receives the unsanitized bytes verbatim.
    assert_eq!(calls[0].1, raw);
}

// ── Fuel exhaustion ───────────────────────────────────────────────────────────

#[test]
fn infinite_loop_plugin_is_trapped_and_host_survives() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "loop.wasm", INFINITE_LOOP_V2);

    let mgr = PluginManager::new(noop_write_pane());
    mgr.load(&path).unwrap();

    // The call must return (fuel exhausted -> trapped -> error swallowed),
    // not hang or panic. A non-suppress result (false) is expected because
    // the plugin never returned 1.
    let suppressed = mgr.on_output(1, b"trigger");
    assert!(!suppressed);

    // The host is still usable afterwards.
    assert_eq!(mgr.plugin_count(), 1);
}

// ── Metadata ──────────────────────────────────────────────────────────────────

#[test]
fn plugin_meta_name_and_version_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "meta.wasm", META_V2);

    let mgr = PluginManager::new(noop_write_pane());
    mgr.load(&path).unwrap();

    let info = mgr.list_info();
    assert_eq!(info[0].name.as_deref(), Some("demo"));
    assert_eq!(info[0].version.as_deref(), Some("1.0"));
}

// ── Multiple plugins in a chain ───────────────────────────────────────────────

#[test]
fn suppressing_plugin_short_circuits_the_chain() {
    let dir = tempfile::tempdir().unwrap();
    let suppress = write_wasm(dir.path(), "suppress.wasm", SUPPRESS_V2);
    let echo = write_wasm(dir.path(), "echo.wasm", ECHO_V2);

    let (write_pane, captured) = capturing_write_pane();
    let mgr = PluginManager::new(write_pane);
    // Load the suppressor first so it runs before the echo plugin.
    mgr.load(&suppress).unwrap();
    mgr.load(&echo).unwrap();

    assert!(mgr.on_output(1, b"hi"), "chain suppressed by first plugin");
    // The echo plugin never ran, so nothing was written.
    assert!(captured.lock().unwrap().is_empty());
}

// ── v3 read API (F3 / ADR-0008) ───────────────────────────────────────────────

/// v3 plugin: reads the visible pane text and echoes the result back via
/// `write_pane` so the test can observe what `read_pane` returned. Memory:
/// input at 64 KiB (host), read buffer at 128 KiB (page 2), 3 pages total.
const READ_PANE_ECHO_V3: &str = r#"
(module
  (import "nexterm" "write_pane" (func $wp (param i32 i32 i32)))
  (import "nexterm" "read_pane" (func $rp (param i32 i32 i32) (result i32)))
  (memory (export "memory") 3)
  (func (export "nexterm_api_version") (result i32) (i32.const 3))
  (func (export "nexterm_on_output") (param $pane i32) (param $ptr i32) (param $len i32) (result i32)
    (local $n i32)
    (local.set $n (call $rp (local.get $pane) (i32.const 131072) (i32.const 4096)))
    (if (i32.gt_s (local.get $n) (i32.const 0))
      (then (call $wp (local.get $pane) (i32.const 131072) (local.get $n))))
    (i32.const 0)))
"#;

/// v3 plugin: same as above but reads the structured grid dump.
const READ_GRID_ECHO_V3: &str = r#"
(module
  (import "nexterm" "write_pane" (func $wp (param i32 i32 i32)))
  (import "nexterm" "read_grid" (func $rg (param i32 i32 i32) (result i32)))
  (memory (export "memory") 3)
  (func (export "nexterm_api_version") (result i32) (i32.const 3))
  (func (export "nexterm_on_output") (param $pane i32) (param $ptr i32) (param $len i32) (result i32)
    (local $n i32)
    (local.set $n (call $rg (local.get $pane) (i32.const 131072) (i32.const 4096)))
    (if (i32.gt_s (local.get $n) (i32.const 0))
      (then (call $wp (local.get $pane) (i32.const 131072) (local.get $n))))
    (i32.const 0)))
"#;

/// v3 plugin: reads scrollback (start_line=0, max_lines=100) and echoes it.
const READ_SCROLLBACK_ECHO_V3: &str = r#"
(module
  (import "nexterm" "write_pane" (func $wp (param i32 i32 i32)))
  (import "nexterm" "read_scrollback" (func $rs (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 3)
  (func (export "nexterm_api_version") (result i32) (i32.const 3))
  (func (export "nexterm_on_output") (param $pane i32) (param $ptr i32) (param $len i32) (result i32)
    (local $n i32)
    (local.set $n (call $rs (local.get $pane) (i32.const 0) (i32.const 100) (i32.const 131072) (i32.const 4096)))
    (if (i32.gt_s (local.get $n) (i32.const 0))
      (then (call $wp (local.get $pane) (i32.const 131072) (local.get $n))))
    (i32.const 0)))
"#;

/// A read callback that returns a distinct marker per read kind.
fn marker_read_fn() -> ReadFn {
    Arc::new(|_pane_id: u32, kind: ReadKind| match kind {
        ReadKind::PaneText => ReadOutcome::Data(b"PANE-TEXT".to_vec()),
        ReadKind::Grid => ReadOutcome::Data(b"GRID-DUMP".to_vec()),
        ReadKind::Scrollback { .. } => ReadOutcome::Data(b"SCROLLBACK".to_vec()),
    })
}

#[test]
fn v3_read_pane_returns_data_to_the_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "readpane.wasm", READ_PANE_ECHO_V3);

    let (write_pane, captured) = capturing_write_pane();
    let mut mgr = PluginManager::new(write_pane);
    mgr.set_read_fn(marker_read_fn());
    mgr.load(&path).unwrap();

    mgr.on_output(7, b"trigger");

    let calls = captured.lock().unwrap();
    assert_eq!(calls.len(), 1, "plugin should echo the read result");
    assert_eq!(calls[0].0, 7);
    assert_eq!(calls[0].1, b"PANE-TEXT");
}

#[test]
fn v3_read_grid_and_scrollback_return_data() {
    let dir = tempfile::tempdir().unwrap();
    let grid_path = write_wasm(dir.path(), "readgrid.wasm", READ_GRID_ECHO_V3);
    let sb_path = write_wasm(dir.path(), "readsb.wasm", READ_SCROLLBACK_ECHO_V3);

    // read_grid
    {
        let (write_pane, captured) = capturing_write_pane();
        let mut mgr = PluginManager::new(write_pane);
        mgr.set_read_fn(marker_read_fn());
        mgr.load(&grid_path).unwrap();
        mgr.on_output(1, b"x");
        assert_eq!(captured.lock().unwrap()[0].1, b"GRID-DUMP");
    }
    // read_scrollback
    {
        let (write_pane, captured) = capturing_write_pane();
        let mut mgr = PluginManager::new(write_pane);
        mgr.set_read_fn(marker_read_fn());
        mgr.load(&sb_path).unwrap();
        mgr.on_output(1, b"x");
        assert_eq!(captured.lock().unwrap()[0].1, b"SCROLLBACK");
    }
}

#[test]
fn v3_read_denied_by_default_policy() {
    // No set_read_fn installed → the default callback denies every read, so
    // read_pane returns -4 and the plugin echoes nothing.
    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "readpane.wasm", READ_PANE_ECHO_V3);

    let (write_pane, captured) = capturing_write_pane();
    let mgr = PluginManager::new(write_pane);
    mgr.load(&path).unwrap();

    mgr.on_output(7, b"trigger");
    assert!(
        captured.lock().unwrap().is_empty(),
        "reads must be denied when no read callback is installed"
    );
}

#[test]
fn v3_read_callback_receives_the_invoked_pane_id() {
    // The pane id reaching the read callback must be the pane the hook was
    // invoked for (allow-list scoping).
    let seen: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_clone = Arc::clone(&seen);
    let read_fn: ReadFn = Arc::new(move |pane_id: u32, _kind: ReadKind| {
        seen_clone.lock().unwrap().push(pane_id);
        ReadOutcome::Data(b"ok".to_vec())
    });

    let dir = tempfile::tempdir().unwrap();
    let path = write_wasm(dir.path(), "readpane.wasm", READ_PANE_ECHO_V3);
    let mut mgr = PluginManager::new(noop_write_pane());
    mgr.set_read_fn(read_fn);
    mgr.load(&path).unwrap();

    mgr.on_output(42, b"x");
    assert_eq!(*seen.lock().unwrap(), vec![42]);
}
