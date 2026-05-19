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
    NodeIdKind, build_tree_from_state, compute_tree_state_hash, decode_node_id,
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
    /// 1. 前回更新から `TREE_UPDATE_THROTTLE` (100ms) 未満なら早期 return（スロットリング）
    /// 2. `compute_tree_state_hash(&self.app.state)` で現在の状態フィンガープリントを計算
    /// 3. 前回ハッシュと一致なら早期 return（状態未変化）
    /// 4. 変化あり: 主 Window + 全追加 Window の各 Adapter に `update_if_active(|| tree)` を呼ぶ
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
        if let Some(last) = self.last_tree_update_at
            && now.duration_since(last) < TREE_UPDATE_THROTTLE
        {
            return;
        }
        self.last_tree_update_at = Some(now);

        let current_hash = compute_tree_state_hash(&self.app.state);
        if self.last_tree_hash == Some(current_hash) {
            return; // 状態変化なし
        }
        self.last_tree_hash = Some(current_hash);

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
}
