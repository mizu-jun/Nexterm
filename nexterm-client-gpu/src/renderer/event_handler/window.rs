//! winit `WindowEvent` のうちウィンドウ・IME 関連ハンドラ
//!
//! `event_handler.rs` から抽出した:
//! - `on_close_requested`
//! - `on_resized` / `on_scale_factor_changed`
//! - `on_modifiers_changed`
//! - `on_ime`
//! - `on_redraw_requested`

use nexterm_config::CloseAction;
use nexterm_proto::ClientToServer;
use tracing::{info, warn};
use winit::{event::Ime, event_loop::ActiveEventLoop, keyboard::ModifiersState};

use super::EventHandler;
use crate::glyph_atlas::GlyphAtlas;

impl EventHandler {
    /// `WindowEvent::CloseRequested`
    ///
    /// Sprint 5-8 Phase 4-4 で 3 値分岐を導入、Phase 4-5 で `Prompt` を本実装。
    ///
    /// 挙動:
    /// - **`Prompt`** (デフォルト): `QueryForegroundProcess` IPC を送信して保留。応答（または
    ///   ダイアログでの選択）に応じて detach / kill を実行する。応答待ち中は `event_loop.exit()`
    ///   を呼ばずに `pending_close_request` に状態を保持する。
    /// - **`Detach`**: Server Window を保持してクライアントのみ切断（tmux 流 detached session）。
    ///   - シングルバイナリ構成では `server_handle.abort()` で内部サーバータスクも終了するため、
    ///     実質 Kill と差がない。マルチプロセス（`nexterm-ctl attach`）で本来の意味を持つ枠組み。
    /// - **`Kill`**: Server Session を `KillSession` IPC で破棄してから exit。
    pub(super) fn on_close_requested(&mut self, event_loop: &ActiveEventLoop) {
        let action = self.app.config.window.close_action;
        // 現状セッション名は固定（`Attach` 時に "main" でアタッチしている）。
        // 将来マルチセッション対応時は `EventHandler.current_session` から取得する。
        let session_name = "main".to_string();

        match action {
            CloseAction::Prompt => {
                // Phase 4-5: QueryForegroundProcess を送信して応答を待つ。
                // pending_close_request に記録し、event_loop.exit() を呼ばない（保留）。
                // 応答は `apply_server_message` で `foreground_process_status` に格納され、
                // about_to_wait → `poll_pending_close_request` で消費される。
                let target_window_id = self.app.state.focused_server_window_id;
                info!(
                    "CloseRequested: close_action = Prompt。QueryForegroundProcess 送信 window_id={}",
                    target_window_id
                );
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::QueryForegroundProcess {
                            window_id: target_window_id,
                        });
                }
                self.app.state.pending_close_request = Some(crate::state::PendingCloseRequest {
                    server_window_id: target_window_id,
                    close_action: crate::state::CloseActionKind::Prompt,
                });
                // 早期 return: 応答受信後に finalize_close を呼ぶ
                return;
            }
            CloseAction::Detach => {
                info!(
                    "CloseRequested: close_action = Detach。Server Window を保持してクライアントのみ切断"
                );
                // KillSession を送らずクライアント側のみ切断。
                // シングルバイナリ構成では server_handle.abort() で実質終了。
            }
            CloseAction::Kill => {
                info!("CloseRequested: close_action = Kill。Server Session を破棄して exit");
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::KillSession {
                        name: session_name.clone(),
                    });
                }
            }
        }

        // 追加 OS Window を破棄 + 接続切断 + サーバータスク abort + exit
        self.windows.clear();
        self.connection = None;
        self.server_handle.abort();
        event_loop.exit();
    }

    /// `pending_close_request` の応答 / ダイアログ確定を処理する（Sprint 5-8 Phase 4-5）。
    ///
    /// `about_to_wait` から毎フレーム呼ばれ、`foreground_process_status` の最新応答が
    /// `pending_close_request` と一致した場合に以下を行う:
    /// - 前景プロセスなし → 即時 Kill 経路で exit
    /// - 前景プロセスあり → `close_window_dialog` をセットしてレンダラーが描画
    ///
    /// `close_window_dialog` が `selected_button` 確定状態（外部で `selected_button = u8::MAX` で
    /// キャンセル / `selected_button = 0` で Kill 確定）になっている場合、それも処理する。
    pub(super) fn poll_pending_close_request(&mut self, event_loop: &ActiveEventLoop) {
        // 1. ダイアログが「確定」された場合の処理
        let dialog_decision: Option<bool> = if let Some(dlg) = &self.app.state.close_window_dialog {
            // selected_button = 0xFF をキャンセル、0xFE を Kill 確定のシグナルとして使う
            match dlg.selected_button {
                0xFE => Some(true),  // Kill 確定
                0xFF => Some(false), // キャンセル
                _ => None,
            }
        } else {
            None
        };
        if let Some(kill) = dialog_decision {
            self.app.state.close_window_dialog = None;
            self.app.state.pending_close_request = None;
            if kill {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::KillSession {
                        name: "main".to_string(),
                    });
                }
                self.windows.clear();
                self.connection = None;
                self.server_handle.abort();
                event_loop.exit();
            }
            return;
        }

        // 2. IPC 応答が来ているかチェック
        let Some(req) = self.app.state.pending_close_request else {
            return;
        };
        let Some(status) = self.app.state.foreground_process_status else {
            return;
        };
        // window_id 一致を確認
        if status.window_id != req.server_window_id {
            // 別 Window の応答 → 無視（クリアはしない）
            return;
        }
        // 応答消費
        self.app.state.foreground_process_status = None;

        if status.has_foreground {
            // 確認ダイアログを表示
            info!(
                "前景プロセス検知あり: window_id={}、確認ダイアログを表示",
                req.server_window_id
            );
            // i18n キーから文言を取得（キーが無い場合は `t` が key 自体を返すので、
            // 文言が必ず i18n JSON 側で定義されている前提）
            let message = nexterm_i18n::fl!("close_window_confirm_foreground");
            let kill_label = nexterm_i18n::fl!("close_window_button_kill");
            let cancel_label = nexterm_i18n::fl!("close_window_button_cancel");
            self.app.state.close_window_dialog = Some(crate::state::CloseWindowDialog {
                server_window_id: req.server_window_id,
                message,
                kill_label,
                cancel_label,
                selected_button: 1, // デフォルトはキャンセル側にフォーカス（安全側）
            });
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        } else {
            // 前景プロセスなし → 即時 Kill
            info!("前景プロセスなし: Prompt → Kill に進む");
            self.app.state.pending_close_request = None;
            if let Some(conn) = &self.connection {
                let _ = conn.send_tx.try_send(ClientToServer::KillSession {
                    name: "main".to_string(),
                });
            }
            self.windows.clear();
            self.connection = None;
            self.server_handle.abort();
            event_loop.exit();
        }
    }

    /// `WindowEvent::Resized`
    pub(super) fn on_resized(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        let cell_h_r = self.app.font.cell_height();
        let tab_bar_h_r = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f32
        } else {
            0.0
        };
        let pad_x_r = self.app.config.window.padding_x as f32;
        let pad_y_r = self.app.config.window.padding_y as f32;
        let cols =
            ((size.width as f32 - pad_x_r * 2.0) / self.app.font.cell_width()).max(1.0) as u16;
        let rows = ((size.height as f32 - tab_bar_h_r - cell_h_r - pad_y_r * 2.0) / cell_h_r)
            .max(1.0) as u16;
        if let Some(wgpu) = &mut self.wgpu_state {
            wgpu.resize(size);
        }
        self.app.state.resize(cols, rows);
        // サーバーにリサイズを通知する
        if let Some(conn) = &self.connection {
            let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
        }
    }

    /// `WindowEvent::ScaleFactorChanged`
    pub(super) fn on_scale_factor_changed(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor as f32;
        self.app.font = crate::font::FontManager::new(
            &self.app.config.font.family,
            self.app.config.font.size,
            &self.app.config.font.font_fallbacks,
            self.scale_factor,
            self.app.config.font.ligatures,
        );
        // スケール変更でグリフが無効化されるためアトラスを再生成する
        let atlas_size = self.app.config.gpu.atlas_size;
        if let Some(wgpu) = &self.wgpu_state {
            self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
        }
        // DPI 変更後のセルサイズ変更に合わせて cols/rows を再計算してサーバーに通知する
        if let Some(win) = &self.window {
            let size = win.inner_size();
            let cell_h_sf = self.app.font.cell_height();
            let tab_bar_h_sf = if self.app.config.tab_bar.enabled {
                self.app.config.tab_bar.height as f32
            } else {
                0.0
            };
            let pad_x_sf = self.app.config.window.padding_x as f32;
            let pad_y_sf = self.app.config.window.padding_y as f32;
            let cols =
                ((size.width as f32 - pad_x_sf * 2.0) / self.app.font.cell_width()).max(1.0) as u16;
            let rows = ((size.height as f32 - tab_bar_h_sf - cell_h_sf - pad_y_sf * 2.0)
                / cell_h_sf)
                .max(1.0) as u16;
            self.app.state.resize(cols, rows);
            if let Some(conn) = &self.connection {
                let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
            }
        }
    }

    /// `WindowEvent::ModifiersChanged`
    pub(super) fn on_modifiers_changed(&mut self, mods: ModifiersState) {
        self.modifiers = mods;
    }

    /// `WindowEvent::Ime` — 日本語・中国語などの IME 入力を処理する
    pub(super) fn on_ime(&mut self, ime_event: Ime) {
        match ime_event {
            Ime::Enabled => {
                // IME が有効になった（特別な処理は不要）
            }
            Ime::Preedit(text, _cursor_range) => {
                // 変換中テキストを state に保存して再描画する
                if text.is_empty() {
                    self.app.state.ime_preedit = None;
                } else {
                    self.app.state.ime_preedit = Some(text);
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Ime::Commit(text) => {
                // 確定テキストをプリエディットクリア + PTY 送信
                self.app.state.ime_preedit = None;
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Ime::Disabled => {
                self.app.state.ime_preedit = None;
            }
        }
        // IME カーソルエリアをフォーカスペインのカーソル位置に更新する
        if let Some(pane) = self.app.state.focused_pane() {
            let cell_w = self.app.font.cell_width();
            let cell_h = self.app.font.cell_height();
            let ime_x = pane.cursor_col as f32 * cell_w;
            let ime_y = (pane.cursor_row + 1) as f32 * cell_h;
            if let Some(w) = &self.window {
                w.set_ime_cursor_area(
                    winit::dpi::PhysicalPosition::new(ime_x as i32, ime_y as i32),
                    winit::dpi::PhysicalSize::new(cell_w as u32, cell_h as u32),
                );
            }
        }
    }

    /// `WindowEvent::RedrawRequested`
    pub(super) fn on_redraw_requested(&mut self) {
        if let (Some(wgpu), Some(atlas)) = (&mut self.wgpu_state, &mut self.atlas)
            && let Err(e) = wgpu.render(
                &mut self.app.state,
                &mut self.app.font,
                atlas,
                &self.app.config.tab_bar,
                &self.app.config.colors,
                self.app.config.gpu.fps_limit,
                self.app.config.window.background_opacity,
                &self.app.config.cursor_style,
                self.app.config.window.padding_x as f32,
                self.app.config.window.padding_y as f32,
                &self.app.config,
            )
        {
            warn!("Render error: {}", e);
        }

        // GlyphAtlas の動的拡張: 満杯になったら 2 倍サイズで再生成する
        // 借用競合を避けるため atlas を一時的に取り出して処理する
        if let Some(mut atlas) = self.atlas.take() {
            if atlas.needs_grow
                && let Some(wgpu) = &self.wgpu_state
            {
                atlas = atlas.grow(&wgpu.device);
            }
            self.atlas = Some(atlas);
        }
    }
}
