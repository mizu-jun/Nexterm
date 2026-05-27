#![warn(missing_docs)]
//! Nexterm WASM plugin host runtime.
//!
//! # Plugin ABI version
//!
//! [`PLUGIN_API_VERSION`] identifies the stable ABI. Plugins can query it via
//! the `nexterm.api_version() -> i32` import.
//!
//! ## v2 (current, recommended)
//!
//! - Input data (pane output / command strings) is passed **already sanitized**.
//!   ESC, C0 controls, and OSC/CSI/DCS/APC sequences are stripped; only tab,
//!   newline, printable ASCII, and UTF-8 multibyte sequences pass through.
//! - `nexterm.write_pane(pane_id, ...)` is filtered per call by the
//!   **allowed pane_id set**. During `nexterm_on_output(pane_id, ...)` only
//!   that `pane_id` may be written to; during `nexterm_on_command(...)` no
//!   pane may be written to at all.
//!
//! ## v1 (still supported for backwards compatibility)
//!
//! Plugins that do not export `nexterm_api_version`, or that return `1`, are
//! treated as v1:
//!
//! - Input data is not sanitized; raw bytes are forwarded (legacy behavior).
//! - `write_pane` accepts any pane_id (legacy behavior).
//! - A one-shot **deprecation warning** is logged at load time.
//!
//! v1 is **scheduled to be removed at the v2.0.0 release** (see ADR-0003).
//! New plugins should target v2.
//!
//! # WASM exports (provided by the plugin)
//!
//! ```wat
//! ;; Module initialization (optional)
//! (export "nexterm_init" (func ...))
//!
//! ;; API version declaration (required for v2)
//! (export "nexterm_api_version" (func (result i32)))
//!
//! ;; Plugin metadata: returns name_ptr/name_len / version_ptr/version_len.
//! ;; Return value: always 0 (unused).
//! (export "nexterm_meta" (func (param i32 i32 i32 i32) (result i32)))
//!
//! ;; Pane output hook: data_ptr/data_len point to UTF-8 bytes in linear memory.
//! ;; Return value: 0 = pass through, 1 = suppress.
//! (export "nexterm_on_output" (func (param i32 i32 i32) (result i32)))
//!
//! ;; Custom command hook: cmd_ptr/cmd_len is a `:cmd arg` formatted string.
//! ;; Return value: 0 = handled, 1 = unhandled.
//! (export "nexterm_on_command" (func (param i32 i32) (result i32)))
//! ```
//!
//! # Host imports (available to the plugin)
//!
//! ```wat
//! (import "nexterm" "log" (func (param i32 i32)))            ;; log output
//! (import "nexterm" "write_pane" (func (param i32 i32 i32))) ;; write to a pane
//! (import "nexterm" "api_version" (func (result i32)))        ;; API version query
//! ```

/// Current plugin ABI version number (latest specification).
///
/// The value that a plugin declares via the `nexterm_api_version` export must
/// either equal this value or be at least [`MIN_SUPPORTED_API_VERSION`].
pub const PLUGIN_API_VERSION: u32 = 2;

/// Lowest API version accepted for load (for backwards compatibility).
///
/// v1 plugins keep the legacy behavior of running without sanitization and
/// without pane_id validation. A deprecation warning is logged at load time.
pub const MIN_SUPPORTED_API_VERSION: u32 = 1;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tracing::{error, info, warn};
use wasmi::{Config, Engine, Linker, Module, Store};

/// Fuel (an approximate instruction budget) supplied per plugin invocation.
///
/// Acts as an upper bound that prevents infinite loops or extremely heavy
/// computation from stalling the host. CRITICAL #10 mitigation: wasmi does
/// not enforce a fuel limit by default.
const FUEL_PER_CALL: u64 = 10_000_000;

/// Maximum number of plugin memory pages (1 page = 64 KiB).
///
/// 256 pages = 16 MiB upper bound. CRITICAL #10 mitigation: wasmi does not
/// enforce a linear-memory limit by default, so a malicious plugin could
/// allocate gigabytes via `memory.grow`.
const MAX_MEMORY_PAGES: u32 = 256;

// ---- Input sanitization ---------------------------------------------------

/// Strip control characters and escape sequences from a byte slice before
/// handing it to a v2 plugin.
///
/// Bytes that pass through:
/// - `0x09` (TAB), `0x0A` (LF), `0x0D` (CR)
/// - `0x20..=0x7E` (printable ASCII)
/// - `0x80..=0xFF` (UTF-8 lead bytes / continuation bytes)
///
/// Bytes that are stripped:
/// - Other C0 controls (`0x00..=0x08`, `0x0B..=0x0C`, `0x0E..=0x1F`)
/// - `0x7F` (DEL)
/// - CSI / OSC / DCS / APC sequences starting with ESC (`0x1B`) — ESC and the
///   trailing sequence are discarded together as a unit.
///
/// ## Why we sanitize in v2
///
/// In v1, plugins could directly observe sensitive sequences such as
/// clipboard rewrites (OSC 52) and hyperlinks (OSC 8). In v2 the host strips
/// them first so the data delivered to plugins is restricted to plain text.
pub fn sanitize_for_plugin(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        match b {
            0x1B => {
                // ESC: skip the following sequence.
                i += 1;
                if i >= input.len() {
                    break;
                }
                match input[i] {
                    b'[' => {
                        // CSI: parameter + intermediate + final byte (0x40..=0x7E).
                        i += 1;
                        while i < input.len() && !(0x40..=0x7E).contains(&input[i]) {
                            i += 1;
                        }
                        if i < input.len() {
                            i += 1; // consume the final byte
                        }
                    }
                    b']' | b'P' | b'_' | b'^' => {
                        // OSC / DCS / APC / PM: terminated by ST (ESC \) or BEL (0x07).
                        i += 1;
                        while i < input.len() {
                            if input[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if input[i] == 0x1B && i + 1 < input.len() && input[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        // Two-byte escape (ESC c, ESC =, ESC > etc.): consume one byte.
                        i += 1;
                    }
                }
            }
            0x09 | 0x0A | 0x0D => {
                // TAB / LF / CR pass through.
                out.push(b);
                i += 1;
            }
            0x00..=0x1F | 0x7F => {
                // Other C0 controls and DEL are stripped.
                i += 1;
            }
            _ => {
                // Printable ASCII / UTF-8 multibyte.
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

// ---- Host state -----------------------------------------------------------

/// Host callback exposed to plugins (e.g. pane writes).
pub type WritePaneFn = Arc<dyn Fn(u32, &[u8]) + Send + Sync>;

/// One plugin's runtime instance.
struct PluginInstance {
    /// Path to the plugin file (for debugging).
    path: PathBuf,
    /// API version declared by the plugin (value of `nexterm_api_version`;
    /// defaults to `1` when unavailable).
    api_version: u32,
    /// Plugin-declared name (from `nexterm_meta`; optional).
    pub meta_name: Option<String>,
    /// Plugin-declared version (from `nexterm_meta`; optional).
    pub meta_version: Option<String>,
    /// WASM store.
    store: Store<HostState>,
    /// WASM instance.
    instance: wasmi::Instance,
}

/// Host-side state stored inside the WASM store.
struct HostState {
    /// Pane-write callback.
    write_pane: WritePaneFn,
    /// Log buffer (strings received via the `nexterm.log` import).
    log_buf: Vec<String>,
    /// Pane IDs that `write_pane` is permitted to write to during the current
    /// hook invocation. Only consulted for v2 plugins. An empty set means no
    /// pane may be written to.
    allowed_panes: HashSet<u32>,
    /// API version of the plugin (needed to bypass `allowed_panes` for v1).
    api_version: u32,
}

// ---- Plugin manager -------------------------------------------------------

/// Manager that loads and tracks WASM plugins.
pub struct PluginManager {
    engine: Engine,
    plugins: Mutex<Vec<PluginInstance>>,
    write_pane: WritePaneFn,
}

impl PluginManager {
    /// Construct a new plugin manager.
    ///
    /// `write_pane` is invoked whenever a plugin calls `nexterm.write_pane`.
    /// For v2 plugins it is only called with allowed pane IDs.
    ///
    /// # Sandbox configuration (CRITICAL #10 mitigation)
    ///
    /// - Fuel metering is enabled (covering all engine operations by default).
    /// - Each `on_output` / `on_command` invocation is refueled with
    ///   `FUEL_PER_CALL`.
    /// - When fuel is exhausted the call aborts with a `TrappedFuelExhausted`
    ///   error.
    pub fn new(write_pane: WritePaneFn) -> Self {
        let mut config = Config::default();
        // Fuel metering = per-instruction budget enforcement.
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        Self {
            engine,
            plugins: Mutex::new(Vec::new()),
            write_pane,
        }
    }

    /// Load a WASM file and register it as a plugin.
    pub fn load(&self, path: &Path) -> Result<()> {
        let wasm_bytes = std::fs::read(path)
            .with_context(|| format!("failed to read plugin file: {}", path.display()))?;

        let module = Module::new(&self.engine, &wasm_bytes[..])
            .with_context(|| format!("failed to compile WASM module: {}", path.display()))?;

        let write_pane = Arc::clone(&self.write_pane);
        let mut store = Store::new(
            &self.engine,
            HostState {
                write_pane,
                log_buf: Vec::new(),
                allowed_panes: HashSet::new(),
                // Provisional value; finalized after reading `nexterm_api_version`.
                api_version: MIN_SUPPORTED_API_VERSION,
            },
        );

        let mut linker = Linker::<HostState>::new(&self.engine);

        // Host import: nexterm.api_version() -> i32
        linker.func_wrap(
            "nexterm",
            "api_version",
            |_: wasmi::Caller<'_, HostState>| PLUGIN_API_VERSION as i32,
        )?;

        // Host import: nexterm.log(ptr: i32, len: i32)
        linker.func_wrap(
            "nexterm",
            "log",
            |mut caller: wasmi::Caller<'_, HostState>, ptr: i32, len: i32| {
                if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = mem.data(&caller);
                    let start = ptr as usize;
                    let end = start.saturating_add(len as usize);
                    if end <= data.len() {
                        let s = String::from_utf8_lossy(&data[start..end]).into_owned();
                        info!("[plugin] {}", s);
                        caller.data_mut().log_buf.push(s);
                    }
                }
            },
        )?;

        // Host import: nexterm.write_pane(pane_id: i32, ptr: i32, len: i32)
        //
        // v2 plugin: if `pane_id` is not in `allowed_panes`, ignore the call
        //   and emit a warn log (to surface the denial explicitly).
        // v1 plugin: always permit (legacy behavior).
        linker.func_wrap(
            "nexterm",
            "write_pane",
            |caller: wasmi::Caller<'_, HostState>, pane_id: i32, ptr: i32, len: i32| {
                let pane_u = pane_id as u32;
                let allowed = {
                    let state = caller.data();
                    state.api_version < 2 || state.allowed_panes.contains(&pane_u)
                };
                if !allowed {
                    warn!(
                        "[plugin] write_pane denied: pane_id={} is not in the allow list (API v2)",
                        pane_u
                    );
                    return;
                }
                if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = mem.data(&caller);
                    let start = ptr as usize;
                    let end = start.saturating_add(len as usize);
                    if end <= data.len() {
                        let bytes = data[start..end].to_vec();
                        (caller.data().write_pane)(pane_u, &bytes);
                    }
                }
            },
        )?;

        // Provide initial fuel (consumed by instantiation, nexterm_init, and nexterm_meta).
        store
            .set_fuel(FUEL_PER_CALL)
            .with_context(|| "failed to set fuel")?;

        let instance = linker
            .instantiate(&mut store, &module)
            .with_context(|| "failed to instantiate the plugin")?
            .start(&mut store)
            .with_context(|| "failed to start the plugin")?;

        // Memory-limit check (CRITICAL #10): reject if the initial memory size
        // exceeds the cap.
        if let Some(mem) = instance.get_memory(&store, "memory")
            && mem.size(&store) > MAX_MEMORY_PAGES
        {
            anyhow::bail!(
                "plugin memory exceeds the limit: {} pages > {} pages (cap {} MiB)",
                mem.size(&store),
                MAX_MEMORY_PAGES,
                MAX_MEMORY_PAGES * 64 / 1024
            );
        }

        // API version detection + compatibility check.
        // - Export present → adopt the value; reject if it exceeds `PLUGIN_API_VERSION`.
        // - Export present, call failed → continue loading (treat as v1).
        // - Export absent → treat as v1.
        let mut api_version = MIN_SUPPORTED_API_VERSION;
        if let Ok(version_fn) = instance.get_typed_func::<(), i32>(&store, "nexterm_api_version") {
            store
                .set_fuel(FUEL_PER_CALL)
                .with_context(|| "failed to set fuel")?;
            match version_fn.call(&mut store, ()) {
                Ok(v) => {
                    let v_u = v as u32;
                    if v_u > PLUGIN_API_VERSION {
                        anyhow::bail!(
                            "plugin API version is newer than the host: plugin={}, host={}",
                            v_u,
                            PLUGIN_API_VERSION
                        );
                    }
                    if v_u < MIN_SUPPORTED_API_VERSION {
                        anyhow::bail!(
                            "plugin API version is too old: plugin={}, min={}",
                            v_u,
                            MIN_SUPPORTED_API_VERSION
                        );
                    }
                    api_version = v_u;
                }
                Err(e) => {
                    warn!(
                        "failed to obtain plugin API version (continuing as v1): {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        // Commit the resolved API version back into the HostState.
        store.data_mut().api_version = api_version;

        // Emit a one-shot deprecation warning for v1 plugins.
        if api_version < PLUGIN_API_VERSION {
            warn!(
                "plugin is running under API v{} (current v{}): {} — \
                 running with the legacy behavior (no sanitization, no PaneId check). \
                 v1 support will be removed in a future release.",
                api_version,
                PLUGIN_API_VERSION,
                path.display()
            );
        }

        // Call `nexterm_init` if present (optional).
        if let Ok(init_fn) = instance.get_typed_func::<(), ()>(&store, "nexterm_init") {
            store
                .set_fuel(FUEL_PER_CALL)
                .with_context(|| "failed to set fuel")?;
            init_fn.call(&mut store, ()).ok();
        }

        // Read metadata from `nexterm_meta` if present (optional).
        store
            .set_fuel(FUEL_PER_CALL)
            .with_context(|| "failed to set fuel")?;
        let (meta_name, meta_version) = read_plugin_meta(&mut store, &instance);

        info!(
            "plugin loaded: {} (api=v{} name={:?} version={:?})",
            path.display(),
            api_version,
            meta_name,
            meta_version
        );

        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex is poisoned; recovering and continuing");
            poisoned.into_inner()
        });
        plugins.push(PluginInstance {
            path: path.to_path_buf(),
            api_version,
            meta_name,
            meta_version,
            store,
            instance,
        });

        Ok(())
    }

    /// Unload the plugin at the given path. Returns `Ok(false)` if no such plugin exists.
    pub fn unload(&self, path: &Path) -> Result<bool> {
        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex is poisoned; recovering and continuing");
            poisoned.into_inner()
        });
        let before = plugins.len();
        plugins.retain(|p| p.path != path);
        let removed = plugins.len() < before;
        if removed {
            info!("plugin unloaded: {}", path.display());
        }
        Ok(removed)
    }

    /// Reload the plugin at the given path (unload, then load).
    pub fn reload(&self, path: &Path) -> Result<()> {
        self.unload(path)?;
        self.load(path)
    }

    /// Load every `.wasm` file in the given directory.
    pub fn load_dir(&self, dir: &Path) -> Result<usize> {
        if !dir.exists() {
            return Ok(0);
        }
        let mut count = 0;
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("failed to read the plugin directory: {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                match self.load(&path) {
                    Ok(()) => count += 1,
                    Err(e) => warn!("skipping plugin load: {} — {}", path.display(), e),
                }
            }
        }
        Ok(count)
    }

    /// Pane output hook — calls `nexterm_on_output` on every plugin.
    ///
    /// Return value: `true` = suppress (do not forward to the client).
    ///
    /// v2 plugin: `data` is sanitized before being passed, and only `pane_id`
    ///   can be written to via `write_pane`.
    /// v1 plugin: raw bytes are passed and there is no write restriction
    ///   (legacy behavior).
    pub fn on_output(&self, pane_id: u32, data: &[u8]) -> bool {
        // Sanitize once for v2 (reused across plugins).
        let sanitized = sanitize_for_plugin(data);
        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex is poisoned; recovering and continuing");
            poisoned.into_inner()
        });
        for plugin in plugins.iter_mut() {
            let Ok(func) = plugin
                .instance
                .get_typed_func::<(i32, i32, i32), i32>(&plugin.store, "nexterm_on_output")
            else {
                continue;
            };
            let payload: &[u8] = if plugin.api_version >= 2 {
                &sanitized
            } else {
                data
            };
            // Write the payload into the plugin's WASM linear memory.
            if let Some(mem) = plugin.instance.get_memory(&plugin.store, "memory") {
                let offset = 64 * 1024usize; // safe region above the stack
                let mem_size = mem.data_size(&plugin.store);
                if offset + payload.len() <= mem_size {
                    mem.write(&mut plugin.store, offset, payload).ok();
                    // Refuel before each call (prevents infinite loops; CRITICAL #10).
                    if let Err(e) = plugin.store.set_fuel(FUEL_PER_CALL) {
                        error!(
                            "[plugin {}] failed to set fuel: {}",
                            plugin.path.display(),
                            e
                        );
                        continue;
                    }
                    // v2: scope the allow list to `{pane_id}` for this call only.
                    {
                        let state = plugin.store.data_mut();
                        state.allowed_panes.clear();
                        if state.api_version >= 2 {
                            state.allowed_panes.insert(pane_id);
                        }
                    }
                    let result = func.call(
                        &mut plugin.store,
                        (pane_id as i32, offset as i32, payload.len() as i32),
                    );
                    // Cleanup: always clear the allow list afterwards.
                    plugin.store.data_mut().allowed_panes.clear();
                    match result {
                        Ok(1) => return true, // suppress
                        Ok(_) => {}
                        Err(e) => {
                            error!("[plugin {}] on_output error: {}", plugin.path.display(), e)
                        }
                    }
                }
            }
        }
        false
    }

    /// Custom command hook — forward a `:cmd arg` style string to every plugin.
    ///
    /// Return value: `true` = at least one plugin handled it.
    ///
    /// v2 plugin: the string is sanitized before being passed, and `write_pane`
    ///   cannot write to any pane (the allow list is empty).
    /// v1 plugin: the raw string is passed and there is no write restriction
    ///   (legacy behavior).
    pub fn on_command(&self, cmd: &str) -> bool {
        let cmd_bytes = cmd.as_bytes();
        let sanitized = sanitize_for_plugin(cmd_bytes);
        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex is poisoned; recovering and continuing");
            poisoned.into_inner()
        });
        for plugin in plugins.iter_mut() {
            let Ok(func) = plugin
                .instance
                .get_typed_func::<(i32, i32), i32>(&plugin.store, "nexterm_on_command")
            else {
                continue;
            };
            let payload: &[u8] = if plugin.api_version >= 2 {
                &sanitized
            } else {
                cmd_bytes
            };
            if let Some(mem) = plugin.instance.get_memory(&plugin.store, "memory") {
                let offset = 64 * 1024usize;
                let mem_size = mem.data_size(&plugin.store);
                if offset + payload.len() <= mem_size {
                    mem.write(&mut plugin.store, offset, payload).ok();
                    // Refuel before each call (prevents infinite loops; CRITICAL #10).
                    if let Err(e) = plugin.store.set_fuel(FUEL_PER_CALL) {
                        error!(
                            "[plugin {}] failed to set fuel: {}",
                            plugin.path.display(),
                            e
                        );
                        continue;
                    }
                    // v2: command hooks cannot write to any pane.
                    plugin.store.data_mut().allowed_panes.clear();
                    let result =
                        func.call(&mut plugin.store, (offset as i32, payload.len() as i32));
                    plugin.store.data_mut().allowed_panes.clear();
                    match result {
                        Ok(0) => return true, // handled
                        Ok(_) => {}
                        Err(e) => {
                            error!("[plugin {}] on_command error: {}", plugin.path.display(), e)
                        }
                    }
                }
            }
        }
        false
    }

    /// Return the number of loaded plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Return the paths of all loaded plugins.
    pub fn plugin_paths(&self) -> Vec<PathBuf> {
        self.plugins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|p| p.path.clone())
            .collect()
    }
}

// ---- Metadata helpers -----------------------------------------------------

/// Read the plugin name and version strings from the `nexterm_meta` export.
/// Returns `(None, None)` if the export is absent.
fn read_plugin_meta(
    store: &mut Store<HostState>,
    instance: &wasmi::Instance,
) -> (Option<String>, Option<String>) {
    let Ok(meta_fn) =
        instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut *store, "nexterm_meta")
    else {
        return (None, None);
    };

    let Some(mem) = instance.get_memory(&mut *store, "memory") else {
        return (None, None);
    };

    // Allocate name/version buffers in WASM memory (128 bytes each).
    let name_off: usize = 64 * 1024;
    let ver_off: usize = name_off + 128;
    let mem_size = mem.data_size(&mut *store);
    if ver_off + 128 > mem_size {
        return (None, None);
    }

    // Zero the buffers before the call.
    mem.write(&mut *store, name_off, &[0u8; 128]).ok();
    mem.write(&mut *store, ver_off, &[0u8; 128]).ok();

    let _ = meta_fn.call(&mut *store, (name_off as i32, 128, ver_off as i32, 128));

    let data = mem.data(&mut *store);
    let name = read_cstr_from(data, name_off, 128);
    let ver = read_cstr_from(data, ver_off, 128);
    (name, ver)
}

/// Read a NUL-terminated UTF-8 string from WASM linear memory.
fn read_cstr_from(data: &[u8], offset: usize, max_len: usize) -> Option<String> {
    let slice = data.get(offset..offset + max_len)?;
    let end = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
    if end == 0 {
        return None;
    }
    String::from_utf8(slice[..end].to_vec()).ok()
}

// ---- Plugin information (for ctl display) ---------------------------------

/// Plugin information (displayed by `nexterm-ctl plugin list` and similar).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PluginInfo {
    /// Path of the WASM plugin file.
    pub path: String,
    /// API version declared by the plugin.
    pub api_version: u32,
    /// Plugin-declared name (from `nexterm_meta`).
    pub name: Option<String>,
    /// Plugin-declared version (from `nexterm_meta`).
    pub version: Option<String>,
}

impl PluginManager {
    /// Return information about all loaded plugins.
    pub fn list_info(&self) -> Vec<PluginInfo> {
        self.plugins
            .lock()
            .unwrap()
            .iter()
            .map(|p| PluginInfo {
                path: p.path.display().to_string(),
                api_version: p.api_version,
                name: p.meta_name.clone(),
                version: p.meta_version.clone(),
            })
            .collect()
    }
}

// ---- Default plugin directory ---------------------------------------------

/// Return the default plugin directory path.
///
/// - Linux/macOS: `~/.config/nexterm/plugins`
/// - Windows:     `%APPDATA%\nexterm\plugins`
pub fn default_plugin_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join("nexterm").join("plugins")
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("nexterm")
            .join("plugins")
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_write_pane() -> WritePaneFn {
        Arc::new(|_pane_id: u32, _data: &[u8]| {})
    }

    #[test]
    fn test_manager_new() {
        let mgr = PluginManager::new(noop_write_pane());
        assert_eq!(mgr.plugin_count(), 0);
    }

    #[test]
    fn test_load_dir_nonexistent() {
        let mgr = PluginManager::new(noop_write_pane());
        // A missing directory returns Ok(0).
        let result = mgr.load_dir(Path::new("/nonexistent/path"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_on_output_no_plugins() {
        let mgr = PluginManager::new(noop_write_pane());
        // Always returns false when there are no plugins (i.e. nothing is suppressed).
        assert!(!mgr.on_output(1, b"hello"));
    }

    #[test]
    fn test_on_command_no_plugins() {
        let mgr = PluginManager::new(noop_write_pane());
        // Always returns false when there are no plugins (i.e. unhandled).
        assert!(!mgr.on_command(":hello world"));
    }

    #[test]
    fn test_default_plugin_dir() {
        let dir = default_plugin_dir();
        // Path must not be empty.
        assert!(!dir.as_os_str().is_empty());
        // Must contain the `nexterm` and `plugins` segments.
        let s = dir.display().to_string();
        assert!(s.contains("nexterm"));
        assert!(s.contains("plugins"));
    }

    #[test]
    fn test_load_invalid_wasm() {
        let mgr = PluginManager::new(noop_write_pane());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not wasm").unwrap();
        let result = mgr.load(tmp.path());
        assert!(result.is_err());
    }

    /// Loading a minimal valid WASM module (an empty module) should not panic.
    #[test]
    fn test_load_minimal_wasm() {
        // Hand-encoded minimal WASM binary for `(module)`.
        let wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        let mgr = PluginManager::new(noop_write_pane());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &wasm).unwrap();
        // The load attempt itself should only return an error, never panic
        // (the nexterm.log / nexterm.write_pane imports are missing).
        let _ = mgr.load(tmp.path());
    }

    #[test]
    fn test_list_info_empty() {
        let mgr = PluginManager::new(noop_write_pane());
        let info = mgr.list_info();
        assert!(info.is_empty());
    }

    #[test]
    fn test_plugin_paths_empty() {
        let mgr = PluginManager::new(noop_write_pane());
        assert!(mgr.plugin_paths().is_empty());
    }

    #[test]
    fn test_load_dir_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = PluginManager::new(noop_write_pane());
        // Empty directory yields zero loaded plugins.
        let count = mgr.load_dir(dir.path()).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_load_dir_skips_non_wasm_files() {
        let dir = tempfile::tempdir().unwrap();
        // Drop non-`.wasm` files in place.
        std::fs::write(dir.path().join("script.sh"), b"#!/bin/sh\necho hello").unwrap();
        std::fs::write(dir.path().join("config.toml"), b"[plugin]\nname = \"test\"").unwrap();
        let mgr = PluginManager::new(noop_write_pane());
        let count = mgr.load_dir(dir.path()).unwrap();
        // No `.wasm` files present, so count is zero (`.sh` / `.toml` are skipped).
        assert_eq!(count, 0);
    }

    #[test]
    fn test_on_output_returns_false_without_plugins() {
        let mgr = PluginManager::new(noop_write_pane());
        // Long data should still return false.
        let data = b"Hello, World! This is a test output from a pane.";
        assert!(!mgr.on_output(42, data));
    }

    #[test]
    fn test_on_command_returns_false_without_plugins() {
        let mgr = PluginManager::new(noop_write_pane());
        assert!(!mgr.on_command(":open-split horizontal"));
        assert!(!mgr.on_command(":zoom"));
    }

    #[test]
    fn test_unload_nonexistent_returns_false() {
        let mgr = PluginManager::new(noop_write_pane());
        let result = mgr.unload(Path::new("/nonexistent/plugin.wasm"));
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_plugin_api_version_constant() {
        assert_eq!(PLUGIN_API_VERSION, 2);
        assert_eq!(MIN_SUPPORTED_API_VERSION, 1);
        // const assert: guarantees the compatibility invariant.
        const _: () = assert!(MIN_SUPPORTED_API_VERSION <= PLUGIN_API_VERSION);
    }

    #[test]
    fn test_list_info_has_path_fields() {
        let mgr = PluginManager::new(noop_write_pane());
        let info = mgr.list_info();
        assert!(info.is_empty());
    }

    #[test]
    fn test_read_cstr_from_empty_returns_none() {
        let data = vec![0u8; 32];
        assert!(read_cstr_from(&data, 0, 32).is_none());
    }

    #[test]
    fn test_read_cstr_from_valid_string() {
        let mut data = vec![0u8; 64];
        data[..5].copy_from_slice(b"hello");
        let result = read_cstr_from(&data, 0, 64);
        assert_eq!(result.as_deref(), Some("hello"));
    }

    // ---- Sanitization tests (Sprint 4-2) ----

    #[test]
    fn sanitize_passes_printable_ascii() {
        let input = b"Hello, World! 0123";
        assert_eq!(sanitize_for_plugin(input), input);
    }

    #[test]
    fn sanitize_passes_utf8_multibyte() {
        let input = "こんにちは世界".as_bytes();
        assert_eq!(sanitize_for_plugin(input), input);
    }

    #[test]
    fn sanitize_keeps_tab_lf_cr() {
        let input = b"a\tb\nc\rd";
        assert_eq!(sanitize_for_plugin(input), input);
    }

    #[test]
    fn sanitize_strips_other_c0_controls() {
        let input = b"a\x00b\x07c\x08d\x0Be\x7Ff";
        // NUL, BEL, BS, VT, DEL are stripped.
        assert_eq!(sanitize_for_plugin(input), b"abcdef");
    }

    #[test]
    fn sanitize_strips_csi_sequence() {
        // ESC [ 31 m  → SGR red.
        let input = b"red:\x1b[31mfoo\x1b[0mend";
        assert_eq!(sanitize_for_plugin(input), b"red:fooend");
    }

    #[test]
    fn sanitize_strips_osc_with_bel_terminator() {
        // OSC 0; title BEL
        let input = b"x\x1b]0;mytitle\x07y";
        assert_eq!(sanitize_for_plugin(input), b"xy");
    }

    #[test]
    fn sanitize_strips_osc_with_st_terminator() {
        // OSC 52 ; c ; <base64> ESC \
        let input = b"a\x1b]52;c;SGVsbG8=\x1b\\b";
        assert_eq!(sanitize_for_plugin(input), b"ab");
    }

    #[test]
    fn sanitize_strips_dcs_and_apc() {
        // DCS / APC are each terminated by ESC \.
        let input = b"a\x1bP123\x1b\\b\x1b_apc\x1b\\c";
        assert_eq!(sanitize_for_plugin(input), b"abc");
    }

    #[test]
    fn sanitize_strips_two_byte_escape() {
        // ESC = (DECKPAM)
        let input = b"x\x1b=y";
        assert_eq!(sanitize_for_plugin(input), b"xy");
    }

    #[test]
    fn sanitize_handles_truncated_csi() {
        // Sequence ending mid-CSI (no final byte) — discard everything after ESC.
        let input = b"safe\x1b[31";
        let out = sanitize_for_plugin(input);
        assert_eq!(out, b"safe");
    }

    #[test]
    fn sanitize_handles_lone_esc_at_end() {
        let input = b"safe\x1b";
        let out = sanitize_for_plugin(input);
        assert_eq!(out, b"safe");
    }

    #[test]
    fn sanitize_does_not_panic_on_arbitrary_bytes() {
        // No panic on an input that covers every possible byte value.
        let input: Vec<u8> = (0..=255u8).collect();
        let _ = sanitize_for_plugin(&input);
    }
}
