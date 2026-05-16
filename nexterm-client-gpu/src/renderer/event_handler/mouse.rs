//! winit `WindowEvent` のうちマウス関連ハンドラ
//!
//! `event_handler.rs` から抽出した:
//! - `on_cursor_moved`
//! - `on_mouse_right_pressed` — コンテキストメニュー表示
//! - `on_mouse_left_pressed` — タブクリック / 設定パネル / 選択開始
//! - `on_mouse_left_released` — 選択確定・クリップボードコピー・URL オープン・フォーカス切替
//! - `on_mouse_wheel`

use std::time::{Duration, Instant};

use nexterm_proto::ClientToServer;
use winit::event::MouseScrollDelta;

use super::EventHandler;
use super::settings_panel_hit::SettingsPanelHit;
use crate::state::ContextMenu;
use crate::vertex_util::visual_width;

impl EventHandler {
    /// `WindowEvent::CursorMoved` — マウスカーソル位置を追跡し、ドラッグ中は選択範囲を更新する
    pub(super) fn on_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        self.cursor_position = Some((position.x, position.y));
        let cell_w = self.app.font.cell_width() as f64;
        let cell_h = self.app.font.cell_height() as f64;
        let tab_bar_h_f64 = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f64
        } else {
            0.0_f64
        };
        let col = (position.x / cell_w) as u16;
        let row = ((position.y - tab_bar_h_f64).max(0.0) / cell_h) as u16;
        if self.app.state.mouse_sel.is_dragging {
            self.app.state.mouse_sel.update(col, row);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            // ドラッグ中もマウスモーションをレポートする（ボタン0=左ドラッグ）
            if let Some(conn) = &self.connection {
                let _ = conn.send_tx.try_send(ClientToServer::MouseReport {
                    button: 0,
                    col,
                    row,
                    pressed: true,
                    motion: true,
                });
            }
        }

        // 設定パネルのスライダーをドラッグ中の場合、値をリアルタイム更新する
        {
            let fx = position.x as f32;
            let sp = &mut self.app.state.settings_panel;
            if let Some(drag) = &sp.drag_slider.clone() {
                use crate::settings_panel::SliderType;
                match drag.slider_type {
                    SliderType::FontSize => {
                        sp.set_font_size_from_slider(fx, drag.track_x, drag.track_w);
                    }
                    SliderType::WindowOpacity => {
                        sp.set_opacity_from_slider(fx, drag.track_x, drag.track_w);
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }

        // コンテキストメニューが開いている場合はホバー項目を更新する
        if let Some(menu) = &mut self.app.state.context_menu {
            let cw = self.app.font.cell_width();
            let ch = self.app.font.cell_height();
            let menu_w = 18.0 * cw;
            let fx = position.x as f32;
            let fy = position.y as f32;
            let mut new_hovered = None;
            if fx >= menu.x && fx <= menu.x + menu_w {
                for (i, _item) in menu.items.iter().enumerate() {
                    let item_y = menu.y + i as f32 * ch;
                    if fy >= item_y && fy < item_y + ch {
                        new_hovered = Some(i);
                        break;
                    }
                }
            }
            if menu.hovered != new_hovered {
                menu.hovered = new_hovered;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }
    }

    /// 右ボタン押下: コンテキストメニューを開く
    pub(super) fn on_mouse_right_pressed(&mut self) {
        if let Some((px, py)) = self.cursor_position {
            let cell_w_ctx = self.app.font.cell_width() as f64;
            let cell_h_ctx = self.app.font.cell_height() as f64;
            let profile_list: Vec<(String, String)> = self
                .app
                .config
                .profiles
                .iter()
                .map(|p| (p.name.clone(), p.icon.clone()))
                .collect();
            let tmp = ContextMenu::new_default(0.0, 0.0, &profile_list);
            let item_count = tmp.items.len();
            // メニュー幅を描画側と同じロジックで計算する
            let max_label = tmp
                .items
                .iter()
                .map(|i| visual_width(&i.label))
                .max()
                .unwrap_or(8);
            let max_hint = tmp
                .items
                .iter()
                .map(|i| visual_width(&i.hint))
                .max()
                .unwrap_or(0);
            let menu_w_px = ((max_label + max_hint + 5) as f64).max(16.0) * cell_w_ctx;
            let menu_h_px = item_count as f64 * cell_h_ctx;

            // ウィンドウ内に収まるように位置をクランプする
            let win_w = self
                .window
                .as_ref()
                .map(|w| w.inner_size().width as f64)
                .unwrap_or(800.0);
            let win_h = self
                .window
                .as_ref()
                .map(|w| w.inner_size().height as f64)
                .unwrap_or(600.0);
            let menu_x = (px).min(win_w - menu_w_px).max(0.0) as f32;
            let menu_y = (py).min(win_h - menu_h_px).max(0.0) as f32;

            self.app.state.context_menu =
                Some(ContextMenu::new_default(menu_x, menu_y, &profile_list));
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    /// 左ボタン押下: タブバークリック判定 + 選択開始 + マウスレポート
    pub(super) fn on_mouse_left_pressed(&mut self) {
        if let Some((px, py)) = self.cursor_position {
            // 設定パネルが開いている場合はヒットテストを先に実行する
            if self.app.state.settings_panel.is_open {
                let hit = self.hit_test_settings_panel(px as f32, py as f32);
                use crate::settings_panel::SliderType;
                match hit {
                    SettingsPanelHit::Outside => {
                        // パネル外クリック → パネルを閉じる
                        self.app.state.settings_panel.close();
                    }
                    SettingsPanelHit::Category(idx) => {
                        // サイドバーカテゴリをクリック → カテゴリ切り替え
                        if let Some(cat) = crate::settings_panel::SettingsCategory::ALL.get(idx) {
                            self.app.state.settings_panel.category = cat.clone();
                        }
                    }
                    SettingsPanelHit::Slider {
                        slider_type,
                        track_x,
                        track_w,
                        min: _,
                        max: _,
                    } => {
                        // スライダーをクリック → 即時値を反映してドラッグ状態を開始する
                        let fx = px as f32;
                        let sp = &mut self.app.state.settings_panel;
                        match slider_type {
                            SliderType::FontSize => {
                                sp.set_font_size_from_slider(fx, track_x, track_w)
                            }
                            SliderType::WindowOpacity => {
                                sp.set_opacity_from_slider(fx, track_x, track_w)
                            }
                        }
                        sp.drag_slider = Some(crate::settings_panel::SliderDrag {
                            slider_type,
                            track_x,
                            track_w,
                            min_val: if matches!(slider_type, SliderType::FontSize) {
                                8.0
                            } else {
                                0.1
                            },
                            max_val: if matches!(slider_type, SliderType::FontSize) {
                                32.0
                            } else {
                                1.0
                            },
                        });
                    }
                    SettingsPanelHit::ThemeColor(idx) => {
                        // テーマカラードットをクリック → スキーム切り替え
                        self.app.state.settings_panel.scheme_index = idx;
                        self.app.state.settings_panel.dirty = true;
                    }
                    SettingsPanelHit::TitleBar | SettingsPanelHit::PanelBackground => {
                        // その他のパネル内クリック → 何もしない
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                return; // 設定パネルが開いている間はターミナルにクリックを伝えない
            }

            let cell_w = self.app.font.cell_width() as f64;
            let cell_h = self.app.font.cell_height() as f64;
            let tab_bar_h_f64 = if self.app.config.tab_bar.enabled {
                self.app.config.tab_bar.height as f64
            } else {
                0.0_f64
            };

            // タブバーエリア（py < tab_bar_h）のクリックを処理する
            if self.app.config.tab_bar.enabled && py < tab_bar_h_f64 {
                let px_f32 = px as f32;
                // 設定ボタンのクリック判定
                let hit_settings = self
                    .app
                    .state
                    .settings_tab_rect
                    .map(|(x0, x1)| px_f32 >= x0 && px_f32 < x1)
                    .unwrap_or(false);
                if hit_settings {
                    self.app.state.settings_panel.is_open = !self.app.state.settings_panel.is_open;
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                } else {
                    // タブクリックでペインフォーカスを切り替える
                    let hit_pane = self
                        .app
                        .state
                        .tab_hit_rects
                        .iter()
                        .find(|&(_, &(x0, x1))| px_f32 >= x0 && px_f32 < x1)
                        .map(|(&id, _)| id);
                    if let Some(pane_id) = hit_pane {
                        let now = Instant::now();
                        // ダブルクリック判定（300ms 以内に同一ペインを再クリック）
                        let is_double_click = self
                            .last_tab_click
                            .map(|(t, id)| {
                                id == pane_id && now.duration_since(t) < Duration::from_millis(300)
                            })
                            .unwrap_or(false);

                        if is_double_click {
                            // ダブルクリック → タブ名変更モードへ
                            let current_name = self
                                .app
                                .state
                                .panes
                                .get(&pane_id)
                                .map(|p| p.title.clone())
                                .filter(|t| !t.is_empty())
                                .unwrap_or_else(|| format!("pane:{}", pane_id));
                            self.app
                                .state
                                .settings_panel
                                .begin_tab_rename(pane_id, &current_name);
                            self.last_tab_click = None;
                        } else {
                            self.last_tab_click = Some((now, pane_id));
                            if self.app.state.focused_pane_id != Some(pane_id)
                                && let Some(conn) = &self.connection
                            {
                                let _ =
                                    conn.send_tx.try_send(ClientToServer::FocusPane { pane_id });
                            }
                        }
                    }
                }
                return; // タブバー内のクリックはターミナルに伝えない
            }

            let col = (px / cell_w) as u16;
            let row = ((py - tab_bar_h_f64).max(0.0) / cell_h) as u16;
            self.app.state.mouse_sel.begin(col, row);
            // マウスレポーティングが有効なら PTY にイベントを送信する
            if let Some(conn) = &self.connection {
                let _ = conn.send_tx.try_send(ClientToServer::MouseReport {
                    button: 0,
                    col,
                    row,
                    pressed: true,
                    motion: false,
                });
            }
        }
    }

    /// 左ボタンリリース: 選択確定 → クリップボードコピー or フォーカス切替
    pub(super) fn on_mouse_left_released(&mut self) {
        // 設定パネルのスライダードラッグを終了して設定を保存する
        if self.app.state.settings_panel.drag_slider.take().is_some() {
            let _ = self.app.state.settings_panel.save_to_toml();
            self.app.state.settings_panel.dirty = false;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }

        // コンテキストメニューが開いている場合はクリックで処理する
        if let Some((px, py)) = self.cursor_position
            && let Some(menu) = self.app.state.context_menu.take()
        {
            let cell_w = self.app.font.cell_width();
            let cell_h = self.app.font.cell_height();
            // 描画幅と同じ値を使用する（ここを変えると描画とクリック判定がずれる）
            let menu_w = 18.0 * cell_w;
            let fx = px as f32;
            let fy = py as f32;
            if fx >= menu.x && fx <= menu.x + menu_w {
                for (i, item) in menu.items.iter().enumerate() {
                    let item_y = menu.y + i as f32 * cell_h;
                    if fy >= item_y && fy < item_y + cell_h {
                        self.execute_context_menu_action(&item.action);
                        break;
                    }
                }
            }
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }

        if let Some((px, py)) = self.cursor_position {
            let cell_w = self.app.font.cell_width() as f64;
            let cell_h = self.app.font.cell_height() as f64;
            let tab_bar_h_f64 = if self.app.config.tab_bar.enabled {
                self.app.config.tab_bar.height as f64
            } else {
                0.0_f64
            };
            let click_col = (px / cell_w) as u16;
            let click_row = ((py - tab_bar_h_f64).max(0.0) / cell_h) as u16;

            // ドラッグ選択を終了して選択テキストをコピーする
            self.app.state.mouse_sel.update(click_col, click_row);
            self.app.state.mouse_sel.finish();

            if let Some(((sc, sr), (ec, er))) = self.app.state.mouse_sel.normalized() {
                // 選択範囲があればテキストを抽出してクリップボードにコピーする
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
                            let line: String = row
                                [col_start.min(row.len())..col_end.min(row.len())]
                                .iter()
                                .map(|c| c.ch)
                                .collect();
                            lines.push(line.trim_end().to_string());
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
                // 選択後はリターン（ペインフォーカス切替を行わない）
                return;
            }

            // 選択なし（単純クリック）: Ctrl+クリックで URL を開く
            // SecurityConfig.external_url ポリシーに従って同意フローを経由する
            if self.modifiers.control_key()
                && let Some(url) = self.find_url_at(click_col, click_row)
            {
                self.request_open_url(url);
                return;
            }

            // クリック座標が含まれるペインを探してフォーカスを移動する
            let target_pane = self
                .app
                .state
                .pane_layouts
                .values()
                .find(|l| {
                    click_col >= l.col_offset
                        && click_col < l.col_offset + l.cols
                        && click_row >= l.row_offset
                        && click_row < l.row_offset + l.rows
                })
                .map(|l| l.pane_id);
            if let Some(pane_id) = target_pane
                && self.app.state.focused_pane_id != Some(pane_id)
                && let Some(conn) = &self.connection
            {
                let _ = conn.send_tx.try_send(ClientToServer::FocusPane { pane_id });
            }
        }
    }

    /// `WindowEvent::MouseWheel` — マウスホイールでスクロールバックをスクロールする
    pub(super) fn on_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as i32,
            MouseScrollDelta::PixelDelta(p) => {
                // Windows タッチパッドは PixelDelta を送る。
                // 積算してセル高さ分溜まったら1行スクロールし、端数は次回に持ち越す。
                self.pixel_scroll_accumulator += p.y;
                let cell_h = self.app.font.cell_height() as f64;
                let lines = (self.pixel_scroll_accumulator / cell_h) as i32;
                self.pixel_scroll_accumulator -= lines as f64 * cell_h;
                lines
            }
        };
        if lines > 0 {
            self.app.state.scroll_up(lines as usize);
        } else if lines < 0 {
            self.app.state.scroll_down((-lines) as usize);
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
