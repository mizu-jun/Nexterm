//! SSH host configuration and event hooks.

use serde::{Deserialize, Serialize};

/// SSH host configuration.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct HostConfig {
    /// Display name.
    pub name: String,
    /// Host name or IP address.
    pub host: String,
    /// SSH port (default: 22).
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// User name.
    pub username: String,
    /// Authentication method: `"password"`, `"key"`, or `"agent"`.
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    /// Private-key path (used when `auth_type = "key"`).
    pub key_path: Option<String>,
    /// Local port-forwarding specifications (e.g. `"8080:localhost:80"`).
    #[serde(default)]
    pub forward_local: Vec<String>,
    /// Remote port-forwarding specifications (e.g. `"9090:localhost:9090"`).
    #[serde(default)]
    pub forward_remote: Vec<String>,
    /// ProxyJump host name (an entry name registered in `hosts`).
    pub proxy_jump: Option<String>,
    /// Whether to enable X11 forwarding (equivalent to `ssh -X`).
    #[serde(default)]
    pub x11_forward: bool,
    /// Trusted X11 forwarding (equivalent to `ssh -Y`).
    #[serde(default)]
    pub x11_trusted: bool,
    /// Group name (arbitrary string used to categorize hosts).
    #[serde(default)]
    pub group: String,
    /// Tag list (multiple labels used for filtering).
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_auth_type() -> String {
    "key".to_string()
}

/// Terminal hook configuration — shell commands or Lua functions to run when
/// the corresponding event fires.
///
/// Shell-command hooks: specify a string; it is executed via `sh -c`.
///   The `$NEXTERM_PANE_ID` and `$NEXTERM_SESSION` environment variables are
///   available.
///
/// Lua-function hooks: specify a Lua function name in the `lua_on_*` field.
///   Define the function inside the configuration file as, for example,
///   `function on_pane_open(session, pane_id) ... end`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// Shell command run when a new pane is opened.
    pub on_pane_open: Option<String>,
    /// Shell command run when a pane is closed.
    pub on_pane_close: Option<String>,
    /// Shell command run when a new session starts.
    pub on_session_start: Option<String>,
    /// Shell command run when a client attaches to a session.
    pub on_attach: Option<String>,
    /// Shell command run when a client detaches from a session.
    pub on_detach: Option<String>,
    /// Lua function name invoked on pane open (e.g. `"on_pane_open"`).
    pub lua_on_pane_open: Option<String>,
    /// Lua function name invoked on pane close.
    pub lua_on_pane_close: Option<String>,
    /// Lua function name invoked on session start.
    pub lua_on_session_start: Option<String>,
    /// Lua function name invoked on attach.
    pub lua_on_attach: Option<String>,
    /// Lua function name invoked on detach.
    pub lua_on_detach: Option<String>,
}
