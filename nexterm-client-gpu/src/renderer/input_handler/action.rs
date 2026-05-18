//! キーバインド・コンテキストメニューのアクション実行
//!
//! `input_handler.rs` から抽出した:
//! - `execute_action` — config.keys からのアクション文字列ディスパッチ
//! - `execute_context_menu_action` — 右クリックメニュー項目の実行

use nexterm_proto::ClientToServer;
use tracing::{debug, info};
use winit::event_loop::ActiveEventLoop;

use super::EventHandler;
use crate::state::ContextMenuAction;
use crate::vertex_util::grid_to_text;

impl EventHandler {
    /// 設定ファイルのキーバインドや CommandPalette から渡されるアクション文字列を実行する
    pub(super) fn execute_action(&mut self, action: &str, event_loop: &ActiveEventLoop) {
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
            // Sprint 5-2 / B1: OSC 133 セマンティックマークによるプロンプトジャンプ
            "JumpPrevPrompt" => {
                self.app.state.jump_prev_prompt();
            }
            "JumpNextPrompt" => {
                self.app.state.jump_next_prompt();
            }
            // Sprint 5-8 / Phase 4-5: tab tearing 関連アクション（Wayland 代替 UX）
            //
            // **`DetachToNewWindow`** — 現在フォーカスペインを新規 OS Window に分離する。
            // タブ外ドロップで自動発火する Phase 4-2 経路と同じ `BreakPane` を起点に、
            // `pending_new_window_drop_pos` を `None` でセットすることで「マウス座標非依存の
            // detach」として扱う。Wayland ではグローバル座標が取れずドラッグ判定ができないため、
            // このアクション経由が代替経路になる。
            //
            // 補足: `BreakPane` 送信後、サーバーから `WindowListChanged` が返ってきた時点で
            // `lifecycle::on_about_to_wait` の new_ids 検出ロジックが `SpawnOsWindow` を送る。
            // `pending_new_window_drop_pos` が `Some(None)` のときも `take()` で `Some(_)` が
            // 返るため判定は維持される（Wayland 等で `pos = None` の場合、winit にウィンドウ
            // 位置決定を委ねる）。
            "DetachToNewWindow" => {
                info!("DetachToNewWindow: BreakPane + 新規 OS Window スポーン要求");
                // pos = None で記録（Wayland でも動作可能なように画面外ヒントなし）
                self.pending_new_window_drop_pos = Some(winit::dpi::PhysicalPosition::new(0, 0));
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::BreakPane);
                }
            }
            // **`CloseOsWindow`** — 現在の OS Window だけを閉じる（プロセス全体は終了しない）。
            // `CloseOsWindow` UserEvent を送信し、`EventHandler::close_os_window` が
            // 実行する。最後の OS Window だった場合のみ `event_loop.exit()` に到達する。
            "CloseOsWindow" => {
                info!("CloseOsWindow: 現在の OS Window を閉じる UserEvent 送出");
                if let Some(w) = &self.window {
                    let wid = w.id();
                    if let Err(e) = self
                        .proxy
                        .send_event(crate::renderer::UserEvent::CloseOsWindow { window_id: wid })
                    {
                        tracing::warn!("CloseOsWindow UserEvent 送信失敗: {}", e);
                    }
                }
            }
            _ => debug!("Execute action: {}", action),
        }
    }

    /// コンテキストメニューのアクションを実行する
    pub(in crate::renderer) fn execute_context_menu_action(&mut self, action: &ContextMenuAction) {
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
            // Sprint 5-8 / Phase 4-5: tab tearing 関連 (Wayland 代替 UX)
            // `execute_action` の同名アクションに委譲して経路を一元化する。
            ContextMenuAction::DetachToNewWindow => {
                info!("ContextMenu: DetachToNewWindow");
                // pos = (0,0) で記録（Wayland でも動作可能、winit が画面位置を決定）
                self.pending_new_window_drop_pos = Some(winit::dpi::PhysicalPosition::new(0, 0));
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::BreakPane);
                }
            }
            ContextMenuAction::CloseOsWindow => {
                info!("ContextMenu: CloseOsWindow");
                if let Some(w) = &self.window {
                    let wid = w.id();
                    if let Err(e) = self
                        .proxy
                        .send_event(crate::renderer::UserEvent::CloseOsWindow { window_id: wid })
                    {
                        tracing::warn!("CloseOsWindow UserEvent 送信失敗: {}", e);
                    }
                }
            }
        }
    }
}
