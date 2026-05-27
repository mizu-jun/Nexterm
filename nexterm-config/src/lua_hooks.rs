//! Lua hook runner — invokes Lua functions defined in the configuration file
//! as event hooks.
//!
//! # Design
//!
//! Like `LuaWorker`, `LuaHookRunner` runs on a dedicated thread. Hook events
//! are sent asynchronously via `fire_hook()` and executed fire-and-forget.
//!
//! # Writing Lua hooks
//!
//! ```lua
//! -- ~/.config/nexterm/nexterm.lua
//! hooks.on_pane_open = function(session, pane_id)
//!   -- Example: log information about the newly opened pane.
//!   io.write("pane opened: " .. tostring(pane_id) .. " in " .. session .. "\n")
//! end
//!
//! hooks.on_session_start = function(session)
//!   io.write("session started: " .. session .. "\n")
//! end
//! ```

use std::path::PathBuf;
use std::sync::mpsc;

use mlua::prelude::*;
use tracing::{error, warn};

/// Hook event kind.
#[derive(Debug)]
pub enum HookEvent {
    /// A pane was opened.
    PaneOpen {
        /// Session name.
        session: String,
        /// Pane ID.
        pane_id: u32,
    },
    /// A pane was closed.
    PaneClose {
        /// Session name.
        session: String,
        /// Pane ID.
        pane_id: u32,
    },
    /// A session started.
    SessionStart {
        /// Session name.
        session: String,
    },
    /// A client attached.
    Attach {
        /// Session name.
        session: String,
    },
    /// A client detached.
    Detach {
        /// Session name.
        session: String,
    },
    /// Macro execution request (with a reply channel).
    RunMacro {
        /// Name of the Lua function to invoke.
        lua_fn: String,
        /// Session name.
        session: String,
        /// Pane ID.
        pane_id: u32,
        /// Channel that returns the execution result.
        reply_tx: mpsc::SyncSender<Option<String>>,
    },
}

/// Lua hook runner.
///
/// Runs Lua on a dedicated thread and handles hook events.
pub struct LuaHookRunner {
    /// Event-send channel (`None` = no Lua script is present).
    event_tx: Option<mpsc::SyncSender<HookEvent>>,
}

impl LuaHookRunner {
    /// Loads the Lua script and starts the runner.
    ///
    /// Returns a no-op runner when the script is missing.
    pub fn new(lua_script_path: Option<PathBuf>) -> Self {
        let script_path = match lua_script_path {
            Some(p) if p.exists() => p,
            _ => return Self { event_tx: None },
        };

        let script = match std::fs::read_to_string(&script_path) {
            Ok(s) => s,
            Err(e) => {
                warn!("LuaHookRunner: error reading the script: {}", e);
                return Self { event_tx: None };
            }
        };

        let (tx, rx) = mpsc::sync_channel::<HookEvent>(64);

        std::thread::Builder::new()
            .name("nexterm-lua-hooks".to_string())
            .spawn(move || {
                // CRITICAL #4: use the sandboxed Lua (os/io/package disabled).
                let lua = match crate::lua_sandbox::sandboxed_lua() {
                    Ok(l) => l,
                    Err(e) => {
                        warn!(
                            "LuaHookRunner: failed to initialize the sandboxed Lua: {}",
                            e
                        );
                        return;
                    }
                };

                // Pre-create the `hooks` table so the user's script can register hook functions.
                if let Err(e) = lua.load("hooks = {}").exec() {
                    warn!(
                        "LuaHookRunner: failed to initialize the `hooks` table: {}",
                        e
                    );
                    return;
                }

                // Run the user script.
                if let Err(e) = lua.load(&script).exec() {
                    warn!("LuaHookRunner: error executing the script: {}", e);
                }

                // Event loop.
                while let Ok(event) = rx.recv() {
                    if let Err(e) = call_hook(&lua, &event) {
                        error!("LuaHookRunner: hook execution error ({:?}): {}", event, e);
                    }
                }
            })
            .expect("failed to start the LuaHookRunner thread");

        Self { event_tx: Some(tx) }
    }

    /// Fires a hook event asynchronously (fire-and-forget).
    ///
    /// When the channel is full, the event is dropped and a log line is emitted.
    pub fn fire(&self, event: HookEvent) {
        if let Some(tx) = &self.event_tx
            && tx.try_send(event).is_err()
        {
            warn!("LuaHookRunner: event queue is full; skipping the hook");
        }
    }

    /// Returns whether the Lua hook runner is active (i.e. a script exists).
    pub fn is_enabled(&self) -> bool {
        self.event_tx.is_some()
    }

    /// Runs a Lua macro synchronously and returns its return value (the text
    /// to send to the PTY).
    ///
    /// The macro must have the signature
    /// `function(session, pane_id) -> string`. Returns `None` on a timeout
    /// (500 ms) or when Lua is disabled.
    pub fn call_macro(&self, lua_fn: &str, session: &str, pane_id: u32) -> Option<String> {
        let tx = self.event_tx.as_ref()?;

        // Reply channel (capacity 1: the caller receives exactly one result).
        let (reply_tx, reply_rx) = mpsc::sync_channel::<Option<String>>(1);

        tx.try_send(HookEvent::RunMacro {
            lua_fn: lua_fn.to_string(),
            session: session.to_string(),
            pane_id,
            reply_tx,
        })
        .ok()?;

        // Wait up to 500 ms for the response.
        reply_rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .ok()
            .flatten()
    }
}

/// Calls a Lua hook function.
///
/// Only invokes `hooks.<event_name>` when it is defined as a function.
fn call_hook(lua: &Lua, event: &HookEvent) -> LuaResult<()> {
    let hooks: LuaTable = match lua.globals().get::<LuaTable>("hooks") {
        Ok(t) => t,
        Err(_) => return Ok(()), // `hooks` table does not exist
    };

    match event {
        HookEvent::PaneOpen { session, pane_id } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_pane_open") {
                func.call::<()>((session.as_str(), *pane_id))?;
            }
        }
        HookEvent::PaneClose { session, pane_id } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_pane_close") {
                func.call::<()>((session.as_str(), *pane_id))?;
            }
        }
        HookEvent::SessionStart { session } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_session_start") {
                func.call::<()>(session.as_str())?;
            }
        }
        HookEvent::Attach { session } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_attach") {
                func.call::<()>(session.as_str())?;
            }
        }
        HookEvent::Detach { session } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_detach") {
                func.call::<()>(session.as_str())?;
            }
        }
        HookEvent::RunMacro {
            lua_fn,
            session,
            pane_id,
            reply_tx,
        } => {
            // Look up by global function name (works for both `macros.*`
            // entries and direct global functions).
            let result: Option<String> = lua
                .globals()
                .get::<LuaFunction>(lua_fn.as_str())
                .ok()
                .and_then(|func| func.call::<LuaValue>((session.as_str(), *pane_id)).ok())
                .and_then(|val| match val {
                    LuaValue::String(s) => s.to_str().ok().map(|s| s.to_string()),
                    _ => None,
                });
            // Send the response (ignored if the receiver has already timed out).
            let _ = reply_tx.try_send(result);
        }
    }

    Ok(())
}
