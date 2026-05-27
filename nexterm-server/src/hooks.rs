//! Terminal hook execution — runs shell commands and Lua functions asynchronously on events.
//!
//! Shell-command hooks are executed via `sh -c <cmd>`. The server keeps running even if a hook fails.
//! Environment variables exposed to hooks:
//!   $NEXTERM_PANE_ID     — target pane ID (pane events only)
//!   $NEXTERM_SESSION     — session name (all events)

use nexterm_config::{HookEvent, HooksConfig, LuaHookRunner};
use tracing::warn;

/// Spawn a hook as an async task (fire-and-forget).
pub fn fire(cmd: &str, session: &str, pane_id: Option<u32>) {
    let cmd = cmd.to_string();
    let session = session.to_string();
    tokio::spawn(async move {
        let mut child = match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .env("NEXTERM_SESSION", &session)
            .env(
                "NEXTERM_PANE_ID",
                pane_id.map(|id| id.to_string()).unwrap_or_default(),
            )
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to launch hook '{}': {}", cmd, e);
                return;
            }
        };
        if let Err(e) = child.wait().await {
            warn!("failed to wait for hook '{}': {}", cmd, e);
        }
    });
}

/// Execute the `on_pane_open` hook (shell command + Lua).
pub fn on_pane_open(hooks: &HooksConfig, lua: &LuaHookRunner, session: &str, pane_id: u32) {
    if let Some(cmd) = &hooks.on_pane_open {
        fire(cmd, session, Some(pane_id));
    }
    lua.fire(HookEvent::PaneOpen {
        session: session.to_string(),
        pane_id,
    });
}

/// Execute the `on_pane_close` hook (shell command + Lua).
#[allow(dead_code)]
pub fn on_pane_close(hooks: &HooksConfig, lua: &LuaHookRunner, session: &str, pane_id: u32) {
    if let Some(cmd) = &hooks.on_pane_close {
        fire(cmd, session, Some(pane_id));
    }
    lua.fire(HookEvent::PaneClose {
        session: session.to_string(),
        pane_id,
    });
}

/// Execute the `on_session_start` hook (shell command + Lua).
pub fn on_session_start(hooks: &HooksConfig, lua: &LuaHookRunner, session: &str) {
    if let Some(cmd) = &hooks.on_session_start {
        fire(cmd, session, None);
    }
    lua.fire(HookEvent::SessionStart {
        session: session.to_string(),
    });
}

/// Execute the `on_attach` hook (shell command + Lua).
pub fn on_attach(hooks: &HooksConfig, lua: &LuaHookRunner, session: &str) {
    if let Some(cmd) = &hooks.on_attach {
        fire(cmd, session, None);
    }
    lua.fire(HookEvent::Attach {
        session: session.to_string(),
    });
}

/// Execute the `on_detach` hook (shell command + Lua).
pub fn on_detach(hooks: &HooksConfig, lua: &LuaHookRunner, session: &str) {
    if let Some(cmd) = &hooks.on_detach {
        fire(cmd, session, None);
    }
    lua.fire(HookEvent::Detach {
        session: session.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::{HooksConfig, LuaHookRunner};

    #[tokio::test]
    async fn test_fire_does_not_panic() {
        fire("echo hello", "test-session", Some(1));
    }
    #[tokio::test]
    async fn test_on_pane_open_calls_fire_and_lua() {
        // Create dummy configs
        let hooks = HooksConfig::default();
        let lua = LuaHookRunner::new(None);

        // This should not panic
        on_pane_open(&hooks, &lua, "test-session", 123);
    }

    #[test]
    fn test_on_pane_close_calls_fire_and_lua() {
        let hooks = HooksConfig::default();
        let lua = LuaHookRunner::new(None);

        on_pane_close(&hooks, &lua, "test-session", 123);
    }

    #[test]
    fn test_on_session_start_calls_fire_and_lua() {
        let hooks = HooksConfig::default();
        let lua = LuaHookRunner::new(None);

        on_session_start(&hooks, &lua, "test-session");
    }

    #[test]
    fn test_on_attach_calls_fire_and_lua() {
        let hooks = HooksConfig::default();
        let lua = LuaHookRunner::new(None);

        on_attach(&hooks, &lua, "test-session");
    }

    #[test]
    fn test_on_detach_calls_fire_and_lua() {
        let hooks = HooksConfig::default();
        let lua = LuaHookRunner::new(None);

        on_detach(&hooks, &lua, "test-session");
    }
}
