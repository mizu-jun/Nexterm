//! Sprint 5-11-1 / H1 PoC: AccessKit イベントハンドラ
//!
//! `UserEvent::Accessibility(accesskit_winit::Event)` を受け取って
//! 適切な応答（初期ツリー送出 / アクション処理 / 非アクティブ化）を行う。
//!
//! Phase 5-11-1 PoC スコープ:
//! - `InitialTreeRequested`: スクリーンリーダー接続時に固定ツリーを返す
//! - `ActionRequested`: ログのみ（実アクションは Phase 5-11-2 以降）
//! - `AccessibilityDeactivated`: ログのみ（リソース解放は adapter 側で完結）
//!
//! Sprint 5-11-2 Step 2-5 で `update_accesskit_tree_if_needed` を追加。
//! `on_about_to_wait` 末尾で呼び出してライブ更新を行う。

use std::time::{Duration, Instant};

use accesskit::{Action, ActionData, ActionRequest};
use nexterm_proto::ClientToServer;
use tracing::{debug, info};
use winit::event_loop::ActiveEventLoop;

use crate::accessibility::{
    NodeIdKind, build_tree_from_state, compute_grid_row_hashes, compute_tree_state_hash,
    decode_node_id, dispatch_settings_action,
};

use super::EventHandler;

/// AccessKit ライブ更新のスロットリング間隔（Q3=a で合意した 100ms）。
///
/// この間隔を空けて `compute_tree_state_hash` と `update_if_active` を実行する。
/// スクリーンリーダー側の認識遅延は最大 100ms 程度に収まる。
const TREE_UPDATE_THROTTLE: Duration = Duration::from_millis(100);

impl EventHandler {
    /// AccessKit プラットフォームアダプタから届いたイベントを処理する。
    ///
    /// Sprint 5-11-2 Step 2-1: 固定ツリーから動的ツリー（`ClientState` 反映）に移行。
    /// Sprint 5-11-2 Step 2-3: 複数 OS Window 対応。`event.window_id` から該当 Adapter を引く。
    ///
    /// **設計メモ**: ツリー内容は `ClientState` 単一インスタンスから生成するため、すべての
    /// OS Window で同じツリーが返る。将来的に「Window ごとに別 view」を導入する場合は
    /// `PerWindowViewState` を参照して各 Adapter に異なるツリーを送る形に拡張する。
    pub(super) fn on_accesskit_event(
        &mut self,
        event: accesskit_winit::Event,
        event_loop: &ActiveEventLoop,
    ) {
        // 先にツリーを計算（adapter mut borrow と state ref borrow を分離する）
        let tree_update_for_initial = matches!(
            event.window_event,
            accesskit_winit::WindowEvent::InitialTreeRequested
        )
        .then(|| build_tree_from_state(&self.app.state));

        // 対象 Adapter を window_id で引く。主 Window と追加 Window を区別する。
        let event_window_id = event.window_id;
        let is_main = self.window.as_ref().map(|w| w.id()) == Some(event_window_id);

        match event.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => {
                info!(
                    "AccessKit: スクリーンリーダーが接続、初期ツリーを送出する (window_id={:?})",
                    event_window_id
                );
                let tree_update =
                    tree_update_for_initial.expect("InitialTreeRequested arm では事前計算済み");
                if is_main {
                    if let Some(adapter) = self.accesskit_adapter.as_mut() {
                        adapter.update_if_active(|| tree_update);
                    }
                } else if let Some(cw) = self.windows.get_mut(&event_window_id) {
                    cw.accesskit_adapter.update_if_active(|| tree_update);
                }
            }
            accesskit_winit::WindowEvent::ActionRequested(request) => {
                self.handle_accesskit_action(request, event_loop);
            }
            accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                info!(
                    "AccessKit: スクリーンリーダーが切断された (window_id={:?})",
                    event_window_id
                );
            }
        }
    }

    /// AccessKit `ActionRequest` を Nexterm 内部操作にマップして実行する（Step 2-4）。
    ///
    /// **ディスパッチ表**:
    ///
    /// | target_node | Action | 効果 |
    /// |---|---|---|
    /// | `Tab { pane_id }` | `Focus` / `Click` | `FocusPane` IPC 送信 + `state.focused_pane_id` 更新 |
    /// | `Pane { pane_id }` | `Focus` / `Click` | 同上 |
    /// | `CloseDialogKill` | `Click` / `Focus` | `selected_button = 0xFE`（Kill 確定） |
    /// | `CloseDialogCancel` | `Click` / `Focus` | `selected_button = 0xFF`（Cancel 確定） |
    /// | `ContextItem { idx }` | `Click` | 既存 `execute_context_menu_action` 流用 + メニューを閉じる |
    /// | `PaletteItem { idx }` | `Click` | 既存 `execute_action` 流用 + パレットを閉じる |
    /// | `PaletteSearch` | `SetValue(s)` | `palette.query = s` + selection リセット |
    /// | `QuickSelectItem { idx }` | `Click` | `matches[idx].text` をクリップボードコピー + `quick_select.exit()` |
    /// | `SettingsTab { idx }` | `Focus` / `Click` | カテゴリ切替 + `font_family_editing = false` |
    /// | `SettingsFontFamily` | `Click` | 編集モード ON |
    /// | `SettingsFontFamily` | `SetValue(s)` | `font_family = s`, `dirty = true` |
    /// | `SettingsFontSize` | `SetValue(v)` | 0.5 単位丸め + clamp 8.0〜32.0 |
    /// | `SettingsFontSize` | `Increment` / `Decrement` | `increase_font_size` / `decrease_font_size` |
    /// | `SettingsThemeScheme` | `Click` / `Increment` | `next_scheme` |
    /// | `SettingsThemeScheme` | `Decrement` | `prev_scheme` |
    /// | `SettingsWindowOpacity` | `SetValue(v)` | 0.05 単位丸め + clamp 0.1〜1.0 |
    /// | `SettingsWindowOpacity` | `Increment` / `Decrement` | `increase_opacity` / `decrease_opacity` |
    /// | `SettingsStartupLanguage` | `Click` / `Increment` | `next_language` |
    /// | `SettingsStartupLanguage` | `Decrement` | `prev_language` |
    /// | `SettingsStartupAutoUpdate` | `Click` | トグル + `dirty = true` |
    /// | その他 | — | `debug!` ログのみ |
    ///
    /// **設計メモ**:
    /// - `Focus` を `Click` と同等扱いにする理由: スクリーンリーダー（NVDA / VoiceOver / Orca）
    ///   は仮想カーソルでフォーカスを移動するだけで実際の制御が遷移する。Nexterm でも同じ UX
    ///   を実現するため、Focus アクションでも `FocusPane` IPC を送る。
    /// - `selected_button` の値: `window.rs::poll_pending_close_request` が `0xFE` を Kill、
    ///   `0xFF` を Cancel として消費する（既存の半オープン契約をそのまま利用）。
    /// - パレットの `idx` は `filtered()` 上の位置。動的ツリー側で同じ順序で展開しているため
    ///   その idx の `PaletteAction.action` 文字列を取って `execute_action` に渡せばよい。
    /// - ContextMenu の `idx` は `items` 上の位置で、こちらは生のインデックス。
    fn handle_accesskit_action(&mut self, request: ActionRequest, event_loop: &ActiveEventLoop) {
        let kind = decode_node_id(request.target_node);
        debug!(
            "AccessKit: アクション受信 action={:?}, target={:?} ({:?})",
            request.action, request.target_node, kind
        );

        // 設定パネル系のアクションは純関数 `dispatch_settings_action` に委譲する。
        // 該当した場合は再描画を要求して早期 return。
        if matches!(
            kind,
            NodeIdKind::SettingsTab { .. }
                | NodeIdKind::SettingsFontFamily
                | NodeIdKind::SettingsFontSize
                | NodeIdKind::SettingsThemeScheme
                | NodeIdKind::SettingsWindowOpacity
                | NodeIdKind::SettingsStartupLanguage
                | NodeIdKind::SettingsStartupAutoUpdate
        ) {
            let handled = dispatch_settings_action(
                &mut self.app.state.settings_panel,
                request.action,
                &kind,
                request.data,
            );
            if handled {
                info!(
                    "AccessKit: 設定パネル アクション処理 action={:?}, kind={:?}",
                    request.action, kind
                );
                self.request_redraw_if_window();
            } else {
                debug!(
                    "AccessKit: 設定パネル 未対応 action={:?}, kind={:?}",
                    request.action, kind
                );
            }
            return;
        }

        match (request.action, kind) {
            // ===== タブ / ペインのフォーカス・クリック =====
            (Action::Focus | Action::Click, NodeIdKind::Tab { pane_id })
            | (Action::Focus | Action::Click, NodeIdKind::Pane { pane_id }) => {
                info!("AccessKit: pane_id={} にフォーカス要求", pane_id);
                self.app.state.focused_pane_id = Some(pane_id);
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::FocusPane { pane_id });
                }
                self.request_redraw_if_window();
            }

            // ===== 閉じる確認ダイアログ =====
            (Action::Click | Action::Focus, NodeIdKind::CloseDialogKill) => {
                info!("AccessKit: CloseDialog Kill ボタン確定");
                if let Some(dlg) = self.app.state.close_window_dialog.as_mut() {
                    // 0xFE = Kill 確定（次フレームの poll_pending_close_request が消費）
                    dlg.selected_button = if matches!(request.action, Action::Click) {
                        0xFE
                    } else {
                        0
                    };
                    self.request_redraw_if_window();
                }
            }
            (Action::Click | Action::Focus, NodeIdKind::CloseDialogCancel) => {
                info!("AccessKit: CloseDialog Cancel ボタン確定");
                if let Some(dlg) = self.app.state.close_window_dialog.as_mut() {
                    dlg.selected_button = if matches!(request.action, Action::Click) {
                        0xFF
                    } else {
                        1
                    };
                    self.request_redraw_if_window();
                }
            }

            // ===== コンテキストメニュー =====
            (Action::Click, NodeIdKind::ContextItem { idx }) => {
                let action = self
                    .app
                    .state
                    .context_menu
                    .as_ref()
                    .and_then(|m| m.items.get(idx))
                    .map(|item| item.action.clone());
                if let Some(action) = action {
                    info!("AccessKit: ContextMenu 項目 {} を実行: {:?}", idx, action);
                    // メニューを閉じてからアクション実行（既存マウスクリック経路と同じ順序）
                    self.app.state.context_menu = None;
                    self.execute_context_menu_action(&action);
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: ContextMenu 項目 idx={} が範囲外（メニューが既に閉じている可能性）",
                        idx
                    );
                }
            }
            (Action::Focus, NodeIdKind::ContextItem { idx }) => {
                if let Some(menu) = self.app.state.context_menu.as_mut()
                    && idx < menu.items.len()
                {
                    menu.hovered = Some(idx);
                    self.request_redraw_if_window();
                }
            }

            // ===== コマンドパレット =====
            (Action::Click, NodeIdKind::PaletteItem { idx }) => {
                let action_id = self
                    .app
                    .state
                    .palette
                    .filtered()
                    .get(idx)
                    .map(|a| a.action.clone());
                if let Some(action_id) = action_id {
                    info!("AccessKit: Palette 項目 {} を実行: {}", idx, action_id);
                    // 既存 Enter キー経路と同じ順序: 閉じる → 履歴記録 → アクション実行
                    self.app.state.palette.close();
                    self.app.state.palette.record_use(&action_id);
                    self.execute_action(&action_id, event_loop);
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Palette 項目 idx={} が範囲外（query 変更等で消失した可能性）",
                        idx
                    );
                }
            }
            (Action::Focus, NodeIdKind::PaletteItem { idx }) => {
                if self.app.state.palette.is_open && idx < self.app.state.palette.filtered().len() {
                    self.app.state.palette.selected = idx;
                    self.request_redraw_if_window();
                }
            }
            (Action::SetValue, NodeIdKind::PaletteSearch) => {
                if let Some(ActionData::Value(s)) = request.data {
                    info!("AccessKit: Palette 検索文字列を設定: {:?}", s.as_ref());
                    self.app.state.palette.query = s.into_string();
                    self.app.state.palette.selected = 0;
                    self.request_redraw_if_window();
                }
            }

            // ===== Quick Select（Step 2-2-h）=====
            //
            // SR からの Click は「ラベルキー入力でマッチが確定したとき」と同じ挙動にする
            // （既存の `handle_quick_select_key` の `accept` 経路を踏襲）。
            // Focus は描画状態を変えるだけの非破壊操作なので debug ログだけにしておく。
            (Action::Click, NodeIdKind::QuickSelectItem { idx }) => {
                let text = self
                    .app
                    .state
                    .quick_select
                    .matches
                    .get(idx)
                    .map(|m| m.text.clone());
                if let Some(text) = text {
                    info!("AccessKit: Quick Select 項目 {} を確定: {}", idx, text);
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                    self.app.state.quick_select.exit();
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Quick Select 項目 idx={} が範囲外（既に exit() 済の可能性）",
                        idx
                    );
                }
            }

            // ===== Host Manager（Phase 5-11-6 #2）=====
            //
            // SR からの Click は既存の Enter キー経路（`input_handler/mod.rs` の
            // `host_manager.is_open` 分岐）と同等の挙動:
            // - `auth_type == "password"` → `PasswordModal` を開く（パスワード入力自体の
            //   SR 対応は Phase 5-11-7 候補）
            // - その他 → `record_connection` + `connect_ssh_host_new_tab`
            // Focus は `host_manager.selected` を更新するだけの非破壊操作。
            (Action::Click, NodeIdKind::HostItem { idx }) => {
                let host = self
                    .app
                    .state
                    .host_manager
                    .filtered()
                    .get(idx)
                    .map(|h| (*h).clone());
                if let Some(host) = host {
                    info!("AccessKit: Host 項目 {} を確定: {}", idx, host.name);
                    self.app.state.host_manager.close();
                    if host.auth_type == "password" {
                        self.app.state.host_manager.password_modal =
                            Some(crate::host_manager::PasswordModal::new(host));
                    } else {
                        self.app.state.host_manager.record_connection(&host);
                        self.connect_ssh_host_new_tab(&host);
                    }
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Host 項目 idx={} が範囲外（host_manager は閉じている可能性）",
                        idx
                    );
                }
            }
            (Action::Focus, NodeIdKind::HostItem { idx }) => {
                if self.app.state.host_manager.is_open
                    && idx < self.app.state.host_manager.filtered().len()
                {
                    self.app.state.host_manager.selected = idx;
                    self.request_redraw_if_window();
                }
            }

            // ===== Macro Picker（Phase 5-11-6 #3）=====
            //
            // SR からの Click は既存の Enter キー経路（`input_handler/mod.rs` の
            // `macro_picker.is_open` 分岐）と同等の挙動: `selected = idx` →
            // `selected_macro()` で MacroConfig 取得 → `close()` → IPC `RunMacro` 送信。
            // Focus は `macro_picker.selected` を更新するだけ。
            (Action::Click, NodeIdKind::MacroItem { idx }) => {
                self.app.state.macro_picker.selected = idx;
                let mac = self
                    .app
                    .state
                    .macro_picker
                    .selected_macro()
                    .map(|m| (m.lua_fn.clone(), m.name.clone()));
                if let Some((fn_name, display_name)) = mac {
                    info!("AccessKit: Macro 項目 {} を実行: {}", idx, display_name);
                    self.app.state.macro_picker.close();
                    if let Some(conn) = &self.connection {
                        let _ = conn.send_tx.try_send(ClientToServer::RunMacro {
                            macro_fn: fn_name,
                            display_name,
                        });
                    }
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Macro 項目 idx={} が範囲外（macro_picker は閉じている可能性）",
                        idx
                    );
                }
            }
            (Action::Focus, NodeIdKind::MacroItem { idx }) => {
                if self.app.state.macro_picker.is_open {
                    self.app.state.macro_picker.selected = idx;
                    self.request_redraw_if_window();
                }
            }

            // ===== Alert Dismiss（Phase 5-11-6 #4）=====
            //
            // SR からの Click でアラートを TTL（5 秒）を待たず即時 dismiss する。
            // `Action::Default` は accesskit 0.24 に存在しないため `Click` のみで対応。
            (Action::Click, NodeIdKind::Alert { seq }) => {
                if self.app.state.dismiss_alert(seq) {
                    info!("AccessKit: Alert seq={} を即時 dismiss", seq);
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Alert seq={} が見つからない（既に TTL 切れの可能性）",
                        seq
                    );
                }
            }

            // ===== Scroll（Phase 5-11-6 #5）=====
            //
            // PaneArea に対する SR の Scroll 要求 → `state.scroll_up/down(rows/2)` を呼ぶ。
            // 既存の PageUp/PageDown キー経路と同じ半画面単位。
            //
            // 設計（state API の方向に揃える）:
            // - `Action::ScrollUp` = 過去側を見せる = `state.scroll_up`（offset を増やす）
            // - `Action::ScrollDown` = 最新側に戻る = `state.scroll_down`（offset を減らす）
            (Action::ScrollUp, NodeIdKind::PaneArea) => {
                let lines = (self.app.state.rows as usize / 2).max(1);
                self.app.state.scroll_up(lines);
                self.request_redraw_if_window();
            }
            (Action::ScrollDown, NodeIdKind::PaneArea) => {
                let lines = (self.app.state.rows as usize / 2).max(1);
                self.app.state.scroll_down(lines);
                self.request_redraw_if_window();
            }

            // ===== その他 =====
            (action, kind) => {
                debug!(
                    "AccessKit: 未対応の (action, target) 組合せ: action={:?}, kind={:?}",
                    action, kind
                );
            }
        }
    }

    /// 主 Window のみ再描画要求を送る補助関数。
    /// 追加 OS Window の再描画は現状 `request_redraw_if_window` の対象外（ツリーは
    /// `update_accesskit_tree_if_needed` 経由で次フレームに反映される）。
    fn request_redraw_if_window(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Sprint 5-11-2 Step 2-5: AccessKit ツリーをライブ更新する。
    ///
    /// **呼び出し位置**: `on_about_to_wait` の末尾。サーバーメッセージ・設定リロード・
    /// ホットキー処理など毎フレームの状態変化が反映された後に呼ぶ。
    ///
    /// **更新戦略**:
    /// 1. **Sprint 5-11-5**: アラート TTL (5 秒) 切れエントリを `expire_alerts(now)` で除去
    ///    （スロットリング前に実行: 期限切れ即時除去で SR ツリーを正確に保つ）
    /// 2. 前回更新から `TREE_UPDATE_THROTTLE` (100ms) 未満なら早期 return（スロットリング）
    /// 3. `compute_tree_state_hash(&self.app.state)` で現在の状態フィンガープリントを計算
    /// 4. 前回ハッシュと一致なら早期 return（状態未変化）
    /// 5. 変化あり: 主 Window + 全追加 Window の各 Adapter に `update_if_active(|| tree)` を呼ぶ
    ///    （adapter が非アクティブなら no-op なので毎回安全に呼べる）
    ///
    /// **注意**: ツリー本体は Adapter ごとに別々の `TreeUpdate` を build する。
    /// `TreeUpdate` は `Clone` 不能だが、`build_tree_from_state` は十分軽量（O(N)、典型 ~50µs）
    /// なので複数 Window でも問題ない。
    ///
    /// **設計の根拠**: Q3=a (100ms スロットル) + 設計 (a) ハッシュベース。
    /// 案 (b) 「各イベントで明示的に呼ぶ」は触る箇所が分散するため不採用。
    pub(super) fn update_accesskit_tree_if_needed(&mut self) {
        let now = Instant::now();

        // Sprint 5-11-5: 期限切れアラートを除去（毎フレーム実行、軽量）
        self.app.state.expire_alerts(now);

        if let Some(last) = self.last_tree_update_at
            && now.duration_since(last) < TREE_UPDATE_THROTTLE
        {
            return;
        }
        self.last_tree_update_at = Some(now);

        // 構造変化（タブ・ペイン・オーバーレイ・アラート）の検知
        let current_hash = compute_tree_state_hash(&self.app.state);
        let tree_changed = self.last_tree_hash != Some(current_hash);
        self.last_tree_hash = Some(current_hash);

        // Sprint 5-11-3: ターミナル本文（grid 行内容）の差分検知。
        // 構造変化がないターミナル出力（cargo build / log streaming 等）でも
        // フォーカスペインに `Live::Polite` を設定したノードを送り直すことで SR にアナウンスさせる。
        let grid_changed = self.detect_grid_row_changes();

        if !tree_changed && !grid_changed {
            return; // 構造・内容ともに変化なし
        }

        // 主 Window 用 Adapter
        if let Some(adapter) = self.accesskit_adapter.as_mut() {
            let update = build_tree_from_state(&self.app.state);
            adapter.update_if_active(|| update);
        }
        // 追加 OS Window 用 Adapter（現状は全 Window 同一ツリー）
        for cw in self.windows.values_mut() {
            let update = build_tree_from_state(&self.app.state);
            cw.accesskit_adapter.update_if_active(|| update);
        }
    }

    /// Sprint 5-11-3: 各ペインのグリッド行ハッシュを再計算し、変化を検知する。
    ///
    /// 戻り値: いずれかのペインの行ハッシュ列に変化があれば `true`。
    /// 副作用として `last_grid_row_hashes` を最新値に置き換える。
    ///
    /// ペイン削除・追加（HashMap キーの増減）も `true` として返す（構造変化は通常
    /// `compute_tree_state_hash` が拾うが、このフィールドの整合性を保つために二重判定する）。
    fn detect_grid_row_changes(&mut self) -> bool {
        use std::collections::HashMap;

        let panes = &self.app.state.panes;
        let mut new_hashes: HashMap<u32, Vec<u64>> = HashMap::with_capacity(panes.len());
        let mut changed = false;

        for (&pane_id, pane) in panes {
            let hashes = compute_grid_row_hashes(&pane.grid);
            if self.last_grid_row_hashes.get(&pane_id) != Some(&hashes) {
                changed = true;
            }
            new_hashes.insert(pane_id, hashes);
        }

        // ペイン削除も差分として検知（new_hashes の長さが減った場合）
        if new_hashes.len() != self.last_grid_row_hashes.len() {
            changed = true;
        }

        self.last_grid_row_hashes = new_hashes;
        changed
    }
}
