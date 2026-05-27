//! Lua evaluation worker — runs Lua on a dedicated OS thread.
//!
//! # Background
//!
//! `mlua::Lua` is `!Send + !Sync`, so it cannot be handed to a tokio
//! thread pool. Evaluating Lua synchronously on the winit event loop (the
//! main thread) risks blocking the UI when a script is complex.
//!
//! # Design
//!
//! - Spawn a dedicated Lua worker thread via `std::thread::spawn`.
//! - The worker receives requests through a `sync_channel(1)`, evaluates
//!   them, and writes the result into a cache.
//! - `eval_widgets()` returns from the cache immediately (it never blocks).
//! - When the channel is full (because the worker is busy), the request is
//!   dropped and the previously cached value is returned.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::prelude::*;
use mlua::{HookTriggers, VmState};
use tracing::{error, warn};

/// Evaluation request sent to the worker thread.
struct LuaRequest {
    widgets: Vec<String>,
}

/// Worker that evaluates Lua on a background thread.
///
/// The `Lua` instance lives exclusively on that worker thread (which keeps
/// it within the `!Send` constraint). The main thread can fetch the cached
/// result immediately via `eval_widgets()`.
pub struct LuaWorker {
    /// Cache of the latest evaluation result (updated by the worker thread).
    cache: Arc<Mutex<String>>,
    /// Send channel for evaluation requests (capacity 1: when full,
    /// `try_send` returns `Err`).
    request_tx: std::sync::mpsc::SyncSender<LuaRequest>,
}

/// Sentinel string used to recognize the timeout error inside the worker.
const LUA_TIMEOUT_MARKER: &str = "Lua evaluation timed out";

impl LuaWorker {
    /// Starts the worker thread.
    ///
    /// When `lua_script_path` is `Some`, that script is executed once at
    /// worker startup.
    pub fn new(lua_script_path: Option<PathBuf>) -> Self {
        let cache = Arc::new(Mutex::new(String::new()));
        let cache_clone = Arc::clone(&cache);

        // Capacity-1 channel: when the worker is busy, `try_send` returns
        // `Err` immediately.
        let (tx, rx) = std::sync::mpsc::sync_channel::<LuaRequest>(1);

        std::thread::Builder::new()
            .name("nexterm-lua-worker".to_string())
            .spawn(move || {
                // The Lua instance has to be created on this thread (it is
                // `!Send`, so it cannot be moved in from outside).
                // CRITICAL #4: use the sandboxed Lua (os/io/package disabled).
                let lua = match crate::lua_sandbox::sandboxed_lua() {
                    Ok(l) => l,
                    Err(e) => {
                        warn!("Lua worker: failed to initialize the sandboxed Lua: {}", e);
                        return;
                    }
                };

                if let Some(path) = lua_script_path
                    && path.exists()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(script) => {
                            if let Err(e) = lua.load(&script).exec() {
                                warn!("Lua worker: error loading the script: {}", e);
                            }
                        }
                        Err(e) => warn!("Lua worker: error reading the file: {}", e),
                    }
                }

                // Process requests in order. Exit when the channel closes.
                while let Ok(req) = rx.recv() {
                    // Evaluation timeout: abort after 100 ms.
                    let deadline = Instant::now() + Duration::from_millis(100);
                    lua.set_hook(
                        HookTriggers::new().every_nth_instruction(500),
                        move |_lua, _debug| {
                            if Instant::now() > deadline {
                                Err(LuaError::RuntimeError(format!(
                                    "{} (100ms)",
                                    LUA_TIMEOUT_MARKER
                                )))
                            } else {
                                Ok(VmState::Continue)
                            }
                        },
                    );

                    let parts: Vec<String> = req
                        .widgets
                        .iter()
                        .map(|expr| match lua.load(expr.as_str()).eval::<String>() {
                            Ok(s) => s,
                            Err(LuaError::RuntimeError(ref msg))
                                if msg.contains(LUA_TIMEOUT_MARKER) =>
                            {
                                warn!("Lua evaluation timed out: {}", expr);
                                String::new()
                            }
                            Err(e) => {
                                error!("Lua evaluation error: {}", e);
                                String::new()
                            }
                        })
                        .collect();

                    lua.remove_hook();

                    let result = parts.join("  ");

                    if let Ok(mut guard) = cache_clone.lock() {
                        *guard = result;
                    }
                }
            })
            .expect("failed to start the Lua worker thread");

        Self {
            cache,
            request_tx: tx,
        }
    }

    /// Requests evaluation of the widget expressions and returns the cached result.
    ///
    /// - The request is sent to the background thread and `eval_widgets`
    ///   returns immediately.
    /// - When the worker is busy, the request is dropped and the previously
    ///   cached result is returned.
    /// - The first call returns an empty string; the result becomes visible
    ///   from the next frame onward.
    pub fn eval_widgets(&self, widgets: &[String]) -> String {
        // `try_send`: drop the request when the channel is full (no blocking).
        let _ = self.request_tx.try_send(LuaRequest {
            widgets: widgets.to_vec(),
        });
        self.cache.lock().map(|g| g.clone()).unwrap_or_default()
    }
}
