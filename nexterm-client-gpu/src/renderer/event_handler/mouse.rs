//! winit `WindowEvent` のうちマウス関連ハンドラ
//!
//! `event_handler.rs` から抽出した:
//! - `on_cursor_moved`
//! - `on_mouse_right_pressed` — コンテキストメニュー表示
//! - `on_mouse_left_pressed` — タブクリック / 設定パネル / 選択開始
//! - `on_mouse_left_released` — 選択確定・クリップボードコピー・URL オープン・フォーカス切替
//! - `on_mouse_wheel`

use std::sync::Arc;
use std::time::{Duration, Instant};

use nexterm_proto::ClientToServer;
use winit::event::MouseScrollDelta;

use super::EventHandler;
use super::settings_panel_hit::SettingsPanelHit;
use crate::state::ContextMenu;
use crate::vertex_util::visual_width;

/// タブドラッグの新順序を計算する（Sprint 5-7 / Phase 2-3）。
///
/// `current` から `dragged_id` を取り出し、`target_id` の位置に挿入する。
/// 挙動: 「ターゲットタブの位置に dragged を押し込む」モデル。
///
/// - `from < target_pos`（右へ移動）: dragged を取り除くと target が 1 つ左にズレるため、
///   `insert_at = target_pos - 1` で「元の target_pos」と同じ表示位置に着地する。
/// - `from > target_pos`（左へ移動）: dragged の削除は target に影響しないため、
///   `insert_at = target_pos` で target を右に 1 つ押し出す。
///
/// 隣接 swap（`|from - target_pos| == 1` のうち右ドラッグ）は本モデルでは結果が元と
/// 同一になり `None` を返す（往復不要）。左右判定が必要な場合は将来 `hover_target` を
/// `(pane_id, Before/After)` に拡張すること。
///
/// `current` に `dragged_id` または `target_id` が含まれない場合は `None`。
pub(super) fn compute_reordered_tab_order(
    current: &[u32],
    dragged_id: u32,
    target_id: u32,
) -> Option<Vec<u32>> {
    if dragged_id == target_id {
        return None;
    }
    let from = current.iter().position(|&id| id == dragged_id)?;
    let target_pos = current.iter().position(|&id| id == target_id)?;

    let mut new_order: Vec<u32> = current.to_vec();
    new_order.remove(from);
    let insert_at = if from < target_pos {
        target_pos - 1
    } else {
        target_pos
    };
    new_order.insert(insert_at, dragged_id);

    if new_order == current {
        return None;
    }
    Some(new_order)
}

impl EventHandler {
    /// タブバー外でドロップされた場合の処理（Sprint 5-8 Phase 4-2）。
    ///
    /// `drag.current_screen_pos` と現在登録されている全 OS Window の bounds から
    /// `compute_drop_target` を呼んでドロップ先を判定する:
    ///
    /// - `SameWindow`: 同一 Window 内ペイン領域 → 何もしない（既存挙動と一致）
    /// - `OtherWindowTabBar`: 別 Window のタブバー上 → Phase 4-4 で merge 実装、現状はログのみ
    /// - `NewWindow`: どの Window 外 → `spawn_os_window` 呼び出し（Phase 4-2 はスケルトン）
    ///
    /// `current_screen_pos` が `None`（Wayland 等のフォールバック失敗）の場合は
    /// 何もしない（決定 #4 の代替 UX が機能パリティを提供）。
    fn handle_tab_drag_drop_outside(&mut self, drag: &crate::state::TabDragState) {
        let Some(drop_pos) = drag.current_screen_pos else {
            tracing::debug!("タブ外ドロップ: グローバル座標取得不能（Wayland 等）→ 機能無効化");
            return;
        };
        let Some(source_id) = drag.source_os_window_id else {
            tracing::debug!("タブ外ドロップ: source_os_window_id 未設定 → スキップ");
            return;
        };

        // 現在登録されている OS Window の bounds を収集する
        // Phase 4-2 時点では主 Window 1 個のみだが、Phase 4-4 以降で複数 Window 化対応
        let tab_bar_h = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f32
        } else {
            0.0
        };
        let mut bounds_vec: Vec<crate::drop_target::OsWindowBounds<winit::window::WindowId>> =
            Vec::new();
        if let Some(w) = &self.window
            && let Ok(outer_pos) = w.outer_position()
        {
            let outer_size = w.outer_size();
            bounds_vec.push(crate::drop_target::OsWindowBounds {
                window_id: w.id(),
                position: (outer_pos.x, outer_pos.y),
                size: (outer_size.width, outer_size.height),
                tab_bar_y_range: (0.0, tab_bar_h),
            });
        }
        // Phase 4-1 で導入した `self.windows` HashMap の OS Window も収集する。
        // 主 Window と重複しないように id でフィルタする（Phase 4-4 で `self.window` 廃止予定）。
        for (id, cw) in &self.windows {
            if Some(*id) == self.window.as_ref().map(|w| w.id()) {
                continue;
            }
            if let Ok(outer_pos) = cw.window.outer_position() {
                let outer_size = cw.window.outer_size();
                bounds_vec.push(crate::drop_target::OsWindowBounds {
                    window_id: *id,
                    position: (outer_pos.x, outer_pos.y),
                    size: (outer_size.width, outer_size.height),
                    tab_bar_y_range: (0.0, tab_bar_h),
                });
            }
        }

        let target = crate::drop_target::compute_drop_target(drop_pos, source_id, &bounds_vec);
        match target {
            crate::drop_target::DropTarget::SameWindow { .. } => {
                // ペイン領域へのドロップ: 既存仕様と整合（何もしない）
            }
            crate::drop_target::DropTarget::OtherWindowTabBar { window_id } => {
                // Sprint 5-8 Phase 4-4 Step D: 別 OS Window のタブバーにドロップ。
                //
                // target の OS Window が表示しているサーバー Window ID (`focused_server_window_id`)
                // を解決して `MovePaneToWindow { target_window_id }` を送信する。
                //
                // 解決順序:
                // 1. `self.windows` に登録された追加 OS Window → `view_state.focused_server_window_id`
                // 2. 主 Window（`self.window`）→ `self.app.state.focused_server_window_id`
                //    （`WindowListChanged` で更新される）
                let target_server_id = if let Some(cw) = self.windows.get(&window_id) {
                    Some(cw.view_state.focused_server_window_id)
                } else if self.window.as_ref().map(|w| w.id()) == Some(window_id) {
                    let id = self.app.state.focused_server_window_id;
                    if id == 0 { None } else { Some(id) }
                } else {
                    None
                };

                match target_server_id {
                    Some(target) => {
                        tracing::info!(
                            "タブ外ドロップ: 別 OS Window のタブバーにドロップ (os_window={:?}, target_server_window={})",
                            window_id,
                            target
                        );
                        if let Some(conn) = &self.connection {
                            let _ = conn.send_tx.try_send(
                                nexterm_proto::ClientToServer::MovePaneToWindow {
                                    pane_id: drag.pane_id,
                                    target_window_id: target,
                                    insert_at: None, // Phase 4-5 でホバー位置に応じた挿入位置指定対応
                                },
                            );
                        }
                    }
                    None => {
                        tracing::warn!(
                            "OtherWindowTabBar 分岐: target OS Window の server_window_id を解決できません (window_id={:?})",
                            window_id
                        );
                    }
                }
            }
            crate::drop_target::DropTarget::NewWindow => {
                tracing::info!(
                    "タブ外ドロップ: 新規 Window 生成要求送信（drop_pos={:?}, pane_id={}）",
                    drop_pos,
                    drag.pane_id
                );
                // Sprint 5-8 Phase 4-3 + 4-4:
                // 1. サーバーに `MovePaneToWindow { target_window_id: 0 }` を送り、サーバー側で
                //    新規 Server Window を生成してペインを移動する
                // 2. クライアント側 OS Window スポーンは「サーバーから WindowListChanged が返って
                //    新規 Window ID を検出したとき」に `EventLoopProxy<UserEvent::SpawnOsWindow>`
                //    経由で発火する（Step C で実装）
                // 3. ドロップ位置を `pending_new_window_drop_pos` に記録しておき、スポーン時の
                //    Window 位置として使う
                self.pending_new_window_drop_pos =
                    Some(winit::dpi::PhysicalPosition::new(drop_pos.0, drop_pos.1));
                if let Some(conn) = &self.connection {
                    let _ =
                        conn.send_tx
                            .try_send(nexterm_proto::ClientToServer::MovePaneToWindow {
                                pane_id: drag.pane_id,
                                target_window_id: 0,
                                insert_at: None,
                            });
                }
            }
        }
    }

    /// マウスカーソルのグローバルスクリーン座標を解決する（Sprint 5-8 Phase 4-2）。
    ///
    /// 優先順位:
    /// 1. プラットフォーム別 OS API（Windows: `GetCursorPos`）から取得
    /// 2. winit の `window.outer_position()` + クライアント領域のカーソル座標を加算
    /// 3. どちらも失敗（Wayland など）→ `None`
    ///
    /// `client_x` / `client_y` は winit `CursorMoved` の `position`（ウィンドウ
    /// クライアント領域の左上原点座標、ピクセル単位）。フォールバック計算で使用する。
    ///
    /// 戻り値 `None` の場合、呼び出し側はタブ外ドロップ判定を実行せず、既存の
    /// `ReorderPanes` 経路にフォールバックする（決定 #4: Wayland は代替 UX）。
    fn resolve_screen_pos(
        window: &Option<Arc<winit::window::Window>>,
        client_x: i32,
        client_y: i32,
    ) -> Option<(i32, i32)> {
        if let Some(pos) = crate::platform::cursor_screen_pos() {
            return Some(pos);
        }
        let outer = window.as_ref()?.outer_position().ok()?;
        Some((outer.x + client_x, outer.y + client_y))
    }

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

        // Sprint 5-7 / UI-1-1: タブバー上のホバー追跡
        // カーソルがタブバー領域内（y < tab_bar_h）にあるとき、x 座標で hit テストして
        // ホバー中のタブ ID を更新する。範囲外なら None。タブバー無効時は常に None。
        let prev_hovered = self.app.state.hovered_tab_id;
        let new_hovered = if self.app.config.tab_bar.enabled && position.y < tab_bar_h_f64 {
            let px = position.x as f32;
            self.app
                .state
                .tab_hit_rects
                .iter()
                .find(|&(_, &(x0, x1))| px >= x0 && px < x1)
                .map(|(&id, _)| id)
        } else {
            None
        };
        if prev_hovered != new_hovered {
            self.app.state.hovered_tab_id = new_hovered;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }

        // Sprint 5-7 / Phase 2-3: タブドラッグ中の追跡
        // タブバー領域内 + 進行中ドラッグがある場合、current_x / hover_target / committed を更新する
        //
        // Sprint 5-8 Phase 4-2: タブ外ドロップ判定用に `current_screen_pos` も更新する。
        // Windows は OS API（GetCursorPos）で正確に取得、その他は winit の
        // outer_position + クライアント座標でフォールバック計算する。
        if self.app.state.tab_drag.is_some() {
            let new_screen_pos =
                Self::resolve_screen_pos(&self.window, position.x as i32, position.y as i32);
            if let Some(drag) = self.app.state.tab_drag.as_mut() {
                let px_f32 = position.x as f32;
                drag.current_x = px_f32;
                drag.current_screen_pos = new_screen_pos;
                // 6px 以上動いたらドラッグ確定
                const DRAG_THRESHOLD: f32 = 6.0;
                if !drag.committed && (px_f32 - drag.start_x).abs() >= DRAG_THRESHOLD {
                    drag.committed = true;
                }
                // 挿入先タブを決定（タブバー領域内のタブにヒットしているか）
                let on_tab_bar = position.y < tab_bar_h_f64;
                drag.hover_target = if on_tab_bar {
                    self.app
                        .state
                        .tab_hit_rects
                        .iter()
                        .find(|&(_, &(x0, x1))| px_f32 >= x0 && px_f32 < x1)
                        .map(|(&id, _)| id)
                } else {
                    None
                };
                if drag.committed
                    && let Some(w) = &self.window
                {
                    w.request_redraw();
                }
            }
        }
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
                } else if let Some(tearout_pane_id) = self
                    .app
                    .state
                    .tab_tearout_hit_rects
                    .iter()
                    .find(|&(_, &(x0, x1))| px_f32 >= x0 && px_f32 < x1)
                    .map(|(&id, _)| id)
                {
                    // Sprint 5-9 Phase 4-6: タブホバー `[↗]` ボタンクリックで分離。
                    // タブクリック判定よりも先に評価することで、タブの範囲内に重なる
                    // 分離ボタン領域がフォーカス切替を発火させないようにする。
                    // 経路は `execute_action("DetachToNewWindow")` と同じ
                    // （BreakPane + pending_new_window_drop_pos セット）。
                    tracing::info!(
                        "[↗] tearout ボタンクリック: pane_id={} を新規 OS Window に分離",
                        tearout_pane_id
                    );
                    // pos = (0, 0) で記録（マウス座標非依存、winit が位置決定）
                    self.pending_new_window_drop_pos =
                        Some(winit::dpi::PhysicalPosition::new(0, 0));
                    if let Some(conn) = &self.connection {
                        // 対象ペインを focused にしてから BreakPane を送る方が安全だが、
                        // `[↗]` は hover 中タブにしか表示されないため、現状のフォーカス
                        // 以外でクリックされる可能性は低い。将来必要なら FocusPane を先送り。
                        // 確実性のため pane_id が focused でない場合は FocusPane を挟む。
                        if self.app.state.focused_pane_id != Some(tearout_pane_id) {
                            let _ =
                                conn.send_tx
                                    .try_send(nexterm_proto::ClientToServer::FocusPane {
                                        pane_id: tearout_pane_id,
                                    });
                        }
                        let _ = conn
                            .send_tx
                            .try_send(nexterm_proto::ClientToServer::BreakPane);
                    }
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
                            // Sprint 5-7 / Phase 2-3: ドラッグ可能性を記録（committed=false）。
                            // CursorMoved で閾値を超えたら committed=true となり、Released で並べ替えを送信する。
                            //
                            // Sprint 5-8 Phase 4-2 追加フィールド:
                            // - `source_os_window_id`: ドラッグ元 OS Window（主 Window の id を保持）
                            // - `start_screen_pos` / `current_screen_pos`: グローバル座標
                            //   Windows は `platform::cursor_screen_pos` で OS から取得。
                            //   その他は winit の `outer_position` + クライアント座標で
                            //   フォールバック計算する。Wayland では outer_position が
                            //   取れず `None` のままになり、タブ外ドロップ判定が無効化される。
                            let screen_pos =
                                Self::resolve_screen_pos(&self.window, px as i32, py as i32);
                            self.app.state.tab_drag = Some(crate::state::TabDragState {
                                pane_id,
                                start_x: px_f32,
                                current_x: px_f32,
                                hover_target: Some(pane_id),
                                committed: false,
                                source_os_window_id: self.window.as_ref().map(|w| w.id()),
                                start_screen_pos: screen_pos,
                                current_screen_pos: screen_pos,
                            });
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

        // Sprint 5-7 / Phase 2-3: タブドラッグ終了処理
        // committed なら新順序を計算して ReorderPanes を送信、未 committed は通常クリック扱い（なにもしない）
        //
        // Sprint 5-8 Phase 4-2: タブバー外（hover_target=None）+ committed の場合、
        // グローバル座標で `compute_drop_target` を呼び、判定結果に応じて分岐する:
        // - `SameWindow`: ペイン領域にドロップ → 何もしない（既存挙動）
        // - `OtherWindowTabBar`: 別 OS Window のタブバーにドロップ → Phase 4-4 で `MovePaneToWindow`
        //   送信実装予定、現状はログ出力のみ
        // - `NewWindow`: どの OS Window 外にドロップ → `spawn_os_window` 呼び出し
        //   （Phase 4-2 時点では本体未実装のスケルトン、ログ出力 + 主 Window フォールバック）
        if let Some(drag) = self.app.state.tab_drag.take() {
            if drag.committed
                && let Some(target_id) = drag.hover_target
                && target_id != drag.pane_id
                && let Some(new_order) =
                    compute_reordered_tab_order(&self.app.state.tab_order, drag.pane_id, target_id)
                && let Some(conn) = &self.connection
            {
                let _ = conn.send_tx.try_send(ClientToServer::ReorderPanes {
                    pane_ids: new_order,
                });
            } else if drag.committed && drag.hover_target.is_none() {
                // タブバー外でリリース → タブ外ドロップ判定（Phase 4-2）
                self.handle_tab_drag_drop_outside(&drag);
            }
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

#[cfg(test)]
mod tests {
    use super::compute_reordered_tab_order;

    #[test]
    fn tab_drag_自分自身へのドロップはnone() {
        let current = vec![1, 2, 3];
        assert!(compute_reordered_tab_order(&current, 2, 2).is_none());
    }

    #[test]
    fn tab_drag_右へ移動() {
        // 1, 2, 3 で 1 を 3 の位置にドロップ → 2, 3 の右に 1 が来るはず
        // ただし現実装は「target_id の位置に置き換える」挙動なので、
        // 1 を 3 にドロップ → [2, 1, 3]（target_id=3 の位置に 1 を挿入）
        let current = vec![1, 2, 3];
        let next = compute_reordered_tab_order(&current, 1, 3).unwrap();
        assert_eq!(next, vec![2, 1, 3]);
    }

    #[test]
    fn tab_drag_左へ移動() {
        // 1, 2, 3 で 3 を 1 の位置にドロップ → [3, 1, 2]
        let current = vec![1, 2, 3];
        let next = compute_reordered_tab_order(&current, 3, 1).unwrap();
        assert_eq!(next, vec![3, 1, 2]);
    }

    #[test]
    fn tab_drag_隣接右ドロップはno_op() {
        // [1, 2] で 1 を 2 にドロップしても「target の位置に押し込む」挙動では
        // 結果が [1, 2] となり current と一致。ネットワーク往復を避けるため None。
        let current = vec![1, 2];
        assert!(compute_reordered_tab_order(&current, 1, 2).is_none());
    }

    #[test]
    fn tab_drag_隣接左ドロップはスワップ() {
        // [1, 2] で 2 を 1 にドロップ: from=1, target_pos=0, from>target_pos
        // → insert_at=0, new=[1] → [2, 1]。隣接スワップは左ドラッグ方向でのみ実現可能。
        let current = vec![1, 2];
        let next = compute_reordered_tab_order(&current, 2, 1).unwrap();
        assert_eq!(next, vec![2, 1]);
    }

    #[test]
    fn tab_drag_存在しないidはnone() {
        let current = vec![1, 2, 3];
        assert!(compute_reordered_tab_order(&current, 99, 1).is_none());
        assert!(compute_reordered_tab_order(&current, 1, 99).is_none());
    }

    #[test]
    fn tab_drag_3つの中央への移動() {
        // 1, 2, 3, 4, 5 で 1 を 3 にドロップ → [2, 1, 3, 4, 5]
        let current = vec![1, 2, 3, 4, 5];
        let next = compute_reordered_tab_order(&current, 1, 3).unwrap();
        assert_eq!(next, vec![2, 1, 3, 4, 5]);

        // 5 を 2 にドロップ → [1, 5, 2, 3, 4]
        let next = compute_reordered_tab_order(&current, 5, 2).unwrap();
        assert_eq!(next, vec![1, 5, 2, 3, 4]);
    }
}
