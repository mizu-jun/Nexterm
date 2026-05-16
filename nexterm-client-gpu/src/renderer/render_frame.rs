//! 1 フレーム描画の本体
//!
//! `renderer/mod.rs` から抽出した:
//! - `impl WgpuState { fn render }` — FPS 制限・surface 取得・グリッド/オーバーレイ
//!   頂点構築 → アップロード → メインパス（bg + text）→ 画像パスの順に描画する

use std::time::{Duration, Instant};

use anyhow::Result;
use wgpu::util::DeviceExt;

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::WgpuState;
use super::background_pass::build_background_verts;
use super::image::build_image_verts;

impl WgpuState {
    /// 1フレームを描画する
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &mut self,
        state: &mut ClientState,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        tab_bar_cfg: &nexterm_config::TabBarConfig,
        color_scheme: &nexterm_config::ColorScheme,
        fps_limit: u32,
        background_opacity: f32,
        cursor_style: &nexterm_config::CursorStyle,
        padding_x: f32,
        padding_y: f32,
        // Sprint 5-7 / UI-1-4: キーヒントオーバーレイ用の全設定参照
        config: &nexterm_config::Config,
    ) -> Result<()> {
        // FPS 制限: 前フレームからの経過時間が 1/fps より短い場合はスキップ
        if fps_limit > 0 {
            let frame_duration = Duration::from_secs_f64(1.0 / fps_limit as f64);
            if self.last_frame_at.elapsed() < frame_duration {
                return Ok(());
            }
        }
        self.last_frame_at = Instant::now();

        // フレーム開始時にアトラスのリセットフラグをクリアする
        // （前フレームでリセットされていても、このフレームで正しいUVを使って再描画する）
        atlas.cleared_this_frame = false;

        // カラースキームからパレットを導出する（毎フレーム; コストは小さい）
        let scheme_palette: Option<nexterm_config::SchemePalette> = match color_scheme {
            nexterm_config::ColorScheme::Builtin(s) => Some(s.palette()),
            nexterm_config::ColorScheme::Custom(p) => {
                // Custom パレットを SchemePalette に変換
                let parse_hex = |s: &str| -> [u8; 3] {
                    let s = s.trim_start_matches('#');
                    let v = u32::from_str_radix(s, 16).unwrap_or(0);
                    [
                        ((v >> 16) & 0xFF) as u8,
                        ((v >> 8) & 0xFF) as u8,
                        (v & 0xFF) as u8,
                    ]
                };
                let mut ansi = [[0u8; 3]; 16];
                for (i, hex) in p.ansi.iter().enumerate().take(16) {
                    ansi[i] = parse_hex(hex);
                }
                Some(nexterm_config::SchemePalette {
                    fg: parse_hex(&p.foreground),
                    bg: parse_hex(&p.background),
                    ansi,
                })
            }
        };
        let palette_ref = scheme_palette.as_ref();
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        let sw = self.surface_config.width as f32;
        let sh = self.surface_config.height as f32;
        let cell_w = font.cell_width();
        let cell_h = font.cell_height();

        // タブバー高さ（有効時のみ）: ターミナルコンテンツのy-offsetとして使用
        let tab_bar_h = if tab_bar_cfg.enabled {
            tab_bar_cfg.height as f32
        } else {
            0.0
        };
        // パディングを加味した実効オフセット（グリッド描画の基点）
        let _grid_offset_x = padding_x;
        let grid_offset_y = tab_bar_h + padding_y;

        let mut bg_verts: Vec<BgVertex> = Vec::new();
        let mut bg_idx: Vec<u16> = Vec::new();
        let mut text_verts: Vec<TextVertex> = Vec::new();
        let mut text_idx: Vec<u16> = Vec::new();

        // レイアウト情報がある場合は全ペインを分割表示する
        if !state.pane_layouts.is_empty() {
            // 各ペインをレイアウト矩形に従って描画する
            let layout_ids: Vec<u32> = state.pane_layouts.keys().copied().collect();
            for pane_id in layout_ids {
                let is_focused = state.focused_pane_id == Some(pane_id);
                if let (Some(layout), Some(pane)) =
                    (state.pane_layouts.get(&pane_id), state.panes.get(&pane_id))
                {
                    if pane.scroll_offset > 0 && is_focused {
                        self.build_scrollback_verts_in_rect(
                            pane,
                            layout,
                            sw,
                            sh,
                            cell_w,
                            cell_h,
                            grid_offset_y,
                            font,
                            atlas,
                            palette_ref,
                            &mut bg_verts,
                            &mut bg_idx,
                            &mut text_verts,
                            &mut text_idx,
                        );
                    } else {
                        self.build_grid_verts_in_rect(
                            pane,
                            layout,
                            is_focused,
                            &state.mouse_sel,
                            sw,
                            sh,
                            cell_w,
                            cell_h,
                            grid_offset_y,
                            font,
                            atlas,
                            palette_ref,
                            cursor_style,
                            &mut bg_verts,
                            &mut bg_idx,
                            &mut text_verts,
                            &mut text_idx,
                        );
                    }
                }
            }
            // ペイン境界線を描画する
            self.build_border_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                tab_bar_h,
                &mut bg_verts,
                &mut bg_idx,
            );
        } else if let Some(pane) = state.focused_pane() {
            // フォールバック: レイアウト情報なし（接続直後など）
            if pane.scroll_offset > 0 {
                // ---- スクロールバック表示モード ----
                self.build_scrollback_verts(
                    pane,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    grid_offset_y,
                    font,
                    atlas,
                    palette_ref,
                    &mut bg_verts,
                    &mut bg_idx,
                    &mut text_verts,
                    &mut text_idx,
                );
            } else {
                // ---- 通常グリッド表示 ----
                self.build_grid_verts(
                    pane,
                    &state.mouse_sel,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    grid_offset_y,
                    font,
                    atlas,
                    palette_ref,
                    cursor_style,
                    &mut bg_verts,
                    &mut bg_idx,
                    &mut text_verts,
                    &mut text_idx,
                );
            }
        }

        // ---- ペイン番号オーバーレイ（display_panes_mode 有効時） ----
        if state.display_panes_mode {
            let mut sorted_pane_ids: Vec<u32> = state.pane_layouts.keys().copied().collect();
            sorted_pane_ids.sort();
            for (number, pane_id) in sorted_pane_ids.iter().enumerate() {
                if let Some(layout) = state.pane_layouts.get(pane_id) {
                    let px = layout.col_offset as f32 * cell_w;
                    let py = layout.row_offset as f32 * cell_h + tab_bar_h;
                    let badge_w = cell_w * 2.0;
                    let badge_h = cell_h;
                    // 黄色背景バッジ
                    add_px_rect(
                        px,
                        py,
                        badge_w,
                        badge_h,
                        [0.9, 0.75, 0.0, 0.90],
                        sw,
                        sh,
                        &mut bg_verts,
                        &mut bg_idx,
                    );
                    // ペイン番号テキスト（1 始まり）
                    let label = (number + 1).to_string();
                    add_string_verts(
                        &label,
                        px + 2.0,
                        py,
                        [0.0, 0.0, 0.0, 1.0],
                        true,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        &mut text_verts,
                        &mut text_idx,
                    );
                }
            }
            // レイアウト情報がない場合（フォールバック: フォーカスペインのみ）
            if state.pane_layouts.is_empty()
                && let Some(focused_id) = state.focused_pane_id
            {
                add_px_rect(
                    0.0,
                    tab_bar_h,
                    cell_w * 2.0,
                    cell_h,
                    [0.9, 0.75, 0.0, 0.90],
                    sw,
                    sh,
                    &mut bg_verts,
                    &mut bg_idx,
                );
                let label = focused_id.to_string();
                add_string_verts(
                    &label,
                    2.0,
                    tab_bar_h,
                    [0.0, 0.0, 0.0, 1.0],
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    &mut text_verts,
                    &mut text_idx,
                );
            }
        }

        // ---- タブバー（設定で有効な場合）----
        if tab_bar_cfg.enabled {
            self.build_tab_bar_verts(
                state,
                tab_bar_cfg,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- ステータスライン（常時表示） ----
        self.build_status_verts(
            state,
            sw,
            sh,
            cell_w,
            cell_h,
            font,
            atlas,
            &mut bg_verts,
            &mut bg_idx,
            &mut text_verts,
            &mut text_idx,
        );

        // ---- 検索バー（アクティブ時） ----
        if state.search.is_active {
            self.build_search_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- Quick Select オーバーレイ（アクティブ時） ----
        if state.quick_select.is_active {
            self.build_quick_select_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- SFTP ファイル転送ダイアログ(オープン時) ----
        if state.file_transfer.is_open {
            self.build_file_transfer_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- Lua マクロピッカー(オープン時) ----
        if state.macro_picker.is_open {
            self.build_macro_picker_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- ホストマネージャ(オープン時) ----
        if state.host_manager.is_open {
            self.build_host_manager_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }
        if state.host_manager.password_modal.is_some() {
            self.build_password_modal_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- コマンドパレット(オープン時) ----
        if state.palette.is_open {
            self.build_palette_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- 設定パネル（Ctrl+, でオープン） ----
        if state.settings_panel.is_open {
            self.build_settings_panel_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- コンテキストメニュー（右クリック時） ----
        if let Some(ref menu) = state.context_menu {
            self.build_context_menu_verts(
                menu,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- 更新通知バナー（画面上部） ----
        if state.update_banner.is_some() {
            self.build_update_banner_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- 同意ダイアログ（Sprint 4-1: 機密操作確認モーダル）----
        // 最前面表示するため最後に追加する
        if state.pending_consent.is_some() {
            self.build_consent_dialog_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- キーヒントオーバーレイ（Sprint 5-7 / UI-1-4）----
        // Leader 単独押下後 2 秒間、画面下部に prefix 系バインドの一覧を表示
        self.build_key_hint_verts(
            state,
            config,
            sw,
            sh,
            cell_w,
            cell_h,
            font,
            atlas,
            &mut bg_verts,
            &mut bg_idx,
            &mut text_verts,
            &mut text_idx,
        );

        // ---- IME プリエディットオーバーレイ（変換中テキスト） ----
        if let Some(ref preedit) = state.ime_preedit
            && let Some(pane) = state.focused_pane()
        {
            let px = pane.cursor_col as f32 * cell_w;
            let py = (pane.cursor_row + 1) as f32 * cell_h;
            // プリエディット背景（やや明るいグレー）
            let text_width = preedit.chars().count() as f32 * cell_w;
            add_px_rect(
                px,
                py,
                text_width.max(cell_w),
                cell_h,
                [0.25, 0.25, 0.30, 0.90],
                sw,
                sh,
                &mut bg_verts,
                &mut bg_idx,
            );
            // アンダーライン（黄色）
            add_px_rect(
                px,
                py + cell_h - 2.0,
                text_width.max(cell_w),
                2.0,
                [1.0, 0.85, 0.2, 1.0],
                sw,
                sh,
                &mut bg_verts,
                &mut bg_idx,
            );
            // プリエディットテキスト
            add_string_verts(
                preedit,
                px,
                py,
                [1.0, 1.0, 0.6, 1.0],
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- GPU バッファへアップロード（再利用バッファへ write_buffer で上書き）----
        // 容量不足の場合のみ新規確保する（2倍に拡張）
        self.upload_bg_verts(&bg_verts, &bg_idx);
        self.upload_txt_verts(&text_verts, &text_idx);

        let text_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text_bind_group"),
            layout: &self.text_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });

        // ---- クリアパス（パレット背景色で塗りつぶし） ----
        // 背景画像がある場合は描画の前にクリアを完了させるため、独立パスにする
        {
            let clear_bg = scheme_palette.as_ref().map(|p| p.bg).unwrap_or([0, 0, 0]);
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_bg[0] as f64 / 255.0,
                            g: clear_bg[1] as f64 / 255.0,
                            b: clear_bg[2] as f64 / 255.0,
                            // background_opacity 設定値を alpha に反映（透過ターミナル対応）
                            a: background_opacity as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        // ---- 背景画像パス（Sprint 5-7 / Phase 3-1）----
        // 設定されている場合のみ、clear の後・bg_verts の前に描画する。
        // image_pipeline を再利用（独自パイプラインは作らない）
        if let Some(ref bg_img) = self.background {
            let (bg_verts_img, bg_idx_img) = build_background_verts(
                sw,
                sh,
                bg_img.width,
                bg_img.height,
                &bg_img.fit,
                bg_img.opacity,
            );
            if !bg_idx_img.is_empty() {
                let vbuf = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("bg_image_vbuf"),
                        contents: bytemuck::cast_slice(&bg_verts_img),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                let ibuf = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("bg_image_ibuf"),
                        contents: bytemuck::cast_slice(&bg_idx_img),
                        usage: wgpu::BufferUsages::INDEX,
                    });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("background_image_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.image_pipeline);
                pass.set_bind_group(0, &bg_img.bind_group, &[]);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..bg_idx_img.len() as u32, 0, 0..1);
            }
        }

        // ---- メインレンダーパス（セル背景 + テキスト） ----
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // クリア + 背景画像の結果を保持して上にセル背景・テキストを描く
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if !bg_idx.is_empty() {
                pass.set_pipeline(&self.bg_pipeline);
                pass.set_vertex_buffer(0, self.buf_bg_v.slice(..));
                pass.set_index_buffer(self.buf_bg_i.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..bg_idx.len() as u32, 0, 0..1);
            }
            if !text_idx.is_empty() {
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, &text_bind_group, &[]);
                pass.set_vertex_buffer(0, self.buf_txt_v.slice(..));
                pass.set_index_buffer(self.buf_txt_i.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..text_idx.len() as u32, 0, 0..1);
            }
        }

        // ---- 画像レンダーパス（配置済み画像をオーバーレイ） ----
        if let Some(pane) = state.focused_pane() {
            for (id, img) in &pane.images {
                self.ensure_image_texture(*id, img);
            }
            for (id, img) in &pane.images {
                if let Some(entry) = self.image_textures.get(id) {
                    let (img_verts, img_idx) = build_image_verts(img, sw, sh, cell_w, cell_h);
                    let img_vbuf =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("img_vbuf"),
                                contents: bytemuck::cast_slice(&img_verts),
                                usage: wgpu::BufferUsages::VERTEX,
                            });
                    let img_ibuf =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("img_ibuf"),
                                contents: bytemuck::cast_slice(&img_idx),
                                usage: wgpu::BufferUsages::INDEX,
                            });
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("image_render_pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.image_pipeline);
                        pass.set_bind_group(0, &entry.bind_group, &[]);
                        pass.set_vertex_buffer(0, img_vbuf.slice(..));
                        pass.set_index_buffer(img_ibuf.slice(..), wgpu::IndexFormat::Uint16);
                        pass.draw_indexed(0..img_idx.len() as u32, 0, 0..1);
                    }
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}
