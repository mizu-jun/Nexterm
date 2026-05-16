//! winit `WindowEvent` のうちウィンドウ・IME 関連ハンドラ
//!
//! `event_handler.rs` から抽出した:
//! - `on_close_requested`
//! - `on_resized` / `on_scale_factor_changed`
//! - `on_modifiers_changed`
//! - `on_ime`
//! - `on_redraw_requested`

use nexterm_proto::ClientToServer;
use tracing::warn;
use winit::{event::Ime, event_loop::ActiveEventLoop, keyboard::ModifiersState};

use super::EventHandler;
use crate::glyph_atlas::GlyphAtlas;

impl EventHandler {
    /// `WindowEvent::CloseRequested`
    pub(super) fn on_close_requested(&mut self, event_loop: &ActiveEventLoop) {
        // IPC 接続を先にドロップしてチャネルを閉じる（Windows でのハング防止）
        self.connection = None;
        // サーバータスクを abort してからイベントループを終了する
        self.server_handle.abort();
        event_loop.exit();
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
