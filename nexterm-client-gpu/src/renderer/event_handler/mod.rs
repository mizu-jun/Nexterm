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

use std::sync::Arc;
use std::time::Instant;

use nexterm_config::{Config, StatusBarEvaluator};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, StartCause, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::ModifiersState,
    window::{Window, WindowId},
};

use crate::connection::Connection;
use crate::glyph_atlas::GlyphAtlas;

use super::{NextermApp, WgpuState};

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
