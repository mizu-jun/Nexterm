//! Sprint 2-1 Phase C: 入力ハンドラ
//!
//! `renderer/mod.rs` から抽出した `EventHandler` の入力処理メソッド群。
//! handle_key / コピーモード / クイックセレクト / SSH 接続 / フォントサイズ操作。

use nexterm_proto::ClientToServer;
use nexterm_proto::KeyCode as ProtoKeyCode;
use tracing::{debug, info};
use winit::{
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode as WKeyCode, PhysicalKey},
};

use crate::glyph_atlas::GlyphAtlas;
use crate::key_map::{
    config_key_matches, physical_to_proto_key, proto_modifiers, winit_code_to_char,
};
use crate::state::ContextMenuAction;
use crate::vertex_util::grid_to_text;

use super::EventHandler;

impl EventHandler {
    /// キーを処理してローカルで消費した場合は true を返す
    pub(super) fn handle_key(&mut self, code: WKeyCode, event_loop: &ActiveEventLoop) -> bool {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

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
                        let password = m.take_password();
                        self.app.state.host_manager.password_modal = None;
                        self.app.state.host_manager.record_connection(&host);
                        self.connect_ssh_host_with_password(&host, password);
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

        // 設定ファイルのカスタムキーバインドをチェックする
        if self.check_config_keybindings(code, event_loop) {
            return true;
        }

        false
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

    /// コピーモードのキー入力を処理する（true = 消費済み）
    fn handle_copy_mode_key(&mut self, code: WKeyCode) -> bool {
        // 検索入力中は専用ハンドラに委譲する
        if self.app.state.copy_mode.search_query.is_some() {
            return self.handle_copy_mode_search_key(code);
        }

        let cm = &mut self.app.state.copy_mode;
        let max_col = self.app.state.cols.saturating_sub(1);
        let max_row = self.app.state.rows.saturating_sub(1);

        match code {
            // q / Escape: コピーモードを終了する
            WKeyCode::KeyQ | WKeyCode::Escape => {
                cm.exit();
            }
            // h / Left: 左移動
            WKeyCode::KeyH | WKeyCode::ArrowLeft => {
                cm.cursor_col = cm.cursor_col.saturating_sub(1);
            }
            // l / Right: 右移動
            WKeyCode::KeyL | WKeyCode::ArrowRight => {
                if cm.cursor_col < max_col {
                    cm.cursor_col += 1;
                }
            }
            // j / Down: 下移動
            WKeyCode::KeyJ | WKeyCode::ArrowDown => {
                if cm.cursor_row < max_row {
                    cm.cursor_row += 1;
                }
            }
            // k / Up: 上移動
            WKeyCode::KeyK | WKeyCode::ArrowUp => {
                cm.cursor_row = cm.cursor_row.saturating_sub(1);
            }
            // 0: 行頭へ移動
            WKeyCode::Digit0 => {
                cm.cursor_col = 0;
            }
            // $: 行末へ移動
            WKeyCode::Digit4 => {
                // Shift+4 = '$' として扱う（WKeyCode には Dollar がないため）
                cm.cursor_col = max_col;
            }
            // w: 次の単語の先頭へ移動
            WKeyCode::KeyW => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_next_word_start(col, row, max_col, max_row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // b: 前の単語の先頭へ移動
            WKeyCode::KeyB => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_prev_word_start(col, row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // v: 選択開始/終了をトグル
            WKeyCode::KeyV => {
                cm.toggle_selection();
            }
            // y / Y: y=選択テキストをヤンク、Y=行全体をヤンク
            WKeyCode::KeyY => {
                if self.modifiers.shift_key() {
                    self.yank_current_line();
                } else {
                    self.yank_selection();
                }
            }
            // /: インクリメンタル検索モードへ
            WKeyCode::Slash => {
                self.app.state.copy_mode.search_query = Some(String::new());
            }
            // n: 次の検索結果へ
            WKeyCode::KeyN => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_query
                    .clone()
                    .unwrap_or_default();
                if !q.is_empty() {
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    if let Some((nc, nr)) = self.search_forward(&q, col + 1, row, max_col, max_row)
                    {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// 検索入力中のキー処理（true = 消費済み）
    fn handle_copy_mode_search_key(&mut self, code: WKeyCode) -> bool {
        match code {
            // Escape: 検索をキャンセルして通常コピーモードへ
            WKeyCode::Escape => {
                self.app.state.copy_mode.search_query = None;
            }
            // Enter: 検索確定して最初のマッチへジャンプ
            WKeyCode::Enter => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_query
                    .clone()
                    .unwrap_or_default();
                self.app.state.copy_mode.search_query = None;
                if !q.is_empty() {
                    let max_col = self.app.state.cols.saturating_sub(1);
                    let max_row = self.app.state.rows.saturating_sub(1);
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    if let Some((nc, nr)) = self.search_forward(&q, col, row, max_col, max_row) {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                        // 最後の検索クエリを保存して n キーで再利用できるようにする
                        self.app.state.copy_mode.search_query = Some(q);
                    }
                }
            }
            // Backspace: クエリの末尾を削除
            WKeyCode::Backspace => {
                if let Some(ref mut q) = self.app.state.copy_mode.search_query {
                    q.pop();
                }
            }
            _ => return false,
        }
        true
    }

    /// 次の単語の先頭位置を返す（見つからなければ None）
    fn find_next_word_start(
        &self,
        col: u16,
        row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as usize;
        let mut r = row as usize;

        // 現在位置が単語文字なら単語の終わりまでスキップ
        if let Some(cells) = pane.grid.rows.get(r) {
            while c < cells.len() && !cells[c].ch.is_whitespace() {
                c += 1;
            }
        }
        // 次の単語の先頭（空白をスキップ）
        loop {
            if let Some(cells) = pane.grid.rows.get(r) {
                while c < cells.len() {
                    if !cells[c].ch.is_whitespace() {
                        return Some((c as u16, r as u16));
                    }
                    c += 1;
                }
            }
            // 次の行へ
            if r >= max_row as usize {
                break;
            }
            r += 1;
            c = 0;
        }
        Some((max_col, max_row))
    }

    /// 前の単語の先頭位置を返す（見つからなければ None）
    fn find_prev_word_start(&self, col: u16, row: u16) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as isize - 1;
        let mut r = row as isize;

        // 現在位置の直前が空白ならスキップ
        loop {
            if c < 0 {
                if r <= 0 {
                    return Some((0, 0));
                }
                r -= 1;
                c = pane
                    .grid
                    .rows
                    .get(r as usize)
                    .map(|row| row.len() as isize - 1)
                    .unwrap_or(0);
            }
            if let Some(cells) = pane.grid.rows.get(r as usize)
                && c < cells.len() as isize
                && !cells[c as usize].ch.is_whitespace()
            {
                break;
            }
            c -= 1;
        }
        // 単語の先頭までスキップ
        loop {
            if c <= 0 {
                return Some((0, r as u16));
            }
            if let Some(cells) = pane.grid.rows.get(r as usize) {
                if c - 1 < cells.len() as isize && cells[(c - 1) as usize].ch.is_whitespace() {
                    break;
                }
            } else {
                break;
            }
            c -= 1;
        }
        Some((c as u16, r as u16))
    }

    /// 前方検索: クエリに最初にマッチする (col, row) を返す
    fn search_forward(
        &self,
        query: &str,
        start_col: u16,
        start_row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let rows_total = (max_row + 1) as usize;

        for dr in 0..rows_total {
            let r = ((start_row as usize) + dr) % rows_total;
            let cells = pane.grid.rows.get(r)?;
            let row_str: String = cells.iter().map(|c| c.ch).collect();
            let col_start = if dr == 0 { start_col as usize } else { 0 };
            let search_in = if col_start < row_str.len() {
                &row_str[col_start..]
            } else {
                continue;
            };
            if let Some(offset) = search_in.find(query) {
                let found_col = (col_start + offset).min(max_col as usize) as u16;
                return Some((found_col, r as u16));
            }
        }
        None
    }

    /// 選択範囲のテキストをクリップボードにコピーしてコピーモードを終了する
    fn yank_selection(&mut self) {
        let cm = &self.app.state.copy_mode;
        if let Some(((sc, sr), (ec, er))) = cm.normalized_selection() {
            // グリッドから選択テキストを抽出する
            let text = if let Some(pane) = self.app.state.focused_pane() {
                let mut lines = Vec::new();
                for row_idx in sr..=er {
                    if let Some(row) = pane.grid.rows.get(row_idx as usize) {
                        let col_start = if row_idx == sr { sc as usize } else { 0 };
                        let col_end = if row_idx == er {
                            (ec + 1) as usize
                        } else {
                            row.len()
                        };
                        let line: String = row[col_start.min(row.len())..col_end.min(row.len())]
                            .iter()
                            .map(|c| c.ch)
                            .collect();
                        lines.push(line);
                    }
                }
                lines.join("\n")
            } else {
                String::new()
            };

            if !text.is_empty()
                && let Ok(mut clipboard) = arboard::Clipboard::new()
            {
                let _ = clipboard.set_text(text);
            }
        }
        self.app.state.copy_mode.exit();
    }

    /// カーソル行全体をクリップボードにコピーしてコピーモードを終了する（Y キー）
    fn yank_current_line(&mut self) {
        let row_idx = self.app.state.copy_mode.cursor_row as usize;
        let text = if let Some(pane) = self.app.state.focused_pane() {
            pane.grid
                .rows
                .get(row_idx)
                .map(|row| row.iter().map(|c| c.ch).collect::<String>())
                .unwrap_or_default()
        } else {
            String::new()
        };
        if !text.is_empty()
            && let Ok(mut clipboard) = arboard::Clipboard::new()
        {
            let _ = clipboard.set_text(text);
        }
        self.app.state.copy_mode.exit();
    }

    /// フォントサイズを delta pt だけ変更してグリフアトラスを再生成する
    fn change_font_size(&mut self, delta: f32) {
        let new_size = (self.app.config.font.size + delta).clamp(6.0, 72.0);
        if (new_size - self.app.config.font.size).abs() < f32::EPSILON {
            return;
        }
        self.app.config.font.size = new_size;
        self.app.font = crate::font::FontManager::new(
            &self.app.config.font.family,
            new_size,
            &self.app.config.font.font_fallbacks,
            self.scale_factor,
            self.app.config.font.ligatures,
        );
        let atlas_size = self.app.config.gpu.atlas_size;
        if let Some(wgpu) = &self.wgpu_state {
            self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
        }
        info!("Font size changed to {}pt", new_size);
    }

    /// Quick Select モードのキー入力を処理する（true = 消費済み）
    fn handle_quick_select_key(&mut self, code: WKeyCode) -> bool {
        match code {
            WKeyCode::Escape => {
                self.app.state.quick_select.exit();
                return true;
            }
            WKeyCode::Backspace => {
                self.app.state.quick_select.typed_label.pop();
                return true;
            }
            _ => {}
        }

        // アルファベットキーをラベル入力として受け取る
        if let Some(ch) = winit_code_to_char(code) {
            self.app.state.quick_select.typed_label.push(ch);

            // マッチが確定したらクリップボードにコピーして終了
            if let Some(m) = self.app.state.quick_select.accept() {
                let text = m.text.clone();
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
                self.app.state.quick_select.exit();
            }
        }

        true
    }

    /// フォントサイズを設定ファイルの初期値に戻す
    fn reset_font_size(&mut self) {
        // 設定ファイルの初期値は config 生成時のサイズを参照する手段がないため
        // 慣例の 14pt をデフォルトとして使用する
        let default_size = nexterm_config::Config::default().font.size;
        self.app.config.font.size = default_size;
        self.app.font = crate::font::FontManager::new(
            &self.app.config.font.family,
            default_size,
            &self.app.config.font.font_fallbacks,
            self.scale_factor,
            self.app.config.font.ligatures,
        );
        let atlas_size = self.app.config.gpu.atlas_size;
        if let Some(wgpu) = &self.wgpu_state {
            self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
        }
        info!("Font size reset to {}pt", default_size);
    }

    fn execute_action(&mut self, action: &str, event_loop: &ActiveEventLoop) {
        match action {
            "Quit" => event_loop.exit(),
            "SearchScrollback" => self.app.state.start_search(),
            "SplitVertical" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitVertical);
                }
            }
            "SplitHorizontal" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitHorizontal);
                }
            }
            "FocusNextPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::FocusNextPane);
                }
            }
            "FocusPrevPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::FocusPrevPane);
                }
            }
            "ClosePane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ClosePane);
                }
            }
            "NewWindow" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::NewWindow);
                }
            }
            "Detach" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::Detach);
                }
            }
            "CommandPalette" => {
                self.app.state.toggle_palette();
            }
            "SetBroadcastOn" => {
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::SetBroadcast { enabled: true });
                }
            }
            "SetBroadcastOff" => {
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::SetBroadcast { enabled: false });
                }
            }
            "ToggleZoom" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ToggleZoom);
                }
            }
            "QuickSelect" => {
                if let Some(pane) = self.app.state.focused_pane() {
                    let rows = pane.grid.rows.clone();
                    self.app.state.quick_select.enter(&rows);
                }
            }
            "SwapPaneNext" => {
                // フォーカスペインの次のペイン ID を取得してスワップする
                if let Some(conn) = &self.connection {
                    // 現在フォーカスペインの隣ペインを pane_layouts から探す
                    let layouts: Vec<_> = self.app.state.pane_layouts.values().collect();
                    if layouts.len() >= 2 {
                        let focused = self.app.state.focused_pane_id.unwrap_or(0);
                        // focused 以外で pane_id が最も近い（次の）ペインを選ぶ
                        let target = layouts
                            .iter()
                            .filter(|l| l.pane_id != focused)
                            .map(|l| l.pane_id)
                            .min_by_key(|&id| if id > focused { id - focused } else { u32::MAX })
                            .or_else(|| {
                                layouts.iter().map(|l| l.pane_id).find(|&id| id != focused)
                            });
                        if let Some(target_id) = target {
                            let _ = conn.send_tx.try_send(ClientToServer::SwapPane {
                                target_pane_id: target_id,
                            });
                        }
                    }
                }
            }
            "SwapPanePrev" => {
                if let Some(conn) = &self.connection {
                    let layouts: Vec<_> = self.app.state.pane_layouts.values().collect();
                    if layouts.len() >= 2 {
                        let focused = self.app.state.focused_pane_id.unwrap_or(0);
                        let target = layouts
                            .iter()
                            .filter(|l| l.pane_id != focused)
                            .map(|l| l.pane_id)
                            .min_by_key(|&id| if id < focused { focused - id } else { u32::MAX })
                            .or_else(|| {
                                layouts.iter().map(|l| l.pane_id).find(|&id| id != focused)
                            });
                        if let Some(target_id) = target {
                            let _ = conn.send_tx.try_send(ClientToServer::SwapPane {
                                target_pane_id: target_id,
                            });
                        }
                    }
                }
            }
            "BreakPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::BreakPane);
                }
            }
            "ShowSettings" => {
                self.app.state.settings_panel.open();
            }
            "ShowHostManager" => {
                self.app
                    .state
                    .host_manager
                    .reload(self.app.config.hosts.clone());
                self.app.state.host_manager.open();
            }
            "ShowMacroPicker" => {
                self.app
                    .state
                    .macro_picker
                    .reload(self.app.config.macros.clone());
                self.app.state.macro_picker.open();
            }
            "SftpUploadDialog" => {
                self.app.state.file_transfer.open_upload();
            }
            "SftpDownloadDialog" => {
                self.app.state.file_transfer.open_download();
            }
            "ConnectSerialPrompt" => {
                // 設定ファイルのシリアルポート一覧からデフォルト（先頭）エントリで接続する
                // 設定がない場合は一般的なデフォルト値を使用する
                if let Some(conn) = &self.connection {
                    let serial_cfg = self.app.config.serial_ports.first().cloned();
                    let (port, baud_rate, data_bits, stop_bits, parity) =
                        if let Some(cfg) = serial_cfg {
                            (
                                cfg.port,
                                cfg.baud_rate,
                                cfg.data_bits,
                                cfg.stop_bits,
                                cfg.parity,
                            )
                        } else {
                            // プラットフォームデフォルト
                            #[cfg(unix)]
                            let default_port = "/dev/ttyUSB0".to_string();
                            #[cfg(windows)]
                            let default_port = "COM1".to_string();
                            (default_port, 115200, 8, 1, "none".to_string())
                        };
                    let _ = conn.send_tx.try_send(ClientToServer::ConnectSerial {
                        port,
                        baud_rate,
                        data_bits,
                        stop_bits,
                        parity,
                    });
                }
            }
            _ => debug!("Execute action: {}", action),
        }
    }

    /// コンテキストメニューのアクションを実行する
    pub(super) fn execute_context_menu_action(&mut self, action: &ContextMenuAction) {
        match action {
            ContextMenuAction::Copy => {
                // フォーカスペインの可視グリッドをクリップボードにコピーする
                if let Some(pane) = self.app.state.focused_pane() {
                    let text = grid_to_text(pane);
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                }
            }
            ContextMenuAction::Paste => {
                if let Ok(mut clipboard) = arboard::Clipboard::new()
                    && let Ok(text) = clipboard.get_text()
                    && let Some(conn) = &self.connection
                {
                    let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                }
            }
            ContextMenuAction::SelectAll => {
                // グリッド全体のテキストをクリップボードにコピーする
                if let Some(pane) = self.app.state.focused_pane() {
                    let text = grid_to_text(pane);
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                }
            }
            ContextMenuAction::SplitVertical => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitVertical);
                }
            }
            ContextMenuAction::SplitHorizontal => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitHorizontal);
                }
            }
            ContextMenuAction::ClosePane => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ClosePane);
                }
            }
            ContextMenuAction::InlineSearch => {
                self.app.state.start_search();
            }
            ContextMenuAction::OpenSettings => {
                self.app.state.settings_panel.open();
            }
            ContextMenuAction::OpenProfile { profile_name } => {
                // プロファイルのシェル設定でペインを新規分割する
                if let Some(prof) = self
                    .app
                    .config
                    .profiles
                    .iter()
                    .find(|p| &p.name == profile_name)
                    && let Some(shell) = &prof.shell
                    && let Some(conn) = &self.connection
                {
                    // まず垂直分割してから ConnectSsh の代わりにシェルパスを環境変数で渡す
                    // （現時点では SplitVertical で新ペインを開き、プロファイル設定はログとして記録）
                    let _ = conn.send_tx.try_send(ClientToServer::SplitVertical);
                    info!(
                        "プロファイル '{}' のシェル '{}' で起動を要求",
                        profile_name, shell.program
                    );
                }
            }
            ContextMenuAction::Separator => {
                // セパレーターはクリック不可のため何もしない
            }
        }
    }

    /// HostConfig から ConnectSsh メッセージを送信する（現在のペインに接続）
    #[allow(dead_code)]
    fn connect_ssh_host(&self, host: &nexterm_config::HostConfig) {
        let Some(conn) = &self.connection else { return };
        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password: None,
            key_path: host.key_path.clone(),
            remote_forwards: host.forward_remote.clone(),
            x11_forward: host.x11_forward,
            x11_trusted: host.x11_trusted,
        });
    }

    /// HostConfig から新しいタブを開いて ConnectSsh メッセージを送信する
    fn connect_ssh_host_new_tab(&self, host: &nexterm_config::HostConfig) {
        let Some(conn) = &self.connection else { return };
        // 先に新しいウィンドウ（タブ）を作成してから SSH 接続を要求する
        let _ = conn.send_tx.try_send(ClientToServer::NewWindow);
        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password: None,
            key_path: host.key_path.clone(),
            remote_forwards: host.forward_remote.clone(),
            x11_forward: host.x11_forward,
            x11_trusted: host.x11_trusted,
        });
    }

    /// パスワード付きで新しいタブを開いて ConnectSsh メッセージを送信する（パスワード認証ホスト用）
    ///
    /// HIGH H-6: パスワード文字列は `Zeroizing<String>` で受け取り、
    /// IPC 送信後に drop されてメモリゼロクリアされる。
    fn connect_ssh_host_with_password(
        &self,
        host: &nexterm_config::HostConfig,
        password: zeroize::Zeroizing<String>,
    ) {
        let Some(conn) = &self.connection else { return };
        let _ = conn.send_tx.try_send(ClientToServer::NewWindow);
        // IPC bincode シリアライズ時に String が必要なので一時 clone する。
        // 旧 password は関数終了時に drop されゼロクリアされる。
        // TODO(future): IPC 経由で平文パスワードを送る代わりに OS keyring に
        // 一時保存する設計に変更する（HIGH H-6 の根本対策）。
        let pwd_string: Option<String> = if password.is_empty() {
            None
        } else {
            Some((*password).clone())
        };
        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password: pwd_string,
            key_path: host.key_path.clone(),
            remote_forwards: host.forward_remote.clone(),
            x11_forward: host.x11_forward,
            x11_trusted: host.x11_trusted,
        });
    }

    /// 設定のキーバインド一覧から一致するものを探してアクションを実行する
    /// 消費した場合は true を返す
    fn check_config_keybindings(&mut self, code: WKeyCode, event_loop: &ActiveEventLoop) -> bool {
        // config.keys を走査してマッチするバインドを探す
        let bindings = self.app.config.keys.clone();
        for binding in &bindings {
            if config_key_matches(&binding.key, code, self.modifiers) {
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
