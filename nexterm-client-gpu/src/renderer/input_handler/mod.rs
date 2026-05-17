//! 入力処理ハンドラ
//!
//! Sprint 5-6 で旧 `input_handler.rs`（1,377 行）を 6 サブモジュールに分割：
//! - `copy_mode` — コピーモード（tmux 互換）のキー入力
//! - `action` — config.keys / コンテキストメニューのアクション実行
//! - `ssh` — SSH 接続ヘルパー
//! - `font` — フォントサイズ変更（Ctrl++/-, Ctrl+0）
//! - `special_modes` — Quick Select / 同意ダイアログのキー入力
//!
//! 本ファイルはトップレベルディスパッチ:
//! - `handle_key` — winit キーイベントを解釈してローカル消費判定する
//! - `find_url_at` — クリック位置の URL を返す
//! - `forward_key_to_server` — PTY 転送（特殊キー / Ctrl シーケンス）
//! - `check_config_keybindings` — config.keys のカスタムバインドチェック

use nexterm_proto::ClientToServer;
use nexterm_proto::KeyCode as ProtoKeyCode;
use winit::{
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode as WKeyCode, PhysicalKey},
};

use crate::key_map::{
    config_key_matches, config_key_matches_token, physical_to_proto_key, proto_modifiers,
    winit_code_to_char,
};
use crate::vertex_util::grid_to_text;

use super::EventHandler;

// ---- サブモジュール ----
mod action;
mod copy_mode;
mod font;
mod special_modes;
mod ssh;

impl EventHandler {
    /// キーを処理してローカルで消費した場合は true を返す
    pub(super) fn handle_key(&mut self, code: WKeyCode, event_loop: &ActiveEventLoop) -> bool {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        // Sprint 4-1: 同意ダイアログが開いている間はすべてのキーをダイアログが消費する
        if self.app.state.pending_consent.is_some() {
            return self.handle_consent_dialog_key(code);
        }

        // Ctrl+Shift+V: クリップボードからペーストする
        if ctrl && shift && code == WKeyCode::KeyV {
            if let Ok(mut clipboard) = arboard::Clipboard::new()
                && let Ok(text) = clipboard.get_text()
                && let Some(conn) = &self.connection
            {
                let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
            }
            return true;
        }

        // Ctrl+Shift+C: フォーカスペインの可視グリッドをクリップボードにコピーする
        if ctrl && shift && code == WKeyCode::KeyC {
            if let Some(pane) = self.app.state.focused_pane() {
                let text = grid_to_text(pane);
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
            }
            return true;
        }

        // Ctrl+Shift+P: コマンドパレットのトグル
        if ctrl && shift && code == WKeyCode::KeyP {
            if self.app.state.palette.is_open {
                self.app.state.palette.close();
            } else {
                self.app.state.palette.open();
            }
            return true;
        }

        // Ctrl+Shift+U: SFTP アップロードダイアログを開く
        if ctrl && shift && code == WKeyCode::KeyU {
            self.app.state.file_transfer.open_upload();
            return true;
        }

        // Ctrl+Shift+D: SFTP ダウンロードダイアログを開く
        if ctrl && shift && code == WKeyCode::KeyD {
            self.app.state.file_transfer.open_download();
            return true;
        }

        // Ctrl+Shift+M: Lua マクロピッカーのトグル
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

        // Ctrl+Shift+H: ホストマネージャのトグル
        if ctrl && shift && code == WKeyCode::KeyH {
            if self.app.state.host_manager.is_open {
                self.app.state.host_manager.close();
            } else {
                // 設定ホスト一覧を最新にリロードしてから開く
                self.app
                    .state
                    .host_manager
                    .reload(self.app.config.hosts.clone());
                self.app.state.host_manager.open();
            }
            return true;
        }

        // Ctrl+,: 設定パネルをトグルする
        if ctrl && code == WKeyCode::Comma {
            if self.app.state.settings_panel.is_open {
                self.app.state.settings_panel.close();
            } else {
                self.app.state.settings_panel.open();
            }
            return true;
        }

        // Ctrl+F: スクロールバック検索を開始する
        if ctrl && code == WKeyCode::KeyF {
            self.app.state.start_search();
            return true;
        }

        // Ctrl+[ : コピーモードを開始する（tmux 互換）
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

        // コピーモード中のキー処理
        if self.app.state.copy_mode.is_active {
            return self.handle_copy_mode_key(code);
        }

        // Quick Select モード中のキー処理
        if self.app.state.quick_select.is_active {
            return self.handle_quick_select_key(code);
        }

        // ファイル転送ダイアログが開いているときのキー処理（全キーを消費）
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

        // タブ名変更モード中のキー処理（全キーを消費）
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
                    // 英字・数字・記号を入力する
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

        // マクロピッカーが開いているときのナビゲーション（全キーを消費）
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

        // PageUp / PageDown: スクロールバックをスクロールする
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

        // Ctrl+Shift+ArrowUp / ArrowDown: 前後のシェルプロンプトへジャンプ（Sprint 5-2 / B1）
        // OSC 133 A (PromptStart) で記録した anchor を辿る
        if ctrl && shift && code == WKeyCode::ArrowUp {
            self.app.state.jump_prev_prompt();
            return true;
        }
        if ctrl && shift && code == WKeyCode::ArrowDown {
            self.app.state.jump_next_prompt();
            return true;
        }

        // Escape: 検索・パレット・ホストマネージャを閉じる
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
            // パレット・検索が開いていなければ PTY に転送する
            return false;
        }

        // 設定パネルが開いているときのナビゲーション（全キーを消費）
        if self.app.state.settings_panel.is_open {
            let editing = self.app.state.settings_panel.font_family_editing;
            match code {
                WKeyCode::Escape => {
                    if editing {
                        // 編集モードを終了する（変更を破棄せず入力モードだけ終了）
                        self.app.state.settings_panel.font_family_editing = false;
                    } else {
                        self.app.state.settings_panel.close();
                    }
                }
                WKeyCode::Enter => {
                    if editing {
                        // 編集モードを確定する
                        self.app.state.settings_panel.font_family_editing = false;
                    } else {
                        let _ = self.app.state.settings_panel.save_to_toml();
                        self.app.state.settings_panel.close();
                    }
                }
                WKeyCode::Backspace if editing => {
                    self.app.state.settings_panel.pop_font_family_char();
                }
                // F キーで Font カテゴリのフォントファミリー編集モードをトグルする
                WKeyCode::KeyF if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Font {
                        self.app.state.settings_panel.font_family_editing = true;
                    }
                }
                WKeyCode::Tab | WKeyCode::ArrowDown if !editing => {
                    self.app.state.settings_panel.next_category();
                }
                WKeyCode::ArrowUp if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    match &self.app.state.settings_panel.category {
                        SettingsCategory::Font => {
                            self.app.state.settings_panel.increase_font_size()
                        }
                        SettingsCategory::Window => {
                            self.app.state.settings_panel.increase_opacity()
                        }
                        _ => self.app.state.settings_panel.prev_category(),
                    }
                }
                WKeyCode::ArrowRight if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    match &self.app.state.settings_panel.category {
                        SettingsCategory::Theme => self.app.state.settings_panel.next_scheme(),
                        SettingsCategory::Startup => self.app.state.settings_panel.next_language(),
                        _ => {}
                    }
                }
                WKeyCode::ArrowLeft if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    match &self.app.state.settings_panel.category {
                        SettingsCategory::Theme => self.app.state.settings_panel.prev_scheme(),
                        SettingsCategory::Startup => self.app.state.settings_panel.prev_language(),
                        _ => {}
                    }
                }
                // Space: Startup カテゴリの auto_check_update トグル
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

        // パレットが開いているときのナビゲーション（全キーを消費）
        if self.app.state.palette.is_open {
            match code {
                WKeyCode::ArrowDown => self.app.state.palette.select_next(),
                WKeyCode::ArrowUp => self.app.state.palette.select_prev(),
                WKeyCode::Enter => {
                    if let Some(action) = self.app.state.palette.selected_action() {
                        let action_id = action.action.clone();
                        self.app.state.palette.close();
                        // Sprint 5-7 / Phase 3-3: 使用履歴を記録して永続化
                        self.app.state.palette.record_use(&action_id);
                        self.execute_action(&action_id, event_loop);
                    }
                }
                _ => {}
            }
            return true;
        }

        // 更新通知バナーが表示中のとき: Esc で閉じる、Enter でブラウザを開く
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

        // ホストマネージャが開いているときのナビゲーション（全キーを消費）
        // パスワードモーダルが開いている場合は専用処理
        if self.app.state.host_manager.password_modal.is_some() {
            match code {
                WKeyCode::Escape => {
                    self.app.state.host_manager.password_modal = None;
                }
                WKeyCode::Tab => {
                    // OS キーチェーン保存フラグの切り替え（Sprint 3-2 後半）
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
                        // Sprint 5-1 / G1: take_password() の前に remember を読み出す
                        // （IPC で ephemeral_password = !remember として送信するため）
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
                            // パスワード認証ホストはモーダルを開いてから接続する
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

        // 検索モードの特殊キー
        if self.app.state.search.is_active {
            match code {
                // Enter: 次のマッチへ / Shift+Enter: 前のマッチへ
                WKeyCode::Enter => {
                    if shift {
                        self.app.state.search_prev();
                    } else {
                        self.app.state.search_next();
                    }
                    return true;
                }
                // N: 前のマッチへ（vim 慣習）
                WKeyCode::KeyN if shift => {
                    self.app.state.search_prev();
                    return true;
                }
                _ => {}
            }
        }

        // Ctrl++（Equal / Plus）: フォントサイズを大きくする
        if ctrl && (code == WKeyCode::Equal || code == WKeyCode::NumpadAdd) {
            self.change_font_size(1.0);
            return true;
        }

        // Ctrl+- : フォントサイズを小さくする
        if ctrl && (code == WKeyCode::Minus || code == WKeyCode::NumpadSubtract) {
            self.change_font_size(-1.0);
            return true;
        }

        // Ctrl+0 : フォントサイズをデフォルトに戻す
        if ctrl && code == WKeyCode::Digit0 {
            self.reset_font_size();
            return true;
        }

        // Sprint 5-7 / UI-1-4 + bug fix: Leader 単独押下を検知して prefix モードに突入する
        // leader_key（例: "ctrl+b"）が現在の修飾子+キーと一致し、かつ `<leader> X` 形式の
        // バインドが 1 つ以上設定されている場合のみ prefix モード突入＋ PTY に送らない。
        // prefix バインドが設定されていない場合は素通りで通常の Ctrl+B 等として PTY に流す
        // （ユーザーの既存ワークフロー破壊を避けるため）。
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
            return true; // PTY へは送らない（prefix モード突入を消費）
        }

        // 設定ファイルのカスタムキーバインドをチェックする
        if self.check_config_keybindings(code, event_loop) {
            return true;
        }

        false
    }

    /// 設定に `<leader> X` 形式のプレフィックスバインドが 1 つでも存在するかを返す。
    /// Leader 単独押下時に prefix モード突入するかどうかの判定に使う。
    fn has_prefix_bindings(&self) -> bool {
        let leader = &self.app.config.leader_key;
        if leader.is_empty() {
            return false;
        }
        self.app.config.keys.iter().any(|b| {
            let expanded = self.app.config.expand_leader(&b.key);
            let mut tokens = expanded.split_whitespace();
            let first = tokens.next();
            // 2 トークン以上 + 先頭が leader と一致する
            first.is_some_and(|t| t.eq_ignore_ascii_case(leader)) && tokens.next().is_some()
        })
    }

    /// クリック座標 (col, row) に URL があれば返す
    pub(super) fn find_url_at(&self, col: u16, row: u16) -> Option<String> {
        use crate::state::detect_urls_in_row;
        let pane = self.app.state.focused_pane()?;

        // OSC 8 ハイパーリンクを優先チェックする
        for span in &pane.grid.hyperlinks {
            if span.row == row && col >= span.col_start && col < span.col_end {
                return Some(span.url.clone());
            }
        }

        // テキストパターンから URL を動的検出する
        let cells = pane.grid.rows.get(row as usize)?;
        let urls = detect_urls_in_row(row, cells);
        urls.into_iter()
            .find(|u| u.contains(col, row))
            .map(|u| u.url)
    }

    /// 設定のキーバインド一覧から一致するものを探してアクションを実行する。
    /// 消費した場合は true を返す。
    ///
    /// Sprint 5-7 / UI-1-3 + bug fix: 2 経路でディスパッチする:
    /// - **prefix モード中** (`prefix_pending_until` が有効): `<leader> X` 形式のバインドのみ
    ///   照合し、第 1 トークンが leader と一致するエントリの残り token を本キー入力と比較する。
    ///   マッチで実行＆ prefix モード解除。未マッチでも prefix モード解除して単発バインド照合に
    ///   フォールスルーする（後続のキーが通常入力として動作するようにするため、本キーは消費しない）。
    /// - **prefix モード外**: スペース区切りのバインドはスキップし、単発バインドのみ照合する。
    fn check_config_keybindings(&mut self, code: WKeyCode, event_loop: &ActiveEventLoop) -> bool {
        let bindings = self.app.config.keys.clone();
        let leader = self.app.config.leader_key.clone();
        let now = std::time::Instant::now();

        let in_prefix = matches!(
            self.app.state.prefix_pending_until,
            Some(t) if now < t
        );

        if in_prefix {
            // prefix モード中: <leader> X 形式のバインドのみ照合
            for binding in &bindings {
                let expanded = self.app.config.expand_leader(&binding.key);
                let tokens: Vec<&str> = expanded.split_whitespace().collect();
                if tokens.len() < 2 {
                    continue;
                }
                // 第 1 トークンは leader と一致する必要あり
                if !tokens[0].eq_ignore_ascii_case(leader.as_str()) {
                    continue;
                }
                // 残り token を結合（将来の多段 prefix に備える。現状は単一 token 想定）
                let rest = tokens[1..].join(" ");
                if config_key_matches_token(&rest, code, self.modifiers) {
                    let action = binding.action.clone();
                    self.app.state.prefix_pending_until = None;
                    self.app.state.key_hint_visible_until = None;
                    self.execute_action(&action, event_loop);
                    return true;
                }
            }
            // prefix モード中にマッチしなかった: モード解除して通常バインド照合へフォールスルー
            // （本キーは消費せず、未マッチなら最終的に PTY 入力として処理される）
            self.app.state.prefix_pending_until = None;
            self.app.state.key_hint_visible_until = None;
        }

        // 単発バインド照合（prefix モード外、または prefix モード未マッチ時のフォールスルー）
        for binding in &bindings {
            let expanded = self.app.config.expand_leader(&binding.key);
            // スペース区切り（prefix 系）は本経路ではスキップ
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

    /// キー入力をサーバーの PTY に転送する
    pub(super) fn forward_key_to_server(&self, physical_key: PhysicalKey, text: Option<&str>) {
        let Some(conn) = &self.connection else { return };
        let mods = proto_modifiers(self.modifiers);
        let ctrl = self.modifiers.control_key();

        // Ctrl 非押下でテキストがある場合はテキスト入力として送信する
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

        // 特殊キーおよび Ctrl キーシーケンス
        if let Some(key_code) = physical_to_proto_key(physical_key, self.modifiers) {
            let _ = conn.send_tx.try_send(ClientToServer::KeyEvent {
                code: key_code,
                modifiers: mods,
            });
        }
    }
}
