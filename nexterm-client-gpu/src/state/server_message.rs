//! サーバーメッセージ適用とスクロールバック / プロンプトジャンプ操作
//!
//! `state/mod.rs` から抽出した:
//! - `impl ClientState { apply_server_message }` — サーバーから受信した全
//!   `ServerToClient` メッセージのディスパッチと状態反映
//! - `impl ClientState { scroll_up / scroll_down / jump_prev_prompt / jump_next_prompt }` —
//!   スクロールバックのオフセット操作および OSC 133 PromptStart anchor ベースのプロンプトジャンプ
//! - 単体テスト群（FullRefresh / GridDiff / 検索のライフサイクル / Quick Select 拡充）

use nexterm_proto::ServerToClient;

use super::ClientState;
use super::pane::{FloatRect, PaneState, PlacedImage};

impl ClientState {
    pub fn apply_server_message(&mut self, msg: ServerToClient) {
        match msg {
            ServerToClient::FullRefresh { pane_id, grid } => {
                let cursor_col = grid.cursor_col;
                let cursor_row = grid.cursor_row;
                let capacity = self.scrollback_capacity;
                let state = self
                    .panes
                    .entry(pane_id)
                    .or_insert_with(|| PaneState::new(grid.width, grid.height, capacity));
                state.grid = grid;
                state.cursor_col = cursor_col;
                state.cursor_row = cursor_row;
                if self.focused_pane_id.is_none() {
                    self.focused_pane_id = Some(pane_id);
                }
            }
            ServerToClient::GridDiff {
                pane_id,
                dirty_rows,
                cursor_col,
                cursor_row,
            } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.apply_diff(dirty_rows, cursor_col, cursor_row);
                    // 非フォーカスペインへの出力はアクティビティとしてマーク
                    if self.focused_pane_id != Some(pane_id) {
                        pane.has_activity = true;
                    }
                }
            }
            ServerToClient::Pong => {}
            ServerToClient::HelloAck {
                proto_version,
                server_version,
            } => {
                tracing::info!(
                    "サーバー HelloAck 受信: proto={}, server_version={}",
                    proto_version,
                    server_version
                );
            }
            ServerToClient::Error { message } => {
                tracing::error!("サーバーエラー: {}", message);
            }
            ServerToClient::SessionList { .. } => {}
            ServerToClient::ImagePlaced {
                pane_id,
                image_id,
                col,
                row,
                width,
                height,
                rgba,
            } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.images.insert(
                        image_id,
                        PlacedImage {
                            col,
                            row,
                            width,
                            height,
                            rgba,
                        },
                    );
                    if self.focused_pane_id != Some(pane_id) {
                        pane.has_activity = true;
                    }
                }
            }
            ServerToClient::Bell { .. } => {
                // OS のウィンドウ注目要求をトリガーするためフラグを立てる
                self.pending_bell = true;
            }
            ServerToClient::RecordingStarted { .. } | ServerToClient::RecordingStopped { .. } => {}
            ServerToClient::WindowListChanged { .. } | ServerToClient::PaneClosed { .. } => {}
            // OSC 0/2 タイトル変更 — ペインのタイトルフィールドを更新する
            ServerToClient::TitleChanged { pane_id, title } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.title = title;
                }
            }
            // DesktopNotification と ClipboardWriteRequest は event_handler 側で
            // SecurityConfig ポリシーに従って処理する（state.rs では何もしない）
            ServerToClient::DesktopNotification { .. } => {}
            ServerToClient::ClipboardWriteRequest { .. } => {}
            ServerToClient::BroadcastModeChanged { enabled } => {
                self.broadcast_mode = enabled;
            }
            ServerToClient::AsciicastStarted { .. } | ServerToClient::AsciicastStopped { .. } => {}
            ServerToClient::TemplateSaved { .. }
            | ServerToClient::TemplateLoaded { .. }
            | ServerToClient::TemplateList { .. } => {}
            ServerToClient::ZoomChanged { is_zoomed } => {
                self.is_zoomed = is_zoomed;
            }
            // ペイン分離・シリアル接続はサーバーから LayoutChanged / WindowListChanged が後続するため状態更新不要
            ServerToClient::PaneBroken { .. } | ServerToClient::SerialConnected { .. } => {}
            // SFTP 転送進捗・完了はステータスバーに表示する
            ServerToClient::SftpProgress {
                path,
                transferred,
                total,
            } => {
                let pct = (transferred * 100).checked_div(total).unwrap_or(0);
                self.status_bar_text = format!("SFTP {} {}%", path, pct);
            }
            ServerToClient::SftpDone { path, error } => {
                if let Some(err) = error {
                    self.status_bar_text = format!("SFTP ERR: {}", err);
                } else {
                    self.status_bar_text = format!("SFTP OK: {}", path);
                }
            }
            // OSC 133 セマンティックゾーンマーク — ステータスバーに最新コマンド終了コードを表示
            //   + A (PromptStart) で jump-to-prompt 用の anchor を記録（Sprint 5-2 / B1）
            ServerToClient::SemanticMark {
                pane_id,
                kind,
                exit_code,
                ..
            } => {
                if kind == "A"
                    && let Some(pane) = self.panes.get_mut(&pane_id)
                {
                    // 重複登録防止: 最後の anchor と同一なら追加しない
                    let next_idx = pane.scrollback.len();
                    if pane.prompt_anchors.last().copied() != Some(next_idx) {
                        pane.prompt_anchors.push(next_idx);
                    }
                    // anchor の保持上限（メモリ DoS 防止）。古いものから削除。
                    const MAX_PROMPT_ANCHORS: usize = 1024;
                    if pane.prompt_anchors.len() > MAX_PROMPT_ANCHORS {
                        let excess = pane.prompt_anchors.len() - MAX_PROMPT_ANCHORS;
                        pane.prompt_anchors.drain(..excess);
                    }
                }
                if kind == "D"
                    && self.focused_pane_id == Some(pane_id)
                    && let Some(code) = exit_code
                {
                    if code != 0 {
                        self.status_bar_text = format!("[exit: {}]", code);
                    } else {
                        self.status_bar_text.clear();
                    }
                }
            }
            // フローティングペインイベント — 位置情報をキャッシュするが、
            // レンダラー側での描画は renderer.rs で別途実装する
            ServerToClient::FloatingPaneOpened {
                pane_id,
                col_off,
                row_off,
                cols,
                rows,
            } => {
                self.floating_pane_rects.insert(
                    pane_id,
                    FloatRect {
                        col_off,
                        row_off,
                        cols,
                        rows,
                    },
                );
            }
            ServerToClient::FloatingPaneMoved {
                pane_id,
                col_off,
                row_off,
                cols,
                rows,
            } => {
                self.floating_pane_rects.insert(
                    pane_id,
                    FloatRect {
                        col_off,
                        row_off,
                        cols,
                        rows,
                    },
                );
            }
            ServerToClient::FloatingPaneClosed { pane_id } => {
                self.floating_pane_rects.remove(&pane_id);
            }
            ServerToClient::LayoutChanged {
                panes,
                focused_pane_id,
            } => {
                // レイアウトを全更新する
                self.pane_layouts.clear();
                // Sprint 5-7 / Phase 2-3: panes 配列の登場順を tab_order に反映
                // （サーバーが Window.pane_order に従って並べているため、これが論理タブ順）
                self.tab_order = panes.iter().map(|l| l.pane_id).collect();
                for layout in panes {
                    self.pane_layouts.insert(layout.pane_id, layout);
                }
                // フォーカスペインを更新してアクティビティフラグをクリアする
                self.focused_pane_id = Some(focused_pane_id);
                if let Some(pane) = self.panes.get_mut(&focused_pane_id) {
                    pane.has_activity = false;
                }
            }
            // プラグイン操作応答は GPU クライアントでは無視する
            ServerToClient::PluginList { .. } | ServerToClient::PluginOk { .. } => {}
            // Sprint 5-2 / B2: OSC 7 CWD 通知 — PaneState に保存（UI 表示は今後拡張）
            ServerToClient::CwdChanged { pane_id, cwd } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.cwd = Some(cwd);
                }
            }
            // Sprint 5-7 / Phase 2-1: ワークスペース一覧 / 切替通知
            ServerToClient::WorkspaceList {
                current,
                workspaces: _,
            } => {
                self.current_workspace = current;
            }
            ServerToClient::WorkspaceSwitched { name } => {
                self.current_workspace = name;
            }
            // Sprint 5-7 / Phase 2-2: Quake モード トグル要求
            //
            // nexterm-ctl などから IPC 経由でトグル要求が来た場合、ここでは
            // 「保留中の Quake アクション」だけを記録し、実際のウィンドウ操作は
            // lifecycle 側で winit Window への mutable アクセスを持って実行する。
            ServerToClient::QuakeToggleRequest { action } => {
                self.pending_quake_action = Some(action);
            }
        }
    }

    /// スクロールバックを1画面分上にスクロールする
    pub fn scroll_up(&mut self, lines: usize) {
        if let Some(pane) = self.focused_pane_mut() {
            let max_offset = pane.scrollback.len().saturating_sub(1);
            pane.scroll_offset = (pane.scroll_offset + lines).min(max_offset);
        }
    }

    /// スクロールバックを1画面分下にスクロールする
    pub fn scroll_down(&mut self, lines: usize) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.scroll_offset = pane.scroll_offset.saturating_sub(lines);
        }
    }

    /// 直前のシェルプロンプトへスクロールバックジャンプする（Sprint 5-2 / B1）
    ///
    /// `prompt_anchors` を新しいものから順に走査し、現在の `scroll_offset` より大きい
    /// 最小の anchor へジャンプする（= 画面上で1つ前の prompt）。
    /// anchor がない or 最古に到達済みの場合は no-op。
    /// 戻り値: ジャンプに成功したら `true`。
    pub fn jump_prev_prompt(&mut self) -> bool {
        let Some(pane) = self.focused_pane_mut() else {
            return false;
        };
        let current = pane.scroll_offset;
        let max_offset = pane.scrollback.len().saturating_sub(1);
        // anchor は scrollback の len ベースなので、scroll_offset との比較を直接行う
        let target = pane
            .prompt_anchors
            .iter()
            .rev()
            .copied()
            .find(|&idx| idx > current && idx <= max_offset);
        if let Some(idx) = target {
            pane.scroll_offset = idx;
            true
        } else {
            false
        }
    }

    /// 次のシェルプロンプトへスクロールバックジャンプする（Sprint 5-2 / B1）
    ///
    /// 現在の `scroll_offset` より小さい最大の anchor へジャンプする（= 画面上で1つ後の prompt）。
    /// anchor がない場合は no-op。最新の prompt より新しい位置にいる場合は `scroll_offset = 0` で
    /// ライブ画面へ戻る。戻り値: ジャンプに成功したら `true`。
    pub fn jump_next_prompt(&mut self) -> bool {
        let Some(pane) = self.focused_pane_mut() else {
            return false;
        };
        let current = pane.scroll_offset;
        let target = pane
            .prompt_anchors
            .iter()
            .copied()
            .rev()
            .find(|&idx| idx < current);
        if let Some(idx) = target {
            pane.scroll_offset = idx;
            true
        } else if current > 0 && !pane.prompt_anchors.is_empty() {
            // 全ての anchor より下にいる場合はライブ画面に戻す
            pane.scroll_offset = 0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::menus::find_quick_select_matches;
    use nexterm_proto::{Cell, DirtyRow, Grid};

    #[test]
    fn full_refreshでペインが登録される() {
        let mut state = ClientState::new(80, 24, 1000);
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        assert!(state.panes.contains_key(&1));
        assert_eq!(state.focused_pane_id, Some(1));
    }

    #[test]
    fn grid_diffで差分が適用される() {
        let mut state = ClientState::new(80, 24, 1000);
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        let mut row = vec![Cell::default(); 80];
        row[0].ch = 'X';
        state.apply_server_message(ServerToClient::GridDiff {
            pane_id: 1,
            dirty_rows: vec![DirtyRow { row: 0, cells: row }],
            cursor_col: 1,
            cursor_row: 0,
        });
        let pane = state.focused_pane().unwrap();
        assert_eq!(pane.grid.rows[0][0].ch, 'X');
    }

    #[test]
    fn 検索のライフサイクル() {
        let mut state = ClientState::new(80, 24, 1000);
        state.start_search();
        assert!(state.search.is_active);
        state.push_search_char('a');
        assert_eq!(state.search.query, "a");
        state.end_search();
        assert!(!state.search.is_active);
        assert!(state.search.query.is_empty());
    }

    // ---- Sprint 5-4 / D1: Quick Select 拡充テスト ----

    /// テキストを `Vec<Vec<Cell>>` に変換するヘルパー
    fn text_to_rows(lines: &[&str]) -> Vec<Vec<nexterm_proto::Cell>> {
        lines
            .iter()
            .map(|line| {
                line.chars()
                    .map(|c| nexterm_proto::Cell {
                        ch: c,
                        ..Default::default()
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn quick_select_detects_url() {
        let rows = text_to_rows(&["Visit https://example.com/path?q=1 today"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text.starts_with("https://")));
    }

    #[test]
    fn quick_select_detects_email() {
        let rows = text_to_rows(&["Contact alice@example.com for details"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text == "alice@example.com"));
    }

    #[test]
    fn quick_select_detects_uuid() {
        let rows = text_to_rows(&["session-id: 550e8400-e29b-41d4-a716-446655440000"]);
        let matches = find_quick_select_matches(&rows);
        assert!(
            matches
                .iter()
                .any(|m| m.text == "550e8400-e29b-41d4-a716-446655440000")
        );
    }

    #[test]
    fn quick_select_detects_file_with_line_column() {
        let rows = text_to_rows(&["error in src/main.rs:42:10 — unused variable"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text.contains("src/main.rs:42:10")));
    }

    #[test]
    fn quick_select_detects_jira_ticket() {
        let rows = text_to_rows(&["See PROJ-1234 for tracking"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text == "PROJ-1234"));
    }

    #[test]
    fn quick_select_detects_windows_path() {
        let rows = text_to_rows(&[r"open C:\Users\test\file.txt in editor"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text.starts_with("C:\\")));
    }

    #[test]
    fn quick_select_detects_ipv4_with_port() {
        let rows = text_to_rows(&["connect to 192.168.1.100:8080"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text == "192.168.1.100:8080"));
    }

    #[test]
    fn quick_select_url_priority_over_path() {
        // URL に // 含まれるので path パターンとも重複しうるが、URL を優先すること
        let rows = text_to_rows(&["url: https://github.com/foo/bar"]);
        let matches = find_quick_select_matches(&rows);
        let url_match = matches
            .iter()
            .find(|m| m.text.starts_with("https://"))
            .expect("URL を検出できなかった");
        // path パターンが「/foo/bar」を奪っていないこと（URL に内包されている）
        assert!(url_match.text.contains("github.com/foo/bar"));
        // URL マッチが path マッチと完全重複していないこと
        let path_count = matches.iter().filter(|m| m.text == "/foo/bar").count();
        assert_eq!(path_count, 0, "URL に含まれる path はマッチしないこと");
    }

    #[test]
    fn quick_select_labels_are_assigned() {
        let rows = text_to_rows(&["https://a.com https://b.com https://c.com"]);
        let matches = find_quick_select_matches(&rows);
        assert_eq!(matches.len(), 3);
        let labels: Vec<&str> = matches.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, vec!["a", "b", "c"]);
    }

    #[test]
    fn quick_select_empty_grid_yields_no_matches() {
        let rows: Vec<Vec<nexterm_proto::Cell>> = vec![];
        let matches = find_quick_select_matches(&rows);
        assert!(matches.is_empty());
    }
}
