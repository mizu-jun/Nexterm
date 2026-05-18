//! winit イベントハンドラ
//!
//! Sprint 5-6 で旧 `event_handler.rs`（1,318 行）を 7 サブモジュールに分割：
//! - `consent` — Sprint 4-1 機密操作の同意フロー
//! - `settings_panel_hit` — 設定パネルのマウスヒットテスト
//! - `lifecycle` — `new_events` / `resumed` / `about_to_wait`
//! - `window` — ウィンドウ・IME・再描画イベント
//! - `mouse` — カーソル移動・クリック・ホイール
//! - `keyboard` — キー入力
//!
//! 各サブモジュールは `impl EventHandler` ブロックを介して機能を追加する。
//! 本ファイルは `EventHandler` 構造体と `ApplicationHandler` トレイト実装（dispatch のみ）を保持する。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use nexterm_config::{Config, StatusBarEvaluator};
use tracing::warn;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalPosition,
    event::{ElementState, MouseButton, StartCause, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::ModifiersState,
    window::{Window, WindowId},
};

use crate::connection::Connection;
use crate::glyph_atlas::GlyphAtlas;

use super::{ClientWindow, NextermApp, WgpuState};

// ---- サブモジュール ----
mod consent;
mod keyboard;
mod lifecycle;
mod mouse;
mod settings_panel_hit;
mod window;

/// winit のイベントハンドラ
pub struct EventHandler {
    pub(super) app: NextermApp,
    pub(super) wgpu_state: Option<WgpuState>,
    pub(super) atlas: Option<GlyphAtlas>,
    pub(super) window: Option<Arc<Window>>,
    pub(super) modifiers: ModifiersState,
    /// サーバーとの IPC 接続
    pub(super) connection: Option<Connection>,
    /// マウスカーソル位置（ピクセル）
    pub(super) cursor_position: Option<(f64, f64)>,
    /// 設定ホットリロード受信チャネル
    pub(super) config_rx: Option<tokio::sync::mpsc::Receiver<Config>>,
    /// ファイル監視ウォッチャー（Drop されると停止するため保持する）
    pub(super) _config_watcher: Option<notify::RecommendedWatcher>,
    /// Lua ステータスバー評価器
    pub(super) status_eval: Option<StatusBarEvaluator>,
    /// ステータスバーの最終評価時刻
    pub(super) last_status_eval: Instant,
    /// ディスプレイの DPI スケール係数（winit より取得）
    pub(super) scale_factor: f32,
    /// シェーダーファイル変更通知チャネル（Some = カスタムシェーダー監視中）
    pub(super) shader_reload_rx: Option<tokio::sync::mpsc::Receiver<()>>,
    /// シェーダーファイル監視ウォッチャー
    pub(super) _shader_watcher: Option<notify::RecommendedWatcher>,
    /// タブのダブルクリック検出用（最終クリック時刻とペイン ID）
    pub(super) last_tab_click: Option<(Instant, u32)>,
    /// 内部サーバータスクのハンドル（ウィンドウ終了時に abort する）
    pub(super) server_handle: tokio::task::JoinHandle<()>,
    /// タッチパッド精密スクロール（PixelDelta）の積算バッファ
    pub(super) pixel_scroll_accumulator: f64,
    /// 更新チェッカーからの通知受信チャネル（Some(version) = 新バージョンあり）
    pub(super) update_rx: tokio::sync::watch::Receiver<Option<String>>,
    /// Quake モード ランタイム（Sprint 5-7 / Phase 2-2）。
    /// global-hotkey マネージャを drop しないように保持する。
    /// `pending_quake_action` (state) と組み合わせてホットキー押下 / IPC 経由の
    /// トグル要求を一元処理する。
    pub(super) quake: crate::quake::QuakeRuntime,
    /// 複数 OS Window 対応用 HashMap（Sprint 5-8 Phase 4-1 Step 1.2 スケルトン）。
    ///
    /// 現状は空のまま保持。Step 1.3 以降で `on_resumed` フローを移行し、
    /// `windows` に登録した `ClientWindow` を一次データソースとする。
    /// 移行完了までは既存の `window` / `wgpu_state` / `atlas` フィールドが
    /// 一次データソース。
    #[allow(dead_code)]
    pub(super) windows: HashMap<WindowId, ClientWindow>,
}

impl EventHandler {
    /// 新規 OS Window を生成し、`windows` HashMap に登録する。
    ///
    /// Sprint 5-8 Phase 4-1 Step 1.5 **スケルトン実装**: 現状は実 winit Window 生成
    /// および wgpu 初期化を行わず、主 Window の `WindowId` を返すだけ（Phase 4-2 の
    /// 「タブ外ドロップで新規 OS Window 生成」フロー実装時に本実装する）。
    ///
    /// 引数:
    /// - `event_loop`: winit `ActiveEventLoop`（Window 生成に使用、Phase 4-2 で活用）
    /// - `pos`: 新規 Window のスクリーン位置（タブ外ドロップ座標）
    /// - `server_window_id`: 新規 Window にアタッチするサーバー Window ID
    ///
    /// 戻り値: 生成された Window の `WindowId`。主 Window が未初期化の場合は `None`。
    ///
    /// Phase 4-2 で実装する内容:
    /// 1. `event_loop.create_window(...)` で新規 winit Window を生成
    /// 2. `WgpuState::new` で wgpu パイプラインを初期化
    /// 3. `PerWindowViewState { focused_server_window_id: server_window_id, .. Default::default() }`
    /// 4. `self.windows.insert(window_id, ClientWindow { window, wgpu, view_state })`
    /// 5. サーバーへ `Attach { window_id: server_window_id }` を送信
    #[allow(dead_code)]
    pub(super) fn spawn_os_window(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _pos: PhysicalPosition<i32>,
        _server_window_id: u32,
    ) -> Option<WindowId> {
        warn!("spawn_os_window: Phase 4-2 で本実装予定。現状は主 Window の WindowId を返します");
        self.window.as_ref().map(|w| w.id())
    }

    /// 指定 `WindowId` の OS Window を破棄する。
    ///
    /// Sprint 5-8 Phase 4-1 Step 1.5 **スケルトン実装**: 現状は `windows` HashMap から
    /// エントリを削除し、すべての OS Window が閉じられた場合に既存の exit ロジックを
    /// 実行する。実 winit Window の `request_close` は Phase 4-2 で本実装する。
    ///
    /// 引数:
    /// - `event_loop`: 最後の Window 閉鎖時の `exit()` 呼び出しに使用
    /// - `window_id`: 破棄する Window の ID
    ///
    /// 終了判定:
    /// - `windows` が空 **かつ** 主 Window が閉じられた（または未初期化） → exit
    /// - それ以外 → HashMap から削除のみで継続
    ///
    /// Phase 4-2 で実装する内容:
    /// 1. `ClientWindow` を `windows` から取り出し、`window.set_visible(false)` 等のクリーンアップ
    /// 2. 関連リソース（wgpu surface, atlas）を drop
    /// 3. サーバーへ `Detach { window_id }` を送信
    /// 4. 終了判定で `on_close_requested` の `close_action` ロジックを再利用
    #[allow(dead_code)]
    pub(super) fn close_os_window(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId) {
        self.windows.remove(&window_id);

        // 主 Window が閉じられたかを判定（現状は主 Window のみが実利用される）
        let main_closed = self
            .window
            .as_ref()
            .map(|w| w.id() == window_id)
            .unwrap_or(true);

        // 全 OS Window が閉じられたら exit
        if self.windows.is_empty() && main_closed {
            // `on_close_requested` と同じ exit ロジック（close_action 分岐は Phase 4-3 で統合）
            self.connection = None;
            self.server_handle.abort();
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for EventHandler {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.on_new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.on_resumed(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.on_about_to_wait(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.on_close_requested(event_loop);
            }
            WindowEvent::Resized(size) => {
                self.on_resized(size);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.on_scale_factor_changed(scale_factor);
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.on_modifiers_changed(mods.state());
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.on_cursor_moved(position);
            }
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state: ElementState::Pressed,
                ..
            } => {
                self.on_mouse_right_pressed();
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } => {
                self.on_mouse_left_pressed();
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } => {
                self.on_mouse_left_released();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.on_mouse_wheel(delta);
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed => {
                self.on_keyboard_input(key_event, event_loop);
            }
            WindowEvent::Ime(ime_event) => {
                self.on_ime(ime_event);
            }
            WindowEvent::RedrawRequested => {
                self.on_redraw_requested();
            }
            _ => {}
        }

        // 毎フレーム再描画をリクエストする
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
