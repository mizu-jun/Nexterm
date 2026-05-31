//! Input handler
//!
//! Sprint 5-6 split the original `input_handler.rs` (1,377 lines) into 6 sub-modules:
//! - `copy_mode` — copy-mode (tmux-compatible) key input
//! - `action` — action dispatch for `config.keys` / context menu
//! - `ssh` — SSH connection helpers
//! - `font` — font-size changes (Ctrl++/-, Ctrl+0)
//! - `special_modes` — key input for Quick Select / consent dialogs
//!
//! This file is the top-level dispatcher:
//! - `handle_key` — interpret a winit key event and decide whether to consume it locally
//! - `find_url_at` — return the URL at a click position
//! - `forward_key_to_server` — forward to the PTY (special keys / Ctrl sequences)
//! - `check_config_keybindings` — check custom bindings from `config.keys`

use nexterm_proto::ClientToServer;
use nexterm_proto::KeyCode as ProtoKeyCode;
use winit::{
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode as WKeyCode, PhysicalKey},
};

use crate::key_map::{
    config_key_matches, config_key_matches_token, format_key_event, physical_to_proto_key,
    proto_modifiers, winit_code_to_char,
};
use crate::vertex_util::grid_to_text;

use super::EventHandler;

// ---- Submodules ----
mod action;
mod copy_mode;
mod font;
mod special_modes;
mod ssh;

impl EventHandler {
    /// Handle a key; return true if it was consumed locally
    pub(super) fn handle_key(&mut self, code: WKeyCode, event_loop: &ActiveEventLoop) -> bool {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        // Sprint 4-1: while the consent dialog is open it consumes every key
        if self.app.state.pending_consent.is_some() {
            return self.handle_consent_dialog_key(code);
        }

        // Sprint 5-9 Phase 4-6: while the close-window confirmation dialog is
        // open it also consumes every key (same priority as the consent dialog).
        if self.app.state.close_window_dialog.is_some() {
            return self.handle_close_window_dialog_key(code);
        }

        // Ctrl+Shift+V: paste from the clipboard
        if ctrl && shift && code == WKeyCode::KeyV {
            if let Ok(mut clipboard) = arboard::Clipboard::new()
                && let Ok(text) = clipboard.get_text()
                && let Some(conn) = &self.connection
            {
                let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
            }
            return true;
        }

        // Ctrl+Shift+C: copy the visible grid of the focused pane to the clipboard
        if ctrl && shift && code == WKeyCode::KeyC {
            if let Some(pane) = self.app.state.focused_pane() {
                let text = grid_to_text(pane);
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
            }
            return true;
        }

        // Ctrl+Shift+P: toggle the command palette
        if ctrl && shift && code == WKeyCode::KeyP {
            if self.app.state.palette.is_open {
                self.app.state.palette.close();
            } else {
                self.app.state.palette.open();
            }
            return true;
        }

        // Ctrl+Shift+U: open the SFTP upload dialog
        if ctrl && shift && code == WKeyCode::KeyU {
            self.app.state.file_transfer.open_upload();
            return true;
        }

        // Ctrl+Shift+D: open the SFTP download dialog
        if ctrl && shift && code == WKeyCode::KeyD {
            self.app.state.file_transfer.open_download();
            return true;
        }

        // Ctrl+Shift+M: toggle the Lua macro picker
        if ctrl && shift && code == WKeyCode::KeyM {
            if self.app.state.macro_picker.is_open {
                self.app.state.macro_picker.close();
            } else {
                self.app
                    .state
                    .macro_picker
                    .reload(self.app.config.macros.clone());
                self.app.state.macro_picker.open();
            }
            return true;
        }

        // Ctrl+Shift+H: toggle the host manager
        if ctrl && shift && code == WKeyCode::KeyH {
            if self.app.state.host_manager.is_open {
                self.app.state.host_manager.close();
            } else {
                // Reload the configured host list before opening
                self.app
                    .state
                    .host_manager
                    .reload(self.app.config.hosts.clone());
                self.app.state.host_manager.open();
            }
            return true;
        }

        // Ctrl+,: toggle the settings panel
        if ctrl && code == WKeyCode::Comma {
            if self.app.state.settings_panel.is_open {
                self.app.state.settings_panel.close();
            } else {
                self.app.state.settings_panel.open();
            }
            return true;
        }

        // Ctrl+F: start a scrollback search
        if ctrl && code == WKeyCode::KeyF {
            self.app.state.start_search();
            return true;
        }

        // Ctrl+[ : enter copy mode (tmux-compatible)
        if ctrl && code == WKeyCode::BracketLeft {
            if !self.app.state.copy_mode.is_active {
                let (col, row) = self
                    .app
                    .state
                    .focused_pane()
                    .map(|p| (p.cursor_col, p.cursor_row))
                    .unwrap_or((0, 0));
                self.app.state.copy_mode.enter(col, row);
            }
            return true;
        }

        // Key handling while in copy mode
        if self.app.state.copy_mode.is_active {
            return self.handle_copy_mode_key(code);
        }

        // Key handling while in Quick Select mode
        if self.app.state.quick_select.is_active {
            return self.handle_quick_select_key(code);
        }

        // Key handling while the file-transfer dialog is open (consumes every key)
        if self.app.state.file_transfer.is_open {
            match code {
                WKeyCode::Escape => self.app.state.file_transfer.close(),
                WKeyCode::Tab | WKeyCode::ArrowDown => self.app.state.file_transfer.next_field(),
                WKeyCode::ArrowUp => self.app.state.file_transfer.prev_field(),
                WKeyCode::Backspace => {
                    self.app.state.file_transfer.current_field_mut().pop();
                }
                WKeyCode::Enter => {
                    let ft = &self.app.state.file_transfer;
                    if !ft.host_name.is_empty()
                        && !ft.local_path.is_empty()
                        && !ft.remote_path.is_empty()
                    {
                        let msg = if ft.mode == "upload" {
                            ClientToServer::SftpUpload {
                                host_name: ft.host_name.clone(),
                                local_path: ft.local_path.clone(),
                                remote_path: ft.remote_path.clone(),
                            }
                        } else {
                            ClientToServer::SftpDownload {
                                host_name: ft.host_name.clone(),
                                remote_path: ft.remote_path.clone(),
                                local_path: ft.local_path.clone(),
                            }
                        };
                        if let Some(conn) = &self.connection {
                            let _ = conn.send_tx.try_send(msg);
                        }
                        self.app.state.file_transfer.close();
                    }
                }
                _ => {
                    if let Some(ch) = winit_code_to_char(code) {
                        self.app.state.file_transfer.current_field_mut().push(ch);
                    }
                }
            }
            return true;
        }

        // Key handling while in tab-rename mode (consumes every key)
        if self.app.state.settings_panel.tab_rename_editing.is_some() {
            match code {
                WKeyCode::Escape => {
                    self.app.state.settings_panel.cancel_tab_rename();
                }
                WKeyCode::Enter => {
                    let rename_id = self.app.state.settings_panel.tab_rename_editing;
                    let new_name = self.app.state.settings_panel.tab_rename_text.clone();
                    self.app.state.settings_panel.cancel_tab_rename();
                    if let (Some(window_id), Some(conn)) = (rename_id, &self.connection)
                        && !new_name.is_empty()
                    {
                        let _ = conn.send_tx.try_send(ClientToServer::RenameWindow {
                            window_id,
                            name: new_name,
                        });
                    }
                }
                WKeyCode::Backspace => {
                    self.app.state.settings_panel.pop_tab_rename_char();
                }
                _ => {
                    // Accept letter / digit / symbol input
                    if let Some(ch) = winit_code_to_char(code) {
                        let ch = if self.modifiers.shift_key() {
                            ch.to_uppercase().next().unwrap_or(ch)
                        } else {
                            ch
                        };
                        self.app.state.settings_panel.push_tab_rename_char(ch);
                    }
                }
            }
            return true;
        }

        // Navigation while the macro picker is open (consumes every key)
        if self.app.state.macro_picker.is_open {
            match code {
                WKeyCode::ArrowDown => self.app.state.macro_picker.select_next(),
                WKeyCode::ArrowUp => self.app.state.macro_picker.select_prev(),
                WKeyCode::Escape => self.app.state.macro_picker.close(),
                WKeyCode::Backspace => self.app.state.macro_picker.pop_char(),
                WKeyCode::Enter => {
                    if let Some(mac) = self.app.state.macro_picker.selected_macro() {
                        let fn_name = mac.lua_fn.clone();
                        let display_name = mac.name.clone();
                        self.app.state.macro_picker.close();
                        if let Some(conn) = &self.connection {
                            let _ = conn.send_tx.try_send(ClientToServer::RunMacro {
                                macro_fn: fn_name,
                                display_name,
                            });
                        }
                    }
                }
                _ => {
                    if let Some(ch) = winit_code_to_char(code) {
                        self.app.state.macro_picker.push_char(ch);
                    }
                }
            }
            return true;
        }

        // PageUp / PageDown: scroll the scrollback
        if code == WKeyCode::PageUp {
            let scroll_lines = self.app.state.rows as usize / 2;
            self.app.state.scroll_up(scroll_lines);
            return true;
        }
        if code == WKeyCode::PageDown {
            let scroll_lines = self.app.state.rows as usize / 2;
            self.app.state.scroll_down(scroll_lines);
            return true;
        }

        // Ctrl+Shift+ArrowUp / ArrowDown: jump to the previous/next shell prompt (Sprint 5-2 / B1)
        // Follows the anchors recorded by OSC 133 A (PromptStart)
        if ctrl && shift && code == WKeyCode::ArrowUp {
            self.app.state.jump_prev_prompt();
            return true;
        }
        if ctrl && shift && code == WKeyCode::ArrowDown {
            self.app.state.jump_next_prompt();
            return true;
        }

        // Escape: close search / palette / host manager
        if code == WKeyCode::Escape {
            if self.app.state.settings_panel.is_open {
                self.app.state.settings_panel.close();
                return true;
            } else if self.app.state.palette.is_open {
                self.app.state.palette.close();
                return true;
            } else if self.app.state.host_manager.is_open {
                self.app.state.host_manager.close();
                return true;
            } else if self.app.state.macro_picker.is_open {
                self.app.state.macro_picker.close();
                return true;
            } else if self.app.state.file_transfer.is_open {
                self.app.state.file_transfer.close();
                return true;
            } else if self.app.state.search.is_active {
                self.app.state.end_search();
                return true;
            }
            // If neither palette nor search is open, forward to the PTY
            return false;
        }

        // Navigation while the settings panel is open (consumes every key)
        if self.app.state.settings_panel.is_open {
            let font_editing = self.app.state.settings_panel.font_family_editing;
            // Phase 5-11-8 Step 8-3 (Sub-phase A): SSH field-edit mode
            let ssh_editing = self.app.state.settings_panel.ssh_field_editing.is_some();
            // Phase 5-11-8 Step 8-3 (Sub-phase D): the SSH delete confirmation dialog.
            //   While it is open we absorb every key so dialog operations have
            //   exclusive focus. Treated as higher priority than the editing
            //   flags (you cannot open the dialog while editing by design).
            let dialog_open = self.app.state.settings_panel.ssh_delete_dialog_open;
            // Phase 5-11-9 Sub-phase D: key-binding delete-confirmation dialog.
            //   Like the SSH dialog, while open it has exclusive focus.
            let key_dialog_open = self.app.state.settings_panel.key_delete_dialog_open;
            // Phase 5-11-9 Sub-phase B: key-field edit mode (Record / Text).
            let key_recording = self.app.state.settings_panel.is_key_recording();
            let key_text_editing = self.app.state.settings_panel.is_key_text_editing();
            let key_editing = key_recording || key_text_editing;
            let editing =
                font_editing || ssh_editing || dialog_open || key_dialog_open || key_editing;
            match code {
                // ===== Sub-phase D: dedicated handling while the delete dialog is open (highest priority) =====
                WKeyCode::Escape if dialog_open => {
                    self.app.state.settings_panel.cancel_ssh_delete_dialog();
                }
                WKeyCode::Enter if dialog_open => {
                    let sp = &mut self.app.state.settings_panel;
                    if sp.ssh_delete_dialog_confirm_focused {
                        sp.confirm_ssh_delete_dialog();
                    } else {
                        sp.cancel_ssh_delete_dialog();
                    }
                }
                WKeyCode::ArrowLeft | WKeyCode::ArrowRight | WKeyCode::Tab if dialog_open => {
                    self.app
                        .state
                        .settings_panel
                        .toggle_ssh_delete_dialog_focus();
                }
                // ===== Sub-phase D: dedicated handling while the key-binding delete dialog is open =====
                WKeyCode::Escape if key_dialog_open => {
                    self.app.state.settings_panel.cancel_key_delete_dialog();
                }
                WKeyCode::Enter if key_dialog_open => {
                    let sp = &mut self.app.state.settings_panel;
                    if sp.key_delete_dialog_confirm_focused {
                        sp.confirm_key_delete_dialog();
                    } else {
                        sp.cancel_key_delete_dialog();
                    }
                }
                WKeyCode::ArrowLeft | WKeyCode::ArrowRight | WKeyCode::Tab if key_dialog_open => {
                    self.app
                        .state
                        .settings_panel
                        .toggle_key_delete_dialog_focus();
                }
                // ===== Phase 5-11-9 Sub-phase B: key-field edit handling =====
                WKeyCode::Escape if key_editing => {
                    self.app.state.settings_panel.cancel_key_edit();
                }
                WKeyCode::Tab if key_editing => {
                    // Toggle between Record and Text mode.
                    self.app.state.settings_panel.toggle_key_edit_mode();
                }
                WKeyCode::Enter if key_text_editing => {
                    // Commit Text mode (Record mode commits on capture).
                    self.app.state.settings_panel.commit_key_edit();
                }
                WKeyCode::Backspace if key_text_editing => {
                    self.app.state.settings_panel.key_field_backspace();
                }
                WKeyCode::Delete if key_text_editing => {
                    self.app.state.settings_panel.key_field_delete();
                }
                WKeyCode::ArrowLeft if key_text_editing => {
                    self.app.state.settings_panel.key_field_move_left();
                }
                WKeyCode::ArrowRight if key_text_editing => {
                    self.app.state.settings_panel.key_field_move_right();
                }
                WKeyCode::Home if key_text_editing => {
                    self.app.state.settings_panel.key_field_move_home();
                }
                WKeyCode::End if key_text_editing => {
                    self.app.state.settings_panel.key_field_move_end();
                }
                // Record-mode capture: any other key press becomes the binding.
                // Modifier-only presses (ShiftLeft/ShiftRight/ControlLeft/...) are
                // filtered out because `format_key_event` returns None for them.
                _ if key_recording => {
                    if let Some(formatted) = format_key_event(code, self.modifiers) {
                        self.app.state.settings_panel.capture_key_record(formatted);
                    }
                    // Either way (captured or filtered) consume the key.
                }
                WKeyCode::Escape => {
                    if font_editing {
                        // Exit edit mode (do not discard changes, just leave input mode)
                        self.app.state.settings_panel.font_family_editing = false;
                    } else if ssh_editing {
                        // Sub-phase A: cancel SSH field editing (discard buffer)
                        self.app.state.settings_panel.cancel_ssh_field_edit();
                    } else {
                        self.app.state.settings_panel.close();
                    }
                }
                WKeyCode::Enter => {
                    if font_editing {
                        // Commit edit mode
                        self.app.state.settings_panel.font_family_editing = false;
                    } else if ssh_editing {
                        // Sub-phase A: commit SSH field editing (write the buffer back into host)
                        self.app.state.settings_panel.commit_ssh_field_edit();
                    } else {
                        // Sub-phase A: in the SSH category, focus on fields (1/2/4) enters edit mode.
                        // Sub-phase D: focus=6 (Add) calls add_ssh_host (which starts editing internally).
                        // Sub-phase D: focus=7 (Delete) opens the delete confirmation dialog.
                        use crate::settings_panel::SettingsCategory;
                        let sp = &mut self.app.state.settings_panel;
                        if sp.category == SettingsCategory::Ssh
                            && matches!(sp.ssh_field_focus, 1 | 2 | 4)
                            && sp.begin_ssh_field_edit()
                        {
                            // Phase 5-11-8 Step 8-3 (Sub-phase B): when entering edit mode,
                            // move the IME cursor area onto the SSH field row.
                            self.update_ime_cursor_area_for_ssh_field();
                        } else if sp.category == SettingsCategory::Ssh && sp.ssh_field_focus == 6 {
                            // Sub-phase D: Add button → add a new host + auto-start name editing
                            sp.add_ssh_host();
                            self.update_ime_cursor_area_for_ssh_field();
                        } else if sp.category == SettingsCategory::Ssh
                            && sp.ssh_field_focus == 7
                            && !sp.ssh_hosts.is_empty()
                        {
                            // Sub-phase D: Delete button → open the delete confirmation dialog.
                            //   When the list is empty, treat as disabled and do nothing
                            //   (to prevent accidental presses).
                            sp.open_ssh_delete_dialog();
                        } else if sp.category == SettingsCategory::Keybindings
                            && sp.key_field_focus == 1
                            && sp.begin_key_record()
                        {
                            // Phase 5-11-9 Sub-phase B: Enter on the key field starts
                            // Record mode (the next physical press is captured).
                            // Tab inside Record mode switches to Text mode for
                            // prefix bindings like "ctrl+b d".
                        } else if sp.category == SettingsCategory::Keybindings
                            && sp.key_field_focus == 3
                        {
                            // Phase 5-11-9 Sub-phase D: Add button → append a new
                            // binding and auto-start Record mode on the key field.
                            sp.add_key_binding();
                        } else if sp.category == SettingsCategory::Keybindings
                            && sp.key_field_focus == 4
                            && !sp.keybindings.is_empty()
                        {
                            // Phase 5-11-9 Sub-phase D: Delete button → open the
                            // confirmation dialog. With an empty list it is
                            // treated as disabled.
                            sp.open_key_delete_dialog();
                        } else {
                            let _ = sp.save_to_toml();
                            sp.close();
                        }
                    }
                }
                WKeyCode::Backspace if font_editing => {
                    self.app.state.settings_panel.pop_font_family_char();
                }
                WKeyCode::Backspace if ssh_editing => {
                    self.app.state.settings_panel.ssh_field_backspace();
                }
                WKeyCode::Delete if ssh_editing => {
                    self.app.state.settings_panel.ssh_field_delete();
                }
                WKeyCode::Home if ssh_editing => {
                    self.app.state.settings_panel.ssh_field_move_home();
                }
                WKeyCode::End if ssh_editing => {
                    self.app.state.settings_panel.ssh_field_move_end();
                }
                // F key toggles font-family edit mode in the Font category
                WKeyCode::KeyF if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Font {
                        self.app.state.settings_panel.font_family_editing = true;
                    }
                }
                WKeyCode::Tab | WKeyCode::ArrowDown if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    // Phase 5-11-6 #6: in the Window category, reinterpret ↓ as field navigation.
                    // Once past the last field, fall through to the next category.
                    // Phase 5-11-8 Step 8-3 (Sub-phase A): the Ssh category similarly reinterprets ↓
                    // as moving `ssh_field_focus` 0 → 1 → 2 → 3 → 4 → 5.
                    let sp = &mut self.app.state.settings_panel;
                    if sp.category == SettingsCategory::Window && code == WKeyCode::ArrowDown {
                        if !sp.next_window_field() {
                            sp.next_category();
                            sp.window_field_focus = 0;
                        }
                    } else if sp.category == SettingsCategory::Ssh && code == WKeyCode::ArrowDown {
                        // Phase 5-11-8 Step 8-3 (Sub-phase D): widened to 0..=7
                        //   6=Add, 7=Delete. With an empty list, Delete (7) is treated as
                        //   disabled and skipped, stopping at 6 (Add) (next press → next category).
                        let max_focus = if sp.ssh_hosts.is_empty() { 6 } else { 7 };
                        if sp.ssh_field_focus < max_focus {
                            sp.ssh_field_focus += 1;
                        } else {
                            sp.next_category();
                            sp.ssh_field_focus = 0;
                        }
                    } else if sp.category == SettingsCategory::Keybindings
                        && code == WKeyCode::ArrowDown
                    {
                        // Phase 5-11-9 Sub-phase A: ↓ walks key_field_focus 0 → 1 → 2.
                        // Sub-phase D extends the range to 3 (Add) and 4 (Delete).
                        //   - When the list is empty: 0 (ListBox) → 3 (Add) → next category.
                        //     Delete (4) is treated as disabled and skipped.
                        //   - With entries: 0 → 1 → 2 → 3 → 4 → next category.
                        if sp.keybindings.is_empty() {
                            if sp.key_field_focus < 3 {
                                // 0 → 3 directly (skip 1/2 which require a selected binding).
                                sp.key_field_focus = 3;
                            } else {
                                sp.next_category();
                                sp.key_field_focus = 0;
                            }
                        } else if sp.key_field_focus < 4 {
                            sp.key_field_focus += 1;
                        } else {
                            sp.next_category();
                            sp.key_field_focus = 0;
                        }
                    } else {
                        sp.next_category();
                        sp.window_field_focus = 0;
                        sp.ssh_field_focus = 0;
                        sp.key_field_focus = 0;
                    }
                }
                WKeyCode::ArrowUp if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    let sp = &mut self.app.state.settings_panel;
                    match &sp.category {
                        SettingsCategory::Font => sp.increase_font_size(),
                        SettingsCategory::Window => {
                            // Phase 5-11-6 #6: ↑ navigates between fields. At the top, fall back to the previous category.
                            if !sp.prev_window_field() {
                                sp.prev_category();
                                sp.window_field_focus = 0;
                            }
                        }
                        SettingsCategory::Ssh => {
                            // Phase 5-11-8 Step 8-3 (Sub-phase A): ↑ moves ssh_field_focus back by one
                            if sp.ssh_field_focus > 0 {
                                sp.ssh_field_focus -= 1;
                            } else {
                                sp.prev_category();
                                sp.ssh_field_focus = 0;
                            }
                        }
                        SettingsCategory::Keybindings => {
                            // Phase 5-11-9 Sub-phase A: ↑ moves key_field_focus back by one.
                            // Sub-phase D: when the list is empty, jump from 3 (Add)
                            //   directly to 0 (ListBox) — focuses 1/2 require a selection.
                            if sp.keybindings.is_empty() && sp.key_field_focus == 3 {
                                sp.key_field_focus = 0;
                            } else if sp.key_field_focus > 0 {
                                sp.key_field_focus -= 1;
                            } else {
                                sp.prev_category();
                                sp.key_field_focus = 0;
                            }
                        }
                        _ => sp.prev_category(),
                    }
                }
                WKeyCode::ArrowRight if ssh_editing => {
                    self.app.state.settings_panel.ssh_field_move_right();
                }
                WKeyCode::ArrowRight if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    match &self.app.state.settings_panel.category {
                        SettingsCategory::Theme => self.app.state.settings_panel.next_scheme(),
                        SettingsCategory::Startup => self.app.state.settings_panel.next_language(),
                        // Phase 5-11-6 #6: Window category — increment the value of the focused field
                        SettingsCategory::Window => {
                            self.app.state.settings_panel.window_field_increase()
                        }
                        // Phase 5-11-8 Step 8-3 (Sub-phase C): allow → / ← to inc-dec / cycle
                        // SSH `port` (SpinButton) and `auth_type` (ComboBox).
                        SettingsCategory::Ssh => {
                            let sp = &mut self.app.state.settings_panel;
                            match sp.ssh_field_focus {
                                3 => sp.increase_ssh_host_port(),
                                5 => sp.next_ssh_auth_type(),
                                _ => {}
                            }
                        }
                        // Phase 5-11-9 Sub-phase C: → cycles the action ComboBox forward
                        // when the action field (key_field_focus == 2) is focused.
                        SettingsCategory::Keybindings => {
                            let sp = &mut self.app.state.settings_panel;
                            if sp.key_field_focus == 2 {
                                sp.next_key_action();
                            }
                        }
                        _ => {}
                    }
                }
                WKeyCode::ArrowLeft if ssh_editing => {
                    self.app.state.settings_panel.ssh_field_move_left();
                }
                WKeyCode::ArrowLeft if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    match &self.app.state.settings_panel.category {
                        SettingsCategory::Theme => self.app.state.settings_panel.prev_scheme(),
                        SettingsCategory::Startup => self.app.state.settings_panel.prev_language(),
                        // Phase 5-11-6 #6: Window category — decrement the value of the focused field
                        SettingsCategory::Window => {
                            self.app.state.settings_panel.window_field_decrease()
                        }
                        // Phase 5-11-8 Step 8-3 (Sub-phase C): allow ← to decrement / reverse-cycle
                        // SSH `port` (SpinButton) and `auth_type` (ComboBox).
                        SettingsCategory::Ssh => {
                            let sp = &mut self.app.state.settings_panel;
                            match sp.ssh_field_focus {
                                3 => sp.decrease_ssh_host_port(),
                                5 => sp.prev_ssh_auth_type(),
                                _ => {}
                            }
                        }
                        // Phase 5-11-9 Sub-phase C: ← cycles the action ComboBox backward
                        // when the action field (key_field_focus == 2) is focused.
                        SettingsCategory::Keybindings => {
                            let sp = &mut self.app.state.settings_panel;
                            if sp.key_field_focus == 2 {
                                sp.prev_key_action();
                            }
                        }
                        _ => {}
                    }
                }
                // Space: toggle auto_check_update in the Startup category
                WKeyCode::Space if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Startup {
                        let sp = &mut self.app.state.settings_panel;
                        sp.auto_check_update = !sp.auto_check_update;
                    }
                }
                WKeyCode::BracketRight if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Theme {
                        self.app.state.settings_panel.next_scheme();
                    }
                }
                WKeyCode::BracketLeft if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Theme {
                        self.app.state.settings_panel.prev_scheme();
                    }
                }
                _ => {}
            }
            return true;
        }

        // Navigation while the palette is open (consumes every key)
        if self.app.state.palette.is_open {
            match code {
                WKeyCode::ArrowDown => self.app.state.palette.select_next(),
                WKeyCode::ArrowUp => self.app.state.palette.select_prev(),
                WKeyCode::Enter => {
                    if let Some(action) = self.app.state.palette.selected_action() {
                        let action_id = action.action.clone();
                        self.app.state.palette.close();
                        // Sprint 5-7 / Phase 3-3: record usage history and persist it
                        self.app.state.palette.record_use(&action_id);
                        self.execute_action(&action_id, event_loop);
                    }
                }
                _ => {}
            }
            return true;
        }

        // When the server-error banner is visible: Esc closes it (Sprint 5-12 Phase 1).
        // Processed before update_banner so the overlapping banner clears first.
        if self.app.state.error_banner.is_some()
            && let WKeyCode::Escape = code
        {
            self.app.state.error_banner = None;
            return true;
        }

        // While the update-notification banner is visible: Esc closes it, Enter opens the browser
        if self.app.state.update_banner.is_some() {
            match code {
                WKeyCode::Escape => {
                    self.app.state.update_banner = None;
                    return true;
                }
                WKeyCode::Enter => {
                    crate::platform::open_releases_url();
                    self.app.state.update_banner = None;
                    return true;
                }
                _ => {}
            }
        }

        // Navigation while the host manager is open (consumes every key).
        // When the password modal is open, use its dedicated handling.
        if self.app.state.host_manager.password_modal.is_some() {
            match code {
                WKeyCode::Escape => {
                    self.app.state.host_manager.password_modal = None;
                }
                WKeyCode::Tab => {
                    // Toggle the "save to OS keychain" flag (later half of Sprint 3-2)
                    if let Some(m) = &mut self.app.state.host_manager.password_modal {
                        m.toggle_remember();
                    }
                }
                WKeyCode::Backspace => {
                    if let Some(m) = &mut self.app.state.host_manager.password_modal {
                        m.pop_char();
                    }
                }
                WKeyCode::Enter => {
                    if let Some(m) = &mut self.app.state.host_manager.password_modal {
                        let host = m.host.clone();
                        // Sprint 5-1 / G1: read `remember` before `take_password()`
                        // (sent over IPC as `ephemeral_password = !remember`).
                        let remember = m.remember;
                        let password = m.take_password();
                        self.app.state.host_manager.password_modal = None;
                        self.app.state.host_manager.record_connection(&host);
                        self.connect_ssh_host_with_password(&host, password, remember);
                    }
                }
                _ => {
                    if let Some(ch) = winit_code_to_char(code)
                        && let Some(m) = &mut self.app.state.host_manager.password_modal
                    {
                        m.push_char(ch);
                    }
                }
            }
            return true;
        }

        if self.app.state.host_manager.is_open {
            match code {
                WKeyCode::ArrowDown => self.app.state.host_manager.select_next(),
                WKeyCode::ArrowUp => self.app.state.host_manager.select_prev(),
                WKeyCode::Escape => self.app.state.host_manager.close(),
                WKeyCode::Backspace => self.app.state.host_manager.pop_char(),
                WKeyCode::Enter => {
                    if let Some(host) = self.app.state.host_manager.selected_host() {
                        let host = host.clone();
                        self.app.state.host_manager.close();
                        if host.auth_type == "password" {
                            // For password-auth hosts, open the modal first, then connect
                            self.app.state.host_manager.password_modal =
                                Some(crate::host_manager::PasswordModal::new(host));
                        } else {
                            self.app.state.host_manager.record_connection(&host);
                            self.connect_ssh_host_new_tab(&host);
                        }
                    }
                }
                _ => {
                    if let Some(ch) = winit_code_to_char(code) {
                        self.app.state.host_manager.push_char(ch);
                    }
                }
            }
            return true;
        }

        // Special keys for search mode
        if self.app.state.search.is_active {
            match code {
                // Enter: next match / Shift+Enter: previous match
                WKeyCode::Enter => {
                    if shift {
                        self.app.state.search_prev();
                    } else {
                        self.app.state.search_next();
                    }
                    return true;
                }
                // N: previous match (vim convention)
                WKeyCode::KeyN if shift => {
                    self.app.state.search_prev();
                    return true;
                }
                _ => {}
            }
        }

        // Ctrl+= (Equal / Plus): increase font size
        if ctrl && (code == WKeyCode::Equal || code == WKeyCode::NumpadAdd) {
            self.change_font_size(1.0);
            return true;
        }

        // Ctrl+- : decrease font size
        if ctrl && (code == WKeyCode::Minus || code == WKeyCode::NumpadSubtract) {
            self.change_font_size(-1.0);
            return true;
        }

        // Ctrl+0 : reset font size to the default
        if ctrl && code == WKeyCode::Digit0 {
            self.reset_font_size();
            return true;
        }

        // Sprint 5-7 / UI-1-4 + bug fix: detect a lone Leader press and enter prefix mode.
        // We only enter prefix mode (and suppress PTY forwarding) when `leader_key`
        // (e.g. "ctrl+b") matches the current modifier+key AND at least one
        // `<leader> X` style binding is configured. With no prefix bindings,
        // pass through as a normal Ctrl+B etc. to the PTY (to avoid breaking
        // existing user workflows).
        let leader_str = self.app.config.leader_key.clone();
        if !leader_str.is_empty()
            && config_key_matches(&leader_str, code, self.modifiers)
            && self.has_prefix_bindings()
        {
            let now = std::time::Instant::now();
            let until = now + std::time::Duration::from_secs(2);
            self.app.state.key_hint_visible_until = Some(until);
            self.app.state.prefix_pending_until = Some(until);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return true; // Do not forward to the PTY (consume the prefix-mode entry)
        }

        // Check the custom key bindings from the config file
        if self.check_config_keybindings(code, event_loop) {
            return true;
        }

        false
    }

    /// Return true if any `<leader> X` style prefix binding is configured.
    /// Used to decide whether to enter prefix mode on a lone Leader press.
    fn has_prefix_bindings(&self) -> bool {
        let leader = &self.app.config.leader_key;
        if leader.is_empty() {
            return false;
        }
        self.app.config.keys.iter().any(|b| {
            let expanded = self.app.config.expand_leader(&b.key);
            let mut tokens = expanded.split_whitespace();
            let first = tokens.next();
            // At least two tokens, and the first matches the leader
            first.is_some_and(|t| t.eq_ignore_ascii_case(leader)) && tokens.next().is_some()
        })
    }

    /// Return the URL at click coordinates (col, row), if any
    pub(super) fn find_url_at(&self, col: u16, row: u16) -> Option<String> {
        use crate::state::detect_urls_in_row;
        let pane = self.app.state.focused_pane()?;

        // Check OSC 8 hyperlinks first
        for span in &pane.grid.hyperlinks {
            if span.row == row && col >= span.col_start && col < span.col_end {
                return Some(span.url.clone());
            }
        }

        // Then detect URLs dynamically from text patterns
        let cells = pane.grid.rows.get(row as usize)?;
        let urls = detect_urls_in_row(row, cells);
        urls.into_iter()
            .find(|u| u.contains(col, row))
            .map(|u| u.url)
    }

    /// Search the configured key bindings for a match and execute the action.
    /// Returns true if the key was consumed.
    ///
    /// Sprint 5-7 / UI-1-3 + bug fix: dispatches via two paths:
    /// - **In prefix mode** (`prefix_pending_until` active): only match
    ///   `<leader> X` style bindings. For entries whose first token equals the
    ///   leader, compare the remaining tokens against the incoming key.
    ///   On a match, execute and exit prefix mode. Even on no match, exit
    ///   prefix mode and fall through to single-binding matching (so the next
    ///   key still works as normal input; this key is not consumed).
    /// - **Outside prefix mode**: skip space-separated bindings and only match single-token ones.
    fn check_config_keybindings(&mut self, code: WKeyCode, event_loop: &ActiveEventLoop) -> bool {
        let bindings = self.app.config.keys.clone();
        let leader = self.app.config.leader_key.clone();
        let now = std::time::Instant::now();

        let in_prefix = matches!(
            self.app.state.prefix_pending_until,
            Some(t) if now < t
        );

        if in_prefix {
            // In prefix mode: only match `<leader> X` style bindings
            for binding in &bindings {
                let expanded = self.app.config.expand_leader(&binding.key);
                let tokens: Vec<&str> = expanded.split_whitespace().collect();
                if tokens.len() < 2 {
                    continue;
                }
                // The first token must match the leader
                if !tokens[0].eq_ignore_ascii_case(leader.as_str()) {
                    continue;
                }
                // Concatenate the remaining tokens (future-proof for multi-step prefixes; today single-token)
                let rest = tokens[1..].join(" ");
                if config_key_matches_token(&rest, code, self.modifiers) {
                    let action = binding.action.clone();
                    self.app.state.prefix_pending_until = None;
                    self.app.state.key_hint_visible_until = None;
                    self.execute_action(&action, event_loop);
                    return true;
                }
            }
            // No match in prefix mode: exit the mode and fall through to single-binding matching.
            // (We do not consume this key; if there is no match downstream, it ends up as PTY input.)
            self.app.state.prefix_pending_until = None;
            self.app.state.key_hint_visible_until = None;
        }

        // Single-binding matching (outside prefix mode, or as fall-through after no prefix match)
        for binding in &bindings {
            let expanded = self.app.config.expand_leader(&binding.key);
            // Skip space-separated bindings (prefix-style) on this path
            if expanded.split_whitespace().count() > 1 {
                continue;
            }
            if config_key_matches(&expanded, code, self.modifiers) {
                let action = binding.action.clone();
                self.execute_action(&action, event_loop);
                return true;
            }
        }
        false
    }

    /// Forward a key input to the server-side PTY
    pub(super) fn forward_key_to_server(&self, physical_key: PhysicalKey, text: Option<&str>) {
        let Some(conn) = &self.connection else { return };
        let mods = proto_modifiers(self.modifiers);
        let ctrl = self.modifiers.control_key();

        // When Ctrl is not held and text is present, send it as text input
        if !ctrl
            && let Some(text_str) = text
            && !text_str.is_empty()
        {
            for ch in text_str.chars() {
                let _ = conn.send_tx.try_send(ClientToServer::KeyEvent {
                    code: ProtoKeyCode::Char(ch),
                    modifiers: mods,
                });
            }
            return;
        }

        // Special keys and Ctrl key sequences
        if let Some(key_code) = physical_to_proto_key(physical_key, self.modifiers) {
            let _ = conn.send_tx.try_send(ClientToServer::KeyEvent {
                code: key_code,
                modifiers: mods,
            });
        }
    }
}
