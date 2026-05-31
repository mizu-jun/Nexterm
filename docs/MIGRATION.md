# Migration Guide

This document gathers the steps required when upgrading to a Nexterm version that contains breaking changes.

---

## v1.7.1 → v1.7.2 (IPC connect race fix + targeted wgpu_hal::vulkan::conv silencing)

**No breaking changes and no migration steps.** `PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4` are retained. The snapshot file schema is unchanged.

Two fixes ship as a follow-up PATCH:

- **IPC connect retry**: previously the GPU client tried to connect to the embedded server exactly once, immediately after wgpu init. On slow startups (snapshot load + font parsing + IPC pipe creation ≈ 1 to 1.5 s in practice) this single attempt could race the server, fail with `os error 2`, and leave the client in offline mode forever — the window appeared but no panes could be opened. The client now retries the initial connect up to 15 times on a 200 ms cadence (≈3 s budget). If you were hitting this race, simply upgrading is enough; no config change is required.
- **`wgpu_hal::vulkan::conv` lowered to `error` in the default log filter**: NVIDIA's recent Vulkan drivers advertise `VK_PRESENT_MODE_FIFO_LATEST_READY_EXT` (id `1000361000`), which the current wgpu release does not recognize and which it flags as a WARN every frame. The 1.7.1 filter only quieted INFO from `wgpu_hal`, so the WARN flood survived. The new filter targets just `wgpu_hal::vulkan::conv` at `error` level; legitimate WARNs from the rest of `wgpu_hal` continue to surface. To restore the 1.7.1 behaviour, set `NEXTERM_LOG=info,wgpu_core=warn,wgpu_hal=warn,naga=warn`.

---

## v1.7.0 → v1.7.1 (snapshot self-heal + ConPTY diagnostics + log-noise reduction)

**No breaking changes and no migration steps.** `PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4` are retained. The snapshot file schema is unchanged.

Three improvements ship together as a PATCH:

- **Snapshot self-heal**: when one or more windows or sessions fail to restore at startup (e.g. ConPTY returns `E_INVALIDARG` because the saved cwd no longer exists), the server now rewrites the snapshot immediately to evict the broken entries. Users who repeatedly saw the same `failed to restore window 'window-broken'` warning at every launch will see it once on the upgrade run and never again. No action required.
- **Detailed ConPTY error context**: `openpty` / `spawn_command` failures now carry the shell / cwd / cols / rows that triggered them. The message no longer ends at the opaque `HRESULT -2147024809`. Useful for diagnosing custom-shell setups.
- **Default log filter quietens wgpu**: the GPU client's default log filter (used when `NEXTERM_LOG` is unset) now includes `wgpu_core=warn,wgpu_hal=warn,naga=warn`. The per-frame `Device::maintain` INFO flood from `wgpu_core::device::resource` no longer reaches the log. If you explicitly set `NEXTERM_LOG` (for example to `trace`) the override behaves exactly as before — you still see every wgpu line. To restore the previous behaviour, set `NEXTERM_LOG=info`.

---

## v1.6.1 → v1.7.0 (Sprint 5-11-9 keybinding editor + Sprint 5-12 shell-launch visibility)

**No breaking changes.** `PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4` are unchanged. The configuration schema (`nexterm.toml` / `config.lua`) does not change either. Existing users can upgrade without any action.

### Sprint 5-11-9: interactive keybinding editor in the settings panel

The Keybindings category of the in-app settings panel used to be a read-only placeholder. It is now a fully interactive editor with screen-reader (AccessKit) support.

- **GUI**: 5-row layout showing the binding list, the selected key field (with a "Recording…" indicator while capturing a key press), the action `ComboBox`, and Add / Delete buttons. Navigation: `↑/↓` cycles between `List → Key → Action → Add → Delete`; `←/→` cycles the action or the dialog buttons; `Enter` activates; `Esc` cancels the in-flight edit or closes the delete dialog.
- **Editing modes**: `Click` on the key field enters **Record mode** (the next key press becomes the spelling). `Text mode` (free-form edit) is also available. Screen-reader users can additionally write the spelling directly via `Action::SetValue`, bypassing both modes.
- **Add / Delete**: pressing Add appends a fresh entry and immediately starts Record mode. Delete opens a confirmation dialog (Cancel focused by default to prevent accidental deletion).
- **AccessKit / screen-reader exposure**: fixed `NodeId 50..=56` for the Key field / Action field / Add / Delete / delete-dialog body; dynamic `NodeId 900_000_000 + idx` for each `ListBoxOption`. The Action `ComboBox` rejects `SetValue` strings outside `KEYBINDING_ACTIONS` (the 27 known actions). Live updates flow through the existing 100 ms-throttled `compute_tree_state_hash` path — the SR observes selection / focus / Record-mode / Text-mode buffer changes in real time.

#### Impact on existing users

- No upgrade procedure required. Existing `[[keys]]` entries in `nexterm.toml` continue to load and now appear in the new editor.
- Saving from the editor still goes through the existing `[[keys]]` TOML write path; you can keep editing the file by hand if you prefer.
- No NodeId range that an external tool could have assumed is broken — the new IDs land in previously-reserved slots (50..=56 in the fixed range and 900M..1G in the dynamic range).

---

## v1.6.1 → v1.7.0 (Sprint 5-12: visibility and fix for shell-launch failures)

**No breaking changes.** `PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4` are unchanged. The configuration schema (`nexterm.toml` / `config.lua`) does not change either. Existing users can upgrade without any action.

### What changed

A four-phase fix and regression-prevention pass for a bug on Windows where, despite specifying PowerShell in `config.toml`, the shell would fail to start or the pane would remain blank.

1. **Error-banner UI**: `ServerToClient::Error` is now visualised as a red banner at the top of the screen (closable with `Esc`) instead of being logged only. The root cause of PTY startup failures is immediately visible.
2. **PowerShell version-comparison bug fixed**: when scanning `%ProgramFiles%\PowerShell\`, lexicographic comparison treated `"7" > "10"` and skipped PowerShell 10. The comparison was switched to numeric.
3. **`shell.args` merge support in Lua**: `config.lua` can now override args, for example `shell = { program = "pwsh.exe", args = {"-NoLogo", "-NonInteractive"} }`. The previous implementation silently discarded `args`.
4. **Client notification for config-load errors**: syntax errors in `nexterm.toml` and similar are now accumulated as startup warnings and surfaced through the banner on the first attach, instead of being silently swallowed.

### Impact on existing users

- **No upgrade procedure needed.** No config edits required.
- Users already on PowerShell 7 keep selecting 7 (only an additional PowerShell 10 installation will cause the auto-switch to 10).
- If you had accidentally configured `shell.args` from Lua, the previously-ignored values **will now actually take effect from the next launch onwards**. That is the one thing to watch for.

### How to verify (real Windows machine)

```powershell
# 1. Enable debug logging and launch
$env:NEXTERM_LOG = "debug"
nexterm 2> $env:USERPROFILE\nexterm-debug.log

# 2. Confirm PowerShell 10 is detected in the log
Select-String -Path $env:USERPROFILE\nexterm-debug.log -Pattern "pwsh|powershell"

# 3. When PTY startup fails, confirm that a red banner appears at the top of the screen
#    (closable with Esc while visible)
```

### Interoperability with older versions

- An existing server binary remains interoperable with the new client. Old servers do not have the `startup_warnings` retrieval API, but the client-side error banner still works correctly for existing `ServerToClient::Error` events (PTY spawn failures, etc.).
- With a new server + old client, startup warnings are emitted but the old client only logs them — no banner is shown (no impact).

---

## v1.6.0 → v1.6.1 (Flatpak build hotfix)

**No breaking changes.** Functionally identical to v1.6.0. v1.6.0's `Flatpak` workflow failed the integrity check between `pkg/flatpak/cargo-sources.json` and `Cargo.lock`, so the Flatpak bundle was not being distributed; v1.6.1 fixes that.

### Impact

- Users who already obtained the macOS / Linux / Windows binaries for v1.6.0 **do not need to update**.
- Users planning to install via Flatpak should use the v1.6.1 assets.

### Cause and fix

When Sprint 5-11-1 (AccessKit PoC) added `accesskit = "0.24"` / `accesskit_winit = "0.33"` to `Cargo.toml`, new vendor dependencies were added to `Cargo.lock` (`accesskit-0.24.0`, `accesskit_atspi_common-0.18.1`, `accesskit_consumer-0.36.0`, `accesskit_ios-0.1.0`, `accesskit_macos-0.26.1`, and others), but `scripts/regenerate-flatpak-sources.sh` was not re-run. v1.6.1 regenerates it (a 234-line addition) to restore consistency.

---

## v1.5.1 → v1.6.0 (Sprint 5-11 in full: screen-reader support + SSH host GUI editing)

**No breaking changes.** `PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4` are unchanged. The **last remaining HIGH item from audit round 2, H1 (screen-reader support)**, is now fully implemented on top of AccessKit 0.24 + accesskit_winit 0.33, and the settings panel's SSH category gains GUI editing. Existing users can upgrade without any action.

### Summary

Sprints 5-11-1 through 5-11-8 are released together as v1.6.0. See [CHANGELOG.md](../CHANGELOG.md) for the full changelog. This section focuses on the SSH-host GUI editing feature (Step 8-3).

### New features

#### Sub-phase A: inline GUI editing of SSH fields

In the settings panel → SSH category, pressing Enter on a host-list field (name / host / username) now enters edit mode. While editing, characters / Backspace / ← → / Home / End / Delete are available. **The TUI client is unaffected** (GPU client only).

#### Sub-phase B: route IME preedit into SSH fields

CJK IME (Japanese / Chinese / Korean) preedit text behaves correctly while editing SSH fields. Pre-commit characters are inserted at the caret, and the IME window follows the caret via `set_ime_cursor_area`.

#### Sub-phase C: visual editing of port (SpinButton) / auth_type (ComboBox)

- **port**: `←` / `→` increment or decrement by 1 (clamped 1–65535). For screen readers, exposed as `Role::SpinButton` responding to `Increment` / `Decrement` / `SetValue`.
- **auth_type**: `←` / `→` cycle through `password` / `key` / `agent`. For screen readers, exposed as `Role::ComboBox` responding to `Increment` / `Decrement` / `Click`.

#### Sub-phase D: Add / Delete buttons + delete-confirmation dialog

The end of the host list now has "Add new host" and "Delete selected host" buttons. Keyboard:

| Action | Key |
|--------|-----|
| Focus the Add / Delete button | `↑` / `↓` (focus 6 / 7) |
| Press Add | Enter (creates a new host and immediately enters name edit mode) |
| Press Delete | Enter (opens the delete-confirmation dialog) |
| Confirm in dialog | Enter |
| Cancel in dialog | Esc or `N` |
| Toggle Cancel ↔ Confirm | `←` / `→` or Tab |

The Delete button is shown as disabled when the list is empty. The confirmation dialog defaults focus to Cancel, following standard GUI conventions to prevent accidental deletion. The selection after deletion is **clamped to n**: deleting the last item moves focus to n-1, deleting from the middle keeps the same index (the list shifts up), and emptying the list returns focus to the ListBox.

#### AccessKit NodeId additions (no breaking changes)

NodeIds 45–49 are reserved for the new SSH-category UI elements:

| NodeId | Purpose | Role |
|--------|---------|------|
| 45 | Add button | `Role::Button` |
| 46 | Delete button | `Role::Button` (empty-list "disabled" surfaced via description) |
| 47 | Delete-confirmation dialog itself | `Role::AlertDialog` (modal) |
| 48 | Confirm (delete) button | `Role::Button` |
| 49 | Cancel button | `Role::Button` |

`compute_tree_state_hash` includes the changes to `ssh_delete_dialog_open` / `ssh_delete_dialog_confirm_focused` in the 100 ms throttle window, so dialog open/close and focus changes are reflected to the screen reader correctly.

### No configuration changes required

The new features do not require any `config.toml` change. Existing `[[hosts]]` sections continue to load as-is and can additionally be edited from the GUI.

### Impact on the TUI client

`nexterm-client-tui` is out of scope for both GUI editing and screen-reader support. Continue editing `[[hosts]]` in `config.toml` directly as before.

---

## v1.4.0 → v1.5.0 (Sprint 5-8 / 5-9 Phase 4: tab tearing — drop-out-of-tab)

`PROTOCOL_VERSION` bumps from `7` to `8` and `SNAPSHOT_VERSION` bumps from `3` to `4`. **New servers reject old clients (PROTOCOL v7) during the Hello handshake, and old servers reject new clients.** Upgrade clients and servers together.

### What changes

#### PROTOCOL_VERSION 8 (Phase 4-3: drop-out-of-tab)

`ClientToServer::MovePaneToWindow { pane_id, target_window_id, insert_at }` is new. When a client drops a tab onto a different OS window (or onto a brand-new OS window), the server runs `Window::detach_pane` + `Window::attach_pane` to restructure the session's Window layout. `target_window_id = 0` is the signal for "create a new Window and move the tab there".

Phase 4-5 **added** (v8 compatible — appended to the end of the enum, so discriminants are unaffected):

- `ClientToServer::QueryForegroundProcess { window_id }` — used to detect a foreground process before closing a Window
- `ServerToClient::ForegroundProcessStatus { window_id, has_foreground }` — the response

Old v8 clients and servers neither send nor receive the new variants, so v8-compatible connections from Phase 4-3 are unaffected.

#### SNAPSHOT_VERSION 4 (Phase 4-5: persisting OS Window layout)

`ServerSnapshot.client_os_windows: Vec<OsWindowSnapshot>` is new. It captures the multiple OS windows that tab tearing has produced. It is annotated with `#[serde(default)]`, so **v3 JSON auto-migrates with an empty `Vec`** (existing users do not need to act; the first launch restores into a single-OS-window layout).

`OsWindowSnapshot` structure:

```rust
pub struct OsWindowSnapshot {
    pub position: (i32, i32),            // top-left of the window
    pub size: (u32, u32),                // outer dimensions
    pub server_window_ids: Vec<u32>,     // server Window IDs (tabs) inside this OS window
    pub focused_server_window_id: u32,   // active server Window
}
```

### Migration steps

1. **Upgrade clients and servers together**: a PROTOCOL_VERSION 7 → 8 mismatch will fail the Hello handshake. Update every binary, including `nexterm-ctl`.
2. **Existing snapshots**: no action — they auto-migrate. When `~/.local/state/nexterm/snapshot.json` is loaded, its version is bumped from 3 to 4.
3. **Config changes**: a `close_action` key was added to the `[window]` section (see below). The default when omitted is `"prompt"` (the recommended default).

### The `window.close_action` setting

Choose between three behaviours when closing an OS window:

```toml
[window]
close_action = "prompt"   # prompt (default) / detach / kill
```

| Value | Behaviour |
|-------|-----------|
| `"prompt"` | Send `QueryForegroundProcess` to the server. If a non-shell child (foreground process) is running, show a confirmation dialog; otherwise kill immediately. **This is the safe default.** |
| `"detach"` | Disconnect the client only and keep the server-side Session alive. `nexterm-ctl attach` can re-attach (multi-process layout). In single-binary mode, the embedded server task is also aborted, so this is effectively equivalent to kill. |
| `"kill"` | Send `KillSession` IPC immediately, destroy the Session, and exit. This was the default through v1.4.0. |

### Tab tearing UX (Wayland limits and alternatives)

Dragging a tab outside an OS window opens a new OS window (X11 / macOS / Windows). **Wayland's security model does not allow global coordinates to be read**, so drag-and-drop detection does not work there. Wayland users should use the following alternatives:

| Alternative | Action |
|-------------|--------|
| Context menu | Right-click on a pane → "Detach to new window" |
| Hotkey | `Ctrl+B D` (leader + D) detaches the current tab into a new OS window |
| Command palette | `Ctrl+Shift+P` → "Detach to New Window" |
| `[↗]` button on hover | **Not provided in v1.5.0 (deferred to Phase 4-6).** Implementing this requires hover-time vertex insertion and hit testing in the renderer; planned for the next phase. |

Hotkey to close an OS window: `Ctrl+B W` (leader + W). The entire process exits according to `close_action` only when it was the last OS window.

### Keyboard operation of the confirmation dialog

When `close_action = "prompt"` and a foreground process is detected:

- `Enter` / `Y` → close (kill)
- `Esc` / `N` → cancel
- `←` / `→` to switch button focus

> Note: in v1.5.0 the dialog renderer is a minimum implementation. Strings are already i18n-ready in 8 languages. Visual polish is planned for Phase 4-6.

### Impact on the TUI client

`nexterm-client-tui` runs one process per single terminal, so tab tearing is disabled. The new `ForegroundProcessStatus` / `MovePaneToWindow` variants, when received, are no-ops (we match the full `ServerToClient` enum for compatibility).

---

## v1.2.0 → v1.3.0 (Sprint 5-7 Phase 2: IPC variants and snapshot extensions)

`PROTOCOL_VERSION` bumps from `4` to `7` and `SNAPSHOT_VERSION` bumps from `2` to `3`. **New servers reject old clients during the Hello handshake, and old servers reject new clients.** Upgrade clients and servers together.

### What changes

#### PROTOCOL_VERSION 5 (Sprint 5-7 / Phase 2-1: workspaces)

Five new `ClientToServer` variants and two new `ServerToClient` variants:

- `ListWorkspaces` / `CreateWorkspace { name }` / `SwitchWorkspace { name }` / `RenameWorkspace { from, to }` / `DeleteWorkspace { name, force }`
- `WorkspaceList { workspaces, current }` / `WorkspaceSwitched { name }`

postcard does **not** provide forward compatibility on enum-variant additions, so old clients (PROTOCOL_VERSION 4) cannot decode `WorkspaceList` from a new server.

#### PROTOCOL_VERSION 6 (Sprint 5-7 / Phase 2-2: Quake mode)

Adds `ClientToServer::QuakeToggle { action }` and `ServerToClient::QuakeToggleRequest { action }`. `action` is one of `"toggle"` / `"show"` / `"hide"`.

#### PROTOCOL_VERSION 7 (Sprint 5-7 / Phase 2-3: tab reordering)

Adds `ClientToServer::ReorderPanes { pane_ids: Vec<u32> }`. The server reorders the tabs accordingly and sends `LayoutChanged` only if the order actually changed. The meaning of the order in `LayoutChanged.panes` changes from "BSP DFS order" to "logical tab display order", but old clients did not rely on the order, so behaviour is unaffected.

#### SNAPSHOT_VERSION 3 (Sprint 5-7 / Phase 2-1)

Adds `SessionSnapshot.workspace_name: String` and `ServerSnapshot.current_workspace: String`. **v2 JSON auto-migrates** via `serde(default = "default_workspace")`, so existing snapshots do not need to be edited by hand. After migration, all sessions belong to the `"default"` workspace.

### Migration steps

1. **Upgrade clients and servers together**: a PROTOCOL_VERSION mismatch will fail the handshake.
2. **Existing snapshots**: no action — they auto-migrate. When `~/.local/state/nexterm/snapshot.json` is loaded, its version is bumped from 2 to 3.
3. **No config changes required**: all new features are optional; existing `config.toml` files still work as-is.

### Quake mode (Wayland limits)

The `global-hotkey` 0.8 crate cannot register global hotkeys under the Wayland security model. On Wayland, call `nexterm-ctl quake toggle/show/hide` from compositor keybindings (Sway's `bindsym` / Hyprland's `bind`, etc.):

```
# Sway
bindsym Mod4+grave exec nexterm-ctl quake toggle

# Hyprland
bind = SUPER, grave, exec, nexterm-ctl quake toggle
```

On X11, hotkeys can be registered directly via `config.toml`'s `[quake_mode] hotkey = "ctrl+\`"`.

### Background image (Phase 3-1)

The background image is loaded only at startup. Changes to `config.toml` require a restart.

```toml
[window.background_image]
path = "~/wallpaper.png"
opacity = 0.3
fit = "cover"  # cover / contain / stretch / center / tile
```

### Animations (Phase 3-2)

Ease-out animations on new-pane insertion and tab switching are enabled by default. If you want reduced motion for accessibility reasons, set one of:

```toml
[animations]
enabled = false                # disable all animations
# or
intensity = "off"              # off / subtle / normal / energetic
```

---

## v1.1.0 → Unreleased (Sprint 5-1 / G3: IPC wire format migrates to postcard)

`PROTOCOL_VERSION` bumps from `2` to `3`. **New servers reject old clients during the Hello handshake, and old servers reject new clients.** Upgrade clients and servers together.

### What changes

The IPC serialization format changes from `bincode` 1.x to `postcard` 1.x. The two are not byte-compatible (postcard uses varint encoding; bincode is fixed-width).

Rationale: address `RUSTSEC-2025-0141` (bincode 1.x is no longer maintained) and improve mid- to long-term supply-chain health.

### User impact

- Beyond upgrading binaries together, no user action is required.
- `nexterm-ctl` also connects over IPC, so the CLI binary must be updated at the same time.

### Side effects

- IPC messages tend to shrink by 10–20% on average (varint encoding).
- Third-party plugins that directly depend on `bincode = "1"` need to migrate to `postcard` as well. The plugin API runs through WASM, so this normally has no effect.

---

## v1.1.0 → Unreleased (Sprint 5-1 / G1: SSH passwords move into the keyring)

`PROTOCOL_VERSION` bumps from `1` to `2`. **New servers reject old clients during the Hello handshake, and old servers reject new clients.** Upgrade clients and servers together.

### What changes

The `password: Option<String>` field is removed from `ClientToServer::ConnectSsh` and is replaced by two new fields:

- `password_keyring_account: Option<String>` — keyring account identifier of the form `<username>@<host_name>`
- `ephemeral_password: bool` — when `true`, the server removes the keyring entry after authentication completes

This ensures that passwords no longer travel as plaintext over the IPC channel (Unix domain socket / named pipe). The client stores them in the OS keyring (Service=`"nexterm-ssh"`) and the server retrieves them with the same user's permissions.

### Impact

- Mixing an old `nexterm-ctl` binary with a new server is unsupported (IPC compatibility break).
- When using password-authenticated SSH hosts, the server host and the client host must run as the **same OS user** (the OS keyring is access-controlled per user).
- Environments where the OS keyring service is unavailable (for example a headless Linux without Secret Service) cannot use password-authenticated SSH. Use key authentication or set up `secret-tool` / `gnome-keyring` / `KWallet`.

### Migration steps

Upgrade the client and server binaries simultaneously to v1.2.0+. No extra configuration is required. The "Save / Don't save" UI on `HostManager`'s password modal continues to work.

---

## v1.0.2 → Unreleased (Sprint 4-2 plugin API v2)

`PLUGIN_API_VERSION` bumps from `1` to `2`. **v1 plugins continue to work**, but they log a deprecation warning at load time. Support for v1 will be removed in the future (no specific timing decided yet).

### Migrating a plugin from v1 to v2

1. **Update `nexterm_api_version` to return `2`**:

   ```rust
   #[unsafe(no_mangle)]
   pub extern "C" fn nexterm_api_version() -> i32 {
       2
   }
   ```

2. **Reconsider how input data is handled**: in v2 the host pre-strips ESC, OSC/CSI/DCS/APC, and C0 control characters (other than `\t\r\n`) before handing the bytes to the plugin. Plugins that observed or parsed escape sequences themselves will see different behaviour.

3. **`write_pane` is now restricted by destination PaneId**:
   - Inside `nexterm_on_output(pane_id, ...)`: writes are allowed only to the `pane_id` that was passed in.
   - Inside `nexterm_on_command(...)`: writes are not allowed to any pane.
   - Rejections log a warning but are not fatal; processing continues.

### Staying on v1

If `nexterm_api_version` is not exported, or it still returns `1`, the plugin continues to work. The following deprecation warning is recorded at startup:

```
Plugin running with API v1 (current: v2): <path> — behaves with the legacy path: no sanitization, no PaneId verification.
v1 support is scheduled for removal in a future version.
```

---

## v1.0.2 → Unreleased (Sprint 1 – 3 security hardening)

A security audit produced four large changes, some of them breaking.

### 1. Hello message becomes mandatory (always affects you)

**What changes**

The IPC protocol between client and server gains a handshake message. After connecting, the client must first send `ClientToServer::Hello { proto_version, client_kind, client_version }`.

**Impact**

- Connecting an old client (v1.0.2 or earlier) to a new server **causes the server to drop the connection**, because the first message is not `Hello`.
- Connecting a new client to an old server may cause the server to treat `Hello` as an unknown message.

**What to do**

- Treat the client and server as needing the **same version**.
- When installing, update GPU client, TUI client, `nexterm-ctl`, and server all at once.

```bash
# Clean rebuild while developing with cargo
cargo clean
cargo build --release --workspace

# When installed via a package manager
# Windows: uninstall v1.0.2 via msiexec, then install the new version
# Linux (Flatpak): flatpak update
```

`PROTOCOL_VERSION` will be managed in `nexterm-proto/src/lib.rs` going forward.

---

### 2. Lua API restricted by sandboxing (affects existing `config.lua`)

**What changes**

The Lua instance used by `config.lua`, Lua hooks, and macros is now sandboxed. The following are **disabled**:

| Removed global | Replacement |
|----------------|-------------|
| `os.execute` / `os.remove` / `os.rename` / `os.tmpname` and other `os.*` | None today (a restricted `nexterm.*` namespace is planned) |
| `io.open` / `io.read` / `io.lines` and other `io.*` | None today |
| `require` / `dofile` / `loadfile` / `load` / `loadstring` | All Lua code must be inlined inside `~/.config/nexterm/nexterm.lua` |
| `debug.*` | Removed |
| `package.*` | Removed |
| `collectgarbage` / `rawset` / `rawget` / `setfenv` / `getfenv` | Removed |

Allowed libraries: `string` / `table` / `math` / `coroutine`.

**Impact**

If your old `config.lua` had patterns like the following, **loading fails with an error**:

```lua
-- Old: NG (os.execute is disabled in the sandbox)
hooks.on_pane_open = function(session, pane_id)
    os.execute("notify-send 'New pane opened'")
end

-- Old: NG (io.write is disabled in the sandbox)
print("loaded at " .. os.date())
```

**What to do**

1. **Shell commands** should be invoked through terminal hooks (`config.toml`'s `[hooks] on_pane_open = "/path/to/script"`).
2. **File I/O** should not run inside Lua; use an external script invoked through a terminal hook.
3. **Timestamps** will eventually be available as `nexterm.now()`. Until then, use the UI's status bar `time` widget.

There is no escape hatch for keeping the old `config.lua` as-is. The sandbox cannot be disabled by design (to address CRITICAL #4).

---

### 3. TLS fallback disabled by default (affects users who configured HTTPS)

**What changes**

With `[web] tls.enabled = true`, the behaviour when certificate files fail to load is changing.

| Old (v1.0.2 and earlier) | New (Unreleased) |
|---|---|
| Emit a warning and **automatically downgrade to HTTP** | **Abort web-server startup** |

**Impact**

If the certificate file is missing, has wrong permissions, or has an invalid format, the web terminal will not start (IPC continues to work normally).

**What to do (recommended)**

Set certificate paths correctly, or use the auto-generated self-signed certificate path:

```toml
[web]
enabled = true
[web.tls]
enabled = true
# Omit cert_file / key_file → auto-generated into ~/.config/nexterm/tls/
```

**What to do (test/dev only — not recommended)**

Explicitly opt in to HTTP fallback:

```toml
[web]
enabled = true
allow_http_fallback = true   # WARNING: session tokens travel in plaintext
[web.tls]
enabled = true
cert_file = "/path/to/cert.pem"   # Continue startup even if loading fails
```

---

### 4. OAuth org membership verification now **actually** works

**What changes**

The old implementation did not actually validate `allowed_orgs` (a bug in `get_current_token()` made it impossible for the org check to ever run). The new implementation correctly verifies membership against the GitHub API.

**Impact**

| Old behaviour | New behaviour |
|---|---|
| Setting `allowed_orgs` only → **nobody can log in** (effectively broken) | Setting `allowed_orgs` only → members allowed, non-members denied (as specified) |
| Setting both `allowed_emails` and `allowed_orgs` → only an email match passed; the org check was skipped entirely | Allowed when the email matches OR org membership matches |

Administrators who believed they had "double protection via org membership" had been mistaken. **Please review your configuration.**

---

### 5. WASM plugins: fuel and memory limits (affects plugin authors)

**What changes**

- Each `nexterm_on_output` / `nexterm_on_command` call is provisioned with `10,000,000` units of fuel before execution. Exhausting fuel traps the call.
- A plugin's linear memory is rejected at load time if it requests more than `256 pages (16 MiB)` initially.
- If a plugin exports `nexterm_api_version()`, the value must match the host's `PLUGIN_API_VERSION` (= 1) or the plugin is rejected.

**Impact**

Normal plugins are unaffected. Watch out for:

- Plugins that need more than 10 million instructions per call will trap → split work into multiple calls, or file a request with the host.
- Plugins that request more than 16 MiB initial memory will fail to load → switch to dynamic memory growth.

---

### 6. `config.toml` key renaming (affects users of the old template)

**What changes**

The `DEFAULT_CONFIG_TOML` template generated on first launch used **key names that did not match the implementation**, so it was updated.

| Old template (didn't match the implementation) | New template (matches) |
|---|---|
| `[color_scheme] builtin = "tokyonight"` | `colors = "tokyonight"` or `[colors] scheme = "tokyonight"` |
| `[tab_bar] show = true / position = "top"` | `[tab_bar] enabled = true / height = 28` |
| `[status_bar] show = true / position = "bottom"` | `[status_bar] enabled = true` |

**Impact**

- The old template never actually took effect, so **the user experience does not change** (in fact, settings now genuinely take effect).
- If you customised `config.toml`, update it to the new key names by hand.

**Newly configurable sections** (ignored by the previous `TomlConfig`)

```toml
[window]
background_opacity = 0.85
padding_x = 8
padding_y = 4

[[hosts]]
name = "production"
host = "192.168.1.100"
port = 22
username = "ops"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

[[macros]]
name = "git-status"
description = "Show git status"
lua_fn = "macro_git_status"

[web]
enabled = true
[web.auth]
totp_enabled = true

cursor_style = "block"        # "block" / "beam" / "underline"
auto_check_update = true
language = "auto"             # "auto" / "ja" / "en" / "fr" / "de" / "es" / "it" / "zh-CN" / "ko"
```

---

## Troubleshooting

### Cannot connect to the server ("protocol version mismatch")

Client and server versions do not match. Update both to the latest version.

### Lua scripts fail to load

Check that you are not using `os.execute` / `io.open` / `require`. They are disabled in the sandbox (see [item 2](#2-lua-api-restricted-by-sandboxing-affects-existing-configlua)).

### The web terminal does not start

Falling back from a failed TLS configuration is no longer the default (see [item 3](#3-tls-fallback-disabled-by-default-affects-users-who-configured-https)). Fix your certificate configuration, or set `allow_http_fallback = true`.

### Suddenly cannot log in via OAuth

If you only set `allowed_orgs`: under the old implementation nobody could log in, so you may simply have been unable to use it. Re-check your org-membership configuration (see [item 4](#4-oauth-org-membership-verification-now-actually-works)).

---

## Support

If you run into problems, please report them at https://github.com/mizu-jun/Nexterm/issues.
