# Migration Guide

This document gathers the steps required when upgrading to a Nexterm version that contains breaking changes.

> **Older versions:** Upgrade notes for v1.0.x → v1.5.x have moved to [`docs/migration/v1.x-legacy.md`](migration/v1.x-legacy.md) to keep this file focused on currently relevant releases.

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
