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
use tracing::{info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};

use crate::connection::Connection;
use crate::glyph_atlas::GlyphAtlas;

use super::{ClientWindow, NextermApp, PerWindowViewState, WgpuState};

/// EventLoop に送る非同期ユーザーイベント（Sprint 5-8 Phase 4-4 / Sprint 5-11-1）。
///
/// マウスハンドラやネットワーク受信スレッドから `&ActiveEventLoop` を持たない
/// コンテキストで「OS Window をスポーン/クローズしたい」要求を出すために使う。
/// `EventLoopProxy::send_event(...)` で発火すると、次回 winit イベントループが
/// `user_event` ハンドラを呼び、`&ActiveEventLoop` 付きの安全な文脈で処理できる。
///
/// Sprint 5-11-1 で `Accessibility` バリアントを追加。`accesskit_winit::Adapter::new`
/// は `EventLoopProxy<T: From<accesskit_winit::Event>>` を要求するため、`From` impl
/// を提供する（後述）。`InitialTreeRequested` / `ActionRequested` / `AccessibilityDeactivated`
/// の 3 種が `accesskit_winit::WindowEvent` に格納されて届く。
///
/// `Clone` は派生しない（`accesskit_winit::Event` がクローン不能で、UserEvent のクローン需要も無い）。
#[derive(Debug)]
pub enum UserEvent {
    /// 新規 OS Window を生成し、サーバー Window ID に紐付ける。
    ///
    /// - `server_window_id`: アタッチするサーバー側 Window ID
    /// - `pos`: 新規 Window の希望位置（`None` なら winit のデフォルト配置）
    SpawnOsWindow {
        server_window_id: u32,
        pos: Option<PhysicalPosition<i32>>,
    },
    /// 指定 OS Window を閉じる。最後の 1 個ならアプリ全体を exit する。
    ///
    /// Phase 4-4 時点では現状 `on_close_requested` で全 OS Window を一括破棄するため
    /// 直接の発火経路がない（dead_code 警告抑制）。Phase 4-5 でコンテキストメニュー
    /// 「この Window だけ閉じる」アクションから発火する予定。
    #[allow(dead_code)]
    CloseOsWindow { window_id: WindowId },
    /// Sprint 5-11-1 / H1 PoC: AccessKit プラットフォームアダプタからのイベント。
    ///
    /// スクリーンリーダーが接続したとき（`InitialTreeRequested`）や、ユーザーが
    /// スクリーンリーダー側で操作したとき（`ActionRequested`）、または接続が
    /// 切れたとき（`AccessibilityDeactivated`）に届く。
    Accessibility(accesskit_winit::Event),
}

/// `accesskit_winit::Adapter::new` の型境界 `T: From<accesskit_winit::Event>` を満たすための impl。
/// Sprint 5-11-1 で追加。
impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        UserEvent::Accessibility(event)
    }
}

// ---- サブモジュール ----
mod accessibility;
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
    pub(super) windows: HashMap<WindowId, ClientWindow>,
    /// EventLoopProxy<UserEvent>（Sprint 5-8 Phase 4-4）。
    /// マウスハンドラやネットワークスレッドから `UserEvent::SpawnOsWindow` 等を発火する。
    pub(super) proxy: EventLoopProxy<UserEvent>,
    /// `WindowListChanged` 受信時に「新規 OS Window スポーン要求」を判定するための既知 Window ID 集合
    /// （Sprint 5-8 Phase 4-4 Step C）。サーバーから通知された Window 集合との差分を取り、
    /// クライアントが知らない Window があれば `pending_new_window_drop_pos` の位置に OS Window を
    /// スポーンする。
    pub(super) known_server_window_ids: std::collections::HashSet<u32>,
    /// タブ外ドロップで新規 OS Window 生成を要求した際のドロップ位置（Sprint 5-8 Phase 4-4）。
    /// `WindowListChanged` で新しい Window ID を検出したとき、その位置に OS Window をスポーンする。
    /// 一度消費したら `None` に戻す。
    pub(super) pending_new_window_drop_pos: Option<PhysicalPosition<i32>>,
    /// Sprint 5-11-1 / H1 PoC: AccessKit プラットフォームアダプタ（主 Window 用）。
    ///
    /// `on_resumed` で主 Window 作成時に初期化する。スクリーンリーダーが接続すると
    /// `InitialTreeRequested` イベントが `user_event` 経由で届き、`update_if_active` で
    /// ノードツリーを返す。Phase 5-11-2 以降で全 OS Window 対応に拡張する。
    pub(super) accesskit_adapter: Option<accesskit_winit::Adapter>,
    /// Sprint 5-11-2 Step 2-5: AccessKit ツリーの最終更新時刻（100ms スロットリング用）。
    ///
    /// `on_about_to_wait` 末尾の `update_accesskit_tree_if_needed` で参照・更新する。
    /// `None` の場合は次回の `about_to_wait` で必ず更新を試行する。
    pub(super) last_tree_update_at: Option<Instant>,
    /// Sprint 5-11-2 Step 2-5: 直近送出した `ClientState` のステートハッシュ。
    ///
    /// `compute_tree_state_hash(&state)` の結果と比較し、変化があったときのみ
    /// `update_if_active` を呼ぶ。`None` の場合は初回送出を強制する。
    pub(super) last_tree_hash: Option<u64>,
}

impl EventHandler {
    /// 新規 OS Window を生成し、`windows` HashMap に登録する（Sprint 5-8 Phase 4-4 本実装）。
    ///
    /// `on_resumed` の主 Window 生成フローと同じパターンで:
    /// 1. winit `Window` を `event_loop.create_window(...)` で生成
    /// 2. `WgpuState::new` で wgpu パイプラインを初期化（背景画像も同時にロード）
    /// 3. `PerWindowViewState { focused_server_window_id, .. Default::default() }` を作成
    /// 4. `ClientWindow` を組み立てて `self.windows.insert(window_id, ...)`
    ///
    /// 引数:
    /// - `event_loop`: winit `ActiveEventLoop`（Window 生成に必要）
    /// - `pos`: 新規 Window のスクリーン位置。`None` の場合は winit のデフォルト配置
    /// - `server_window_id`: 新規 Window が表示するサーバー Window ID（`view_state` に格納）
    ///
    /// 戻り値: 生成された Window の `WindowId`。Window 作成または wgpu 初期化失敗時は `None`。
    ///
    /// **注意**: グリフアトラス・フォント・サーバー接続は EventHandler レベルで共有しているため、
    /// 新規 OS Window は既存のサーバー接続（`self.connection`）を介してメッセージを受信する。
    /// `connection.send_tx` でのアタッチ要求は本関数の呼び出し側（[[project_sprint5_8_phase4_3_progress]] の
    /// `MovePaneToWindow` 経路）でサーバーが既に行っているため、ここでは送らない。
    pub(super) fn spawn_os_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        pos: Option<PhysicalPosition<i32>>,
        server_window_id: u32,
    ) -> Option<WindowId> {
        use nexterm_config::WindowDecorations;

        let win_cfg = &self.app.config.window;
        let transparent = win_cfg.background_opacity < 1.0;
        let decorations = !matches!(win_cfg.decorations, WindowDecorations::None);

        // Sprint 5-11-2 Step 2-3: AccessKit Adapter は Window を可視化する **前** に作成する。
        // `on_resumed` と同じ `with_visible(false)` → Adapter 初期化 → `set_visible(true)` シーケンス。
        let mut attrs = Window::default_attributes()
            .with_title(format!("Nexterm - Window {}", server_window_id))
            .with_inner_size(PhysicalSize::new(1280u32, 800u32))
            .with_transparent(transparent)
            .with_decorations(decorations)
            .with_visible(false);
        if let Some(p) = pos {
            attrs = attrs.with_position(p);
        }

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                warn!("新規 OS Window の作成に失敗: {}", e);
                return None;
            }
        };
        window.set_ime_allowed(true);

        // Sprint 5-11-2 Step 2-3: 新規 OS Window 用の AccessKit Adapter を初期化（可視化前）。
        let accesskit_adapter = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        info!(
            "新規 OS Window 用 AccessKit Adapter を初期化 (window_id={:?})",
            window.id()
        );

        // wgpu を非同期で初期化する（tokio runtime が必要）
        let mut wgpu_state = match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(WgpuState::new(Arc::clone(&window), &self.app.config.gpu))
        }) {
            Ok(s) => s,
            Err(e) => {
                warn!("新規 OS Window の wgpu 初期化に失敗: {}", e);
                return None;
            }
        };
        wgpu_state.load_background(&self.app.config.window);

        // Adapter 初期化が完了したので Window を可視化する
        window.set_visible(true);

        let window_id = window.id();
        let view_state = PerWindowViewState {
            focused_server_window_id: server_window_id,
            ..Default::default()
        };

        self.windows.insert(
            window_id,
            ClientWindow {
                window: Arc::clone(&window),
                wgpu: wgpu_state,
                view_state,
                accesskit_adapter,
            },
        );

        info!(
            "spawn_os_window: 新規 OS Window 生成 (window_id={:?}, server_window_id={}, pos={:?})",
            window_id, server_window_id, pos
        );

        // 即時 1 フレーム描画を要求
        window.request_redraw();
        Some(window_id)
    }

    /// 指定 `WindowId` の OS Window を破棄する（Sprint 5-8 Phase 4-4 本実装）。
    ///
    /// 動作:
    /// 1. `self.windows` から該当 `ClientWindow` を取り出して drop（wgpu surface / Window 解放）
    /// 2. 主 Window が閉じられたかを判定
    /// 3. **すべての OS Window が閉じられた**場合のみ、サーバータスクを abort して `event_loop.exit()`
    ///    （単一の追加 Window 閉鎖ではアプリ継続）
    ///
    /// 引数:
    /// - `event_loop`: 最後の Window 閉鎖時の `exit()` 呼び出しに使用
    /// - `window_id`: 破棄する Window の ID
    pub(super) fn close_os_window(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId) {
        let removed = self.windows.remove(&window_id);
        if removed.is_some() {
            info!(
                "close_os_window: OS Window を破棄 (window_id={:?})",
                window_id
            );
        }

        // 主 Window が閉じられたか
        let main_closed = self
            .window
            .as_ref()
            .map(|w| w.id() == window_id)
            .unwrap_or(true);

        // 主 Window が閉じられた場合は主 Window 参照もクリア
        if main_closed && self.window.as_ref().map(|w| w.id()) == Some(window_id) {
            self.window = None;
            self.wgpu_state = None;
        }

        // 全 OS Window が閉じられたら exit
        if self.windows.is_empty() && self.window.is_none() {
            self.connection = None;
            self.server_handle.abort();
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<UserEvent> for EventHandler {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.on_new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.on_resumed(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.on_about_to_wait(event_loop);
    }

    /// `UserEvent` 経由のリクエストを処理する（Sprint 5-8 Phase 4-4）。
    ///
    /// マウスハンドラやネットワーク受信スレッドが `&ActiveEventLoop` を持たない
    /// 文脈で発火した OS Window 操作要求を、ここで `&ActiveEventLoop` 付きで実行する。
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::SpawnOsWindow {
                server_window_id,
                pos,
            } => {
                let _ = self.spawn_os_window(event_loop, pos, server_window_id);
            }
            UserEvent::CloseOsWindow { window_id } => {
                self.close_os_window(event_loop, window_id);
            }
            // Sprint 5-11-1 / H1 PoC: AccessKit イベント
            // Sprint 5-11-2 Step 2-4: ActionRequested ディスパッチのため event_loop を渡す
            UserEvent::Accessibility(ak_event) => {
                self.on_accesskit_event(ak_event, event_loop);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Sprint 5-11-1 / H1 PoC + Step 2-3: AccessKit Adapter にウィンドウイベントを転送する。
        // フォーカス変更・カーソル移動などはアダプタ側でハンドリングされ、
        // 必要に応じてプラットフォーム a11y イベントとして発火される。
        // 描画イベントなど accesskit が無視するイベントもあるが、`process_event`
        // 内で振り分けられるため毎イベントに対して安全に呼び出してよい。
        //
        // Step 2-3 で複数 OS Window 対応: window_id から該当 Adapter を引く。
        // - 主 Window: `self.window` の ID と一致 → `self.accesskit_adapter`
        // - 追加 Window: `self.windows[window_id].accesskit_adapter`
        let is_main = self.window.as_ref().map(|w| w.id()) == Some(window_id);
        if is_main {
            if let (Some(adapter), Some(window)) =
                (self.accesskit_adapter.as_mut(), self.window.as_ref())
            {
                adapter.process_event(window, &event);
            }
        } else if let Some(cw) = self.windows.get_mut(&window_id) {
            cw.accesskit_adapter.process_event(&cw.window, &event);
        }

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
