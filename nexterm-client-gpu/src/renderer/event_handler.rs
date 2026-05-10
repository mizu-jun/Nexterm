//! Sprint 2-1 Phase B: winit イベントハンドラ
//!
//! `renderer/mod.rs` から抽出した:
//! - `EventHandler` 構造体（winit `ApplicationHandler` 実装を保持）
//! - `SettingsPanelHit` enum（設定パネルマウスヒットテスト結果）
//! - `impl EventHandler` ヘルパー（`hit_test_settings_panel`）
//! - `impl ApplicationHandler for EventHandler`（メイン winit イベントループ）

use std::sync::Arc;
use std::time::{Duration, Instant};

use nexterm_config::{Config, StatusBarEvaluator};
use nexterm_proto::{ClientToServer, ServerToClient};
use tracing::{info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode as WKeyCode, ModifiersState, PhysicalKey},
    window::{Window, WindowId},
};

use crate::connection::{Connection, ConnectionExt};
use crate::font::FontManager;
use crate::glyph_atlas::{GlyphAtlas, GlyphKey};
use crate::state::ContextMenu;
use crate::vertex_util::visual_width;

use super::{NextermApp, WgpuState};

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

/// 設定パネルに対するマウスヒットテスト結果
enum SettingsPanelHit {
    /// パネル外をクリック → パネルを閉じる
    Outside,
    /// タイトルバーエリア（ドラッグ移動等の将来拡張用）
    TitleBar,
    /// サイドバーカテゴリをクリック
    Category(usize),
    /// スライダーをクリック/ドラッグ
    Slider {
        slider_type: crate::settings_panel::SliderType,
        track_x: f32,
        track_w: f32,
        #[allow(dead_code)]
        min: f32,
        #[allow(dead_code)]
        max: f32,
    },
    /// テーマカラードット
    ThemeColor(usize),
    /// パネル内の空白エリア（何もしない）
    PanelBackground,
}

impl EventHandler {
    /// 設定パネルに対するマウスヒットテストを実行する
    fn hit_test_settings_panel(&self, cx: f32, cy: f32) -> SettingsPanelHit {
        use crate::settings_panel::{SettingsCategory, SliderType};

        let sp = &self.app.state.settings_panel;
        if !sp.is_open {
            return SettingsPanelHit::Outside;
        }
        let (sw, sh) = match self.wgpu_state.as_ref() {
            Some(w) => (
                w.surface_config.width as f32,
                w.surface_config.height as f32,
            ),
            None => return SettingsPanelHit::Outside,
        };
        let cell_w = self.app.font.cell_width();
        let cell_h = self.app.font.cell_height();

        // パネル寸法 (build_settings_panel_verts と同じ式)
        let panel_w = (sw * 0.72).min(sw - cell_w * 4.0);
        let panel_h = (sh * 0.75).min(sh - cell_h * 4.0);
        let px = (sw - panel_w) / 2.0;
        let eased = sp.eased_progress();
        let slide_offset = (1.0 - eased) * 16.0;
        let py = (sh - panel_h) / 2.0 + slide_offset;

        let sidebar_w = cell_w * 18.0;
        let content_x = px + sidebar_w;
        let content_w = panel_w - sidebar_w;
        let content_inner_x = content_x + cell_w;

        // パネル外 → 閉じる
        if cx < px || cx > px + panel_w || cy < py || cy > py + panel_h {
            return SettingsPanelHit::Outside;
        }

        // タイトルバー
        let title_h = cell_h * 1.4;
        if cy < py + title_h {
            return SettingsPanelHit::TitleBar;
        }

        // サイドバーカテゴリ
        let sidebar_top = py + title_h;
        let cat_item_h = cell_h * 1.3;
        if cx < px + sidebar_w {
            let rel_y = cy - sidebar_top;
            if rel_y >= 0.0 {
                let cat_idx = (rel_y / cat_item_h) as usize;
                if cat_idx < SettingsCategory::ALL.len() {
                    return SettingsPanelHit::Category(cat_idx);
                }
            }
            return SettingsPanelHit::PanelBackground;
        }

        // コンテンツ領域ヒットテスト
        let content_top = py + title_h + cell_h * 0.5;
        let bar_w = content_w - cell_w * 3.0;

        match &sp.category {
            SettingsCategory::Font => {
                // フォントサイズスライダー
                let bar_y = content_top + cell_h * 4.2;
                if cy >= bar_y - cell_h * 0.5
                    && cy <= bar_y + cell_h
                    && cx >= content_inner_x
                    && cx <= content_inner_x + bar_w
                {
                    return SettingsPanelHit::Slider {
                        slider_type: SliderType::FontSize,
                        track_x: content_inner_x,
                        track_w: bar_w,
                        min: 8.0,
                        max: 32.0,
                    };
                }
            }
            SettingsCategory::Theme => {
                // テーマカラードット
                let dot_y = content_top + cell_h * 2.5;
                let dot_gap = (content_w - cell_w * 2.0) / 9.0;
                let dot_size = cell_w * 1.2;
                if cy >= dot_y && cy <= dot_y + cell_h {
                    for i in 0..9_usize {
                        let dot_x = content_inner_x + i as f32 * dot_gap;
                        if cx >= dot_x && cx <= dot_x + dot_size {
                            return SettingsPanelHit::ThemeColor(i);
                        }
                    }
                }
            }
            SettingsCategory::Window => {
                // 不透明度スライダー
                let bar_y = content_top + cell_h * 2.4;
                if cy >= bar_y - cell_h * 0.5
                    && cy <= bar_y + cell_h
                    && cx >= content_inner_x
                    && cx <= content_inner_x + bar_w
                {
                    return SettingsPanelHit::Slider {
                        slider_type: SliderType::WindowOpacity,
                        track_x: content_inner_x,
                        track_w: bar_w,
                        min: 0.1,
                        max: 1.0,
                    };
                }
            }
            _ => {}
        }

        SettingsPanelHit::PanelBackground
    }
}

// ---- Sprint 4-1: 機密操作の同意フロー ----

impl EventHandler {
    /// OSC 9 / 777 デスクトップ通知要求を処理する
    ///
    /// `SecurityConfig.osc_notification` ポリシー + セッション内 override に従って
    /// 即時送信 / 拒否 / 同意ダイアログ表示のいずれかを行う。
    pub(super) fn handle_notification_request(
        &mut self,
        pane_id: u32,
        title: String,
        body: String,
    ) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.osc_notification;
        let session_override = self.app.state.session_consent_overrides.osc_notification;
        // 通知本文は config の上限で切り詰める（DoS 対策）
        let max = self.app.config.security.notification_max_bytes;
        let body = truncate_utf8_at(&body, max);

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                // ダイアログを表示。ユーザーの選択は input_handler 側で処理する
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::Notification {
                        source_pane: pane_id,
                        title,
                        body,
                    },
                ));
                return;
            }
        };

        if allow {
            crate::notification::send_notification(&title, &body);
        } else {
            tracing::info!(
                "デスクトップ通知をポリシーで拒否しました: pane={} title={:?}",
                pane_id,
                title
            );
        }
    }

    /// OSC 52 クリップボード書き込み要求を処理する
    ///
    /// `SecurityConfig.osc52_clipboard` ポリシー + セッション内 override に従って
    /// 即時書き込み / 拒否 / 同意ダイアログ表示のいずれかを行う。
    pub(super) fn handle_clipboard_write_request(&mut self, pane_id: u32, text: String) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.osc52_clipboard;
        let session_override = self.app.state.session_consent_overrides.osc52_clipboard;
        // 設定で許可された最大バイト数を超える要求は無条件で拒否
        let max = self.app.config.security.osc52_max_bytes;
        if text.len() > max {
            tracing::warn!(
                "OSC 52 要求をサイズ上限超過で拒否: pane={} bytes={} max={}",
                pane_id,
                text.len(),
                max
            );
            return;
        }

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::ClipboardWrite {
                        source_pane: Some(pane_id),
                        text,
                    },
                ));
                return;
            }
        };

        if allow {
            match arboard::Clipboard::new() {
                Ok(mut clipboard) => {
                    if let Err(e) = clipboard.set_text(text) {
                        tracing::warn!("OSC 52 クリップボード書き込み失敗: {}", e);
                    }
                }
                Err(_) => {
                    tracing::warn!("OSC 52: クリップボード API を初期化できません");
                }
            }
        } else {
            tracing::info!(
                "OSC 52 クリップボード要求をポリシーで拒否しました: pane={}",
                pane_id
            );
        }
    }

    /// 外部 URL を開く要求を処理する（Ctrl+クリック / OSC 8 経由）
    ///
    /// `SecurityConfig.external_url` ポリシー + セッション内 override に従う。
    pub(super) fn request_open_url(&mut self, url: String) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.external_url;
        let session_override = self.app.state.session_consent_overrides.external_url;

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::OpenUrl(url),
                ));
                return;
            }
        };

        if allow {
            crate::vertex_util::open_url(&url);
        } else {
            tracing::info!("URL オープン要求をポリシーで拒否しました: {}", url);
        }
    }

    /// 同意ダイアログのユーザー決定を実行する
    ///
    /// 呼び出し側 (input_handler) はキー入力を解釈してこのメソッドを呼ぶ。
    /// 引数 `decision`:
    /// - `Some(true)`: 1 度だけ許可
    /// - `Some(false)`: 1 度だけ拒否
    /// - `None`: ダイアログ閉じるのみ（拒否扱い）
    ///
    /// 引数 `always`: true なら同種の要求をセッション中常に同じ決定で扱う
    pub(super) fn resolve_pending_consent(&mut self, decision: Option<bool>, always: bool) {
        let Some(dialog) = self.app.state.pending_consent.take() else {
            return;
        };
        let allow = decision.unwrap_or(false);

        if always {
            match &dialog.kind {
                crate::state::ConsentKind::OpenUrl(_) => {
                    self.app.state.session_consent_overrides.external_url = Some(allow);
                }
                crate::state::ConsentKind::ClipboardWrite { .. } => {
                    self.app.state.session_consent_overrides.osc52_clipboard = Some(allow);
                }
                crate::state::ConsentKind::Notification { .. } => {
                    self.app.state.session_consent_overrides.osc_notification = Some(allow);
                }
            }
        }

        if !allow {
            return;
        }

        match dialog.kind {
            crate::state::ConsentKind::OpenUrl(url) => {
                crate::vertex_util::open_url(&url);
            }
            crate::state::ConsentKind::ClipboardWrite { text, .. } => {
                if let Ok(mut clipboard) = arboard::Clipboard::new()
                    && let Err(e) = clipboard.set_text(text)
                {
                    tracing::warn!("クリップボード書き込み失敗: {}", e);
                }
            }
            crate::state::ConsentKind::Notification { title, body, .. } => {
                crate::notification::send_notification(&title, &body);
            }
        }
    }
}

/// バイト長で UTF-8 文字境界を尊重しつつ文字列を切り詰める
fn truncate_utf8_at(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

impl ApplicationHandler for EventHandler {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        // PTY 出力を 16ms ごとにポーリングする（約 60fps）
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(16),
        ));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ウィンドウを作成する（設定に従って透過・ぼかし・装飾を適用する）
        use nexterm_config::WindowDecorations;
        let win_cfg = &self.app.config.window;
        let transparent = win_cfg.background_opacity < 1.0;
        let decorations = !matches!(win_cfg.decorations, WindowDecorations::None);

        let attrs = Window::default_attributes()
            .with_title("Nexterm")
            .with_inner_size(PhysicalSize::new(1280u32, 800u32))
            .with_transparent(transparent)
            .with_decorations(decorations);

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("Failed to create window"),
        );

        // アプリケーションアイコンを設定する
        {
            let icon_bytes = include_bytes!("../../../assets/nexterm-source.png");
            if let Ok(img) = image::load_from_memory(icon_bytes) {
                let rgba = img.into_rgba8();
                let (iw, ih) = (rgba.width(), rgba.height());
                if let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), iw, ih) {
                    window.set_window_icon(Some(icon));
                }
            }
        }

        // IME 入力を有効にする
        window.set_ime_allowed(true);

        // DPI スケール係数を取得し、フォントを実スケールで再生成する
        let scale_factor = window.scale_factor() as f32;
        self.scale_factor = scale_factor;
        self.app.font = FontManager::new(
            &self.app.config.font.family,
            self.app.config.font.size,
            &self.app.config.font.font_fallbacks,
            scale_factor,
            self.app.config.font.ligatures,
        );

        // Acrylic（すりガラス）背景を適用する（Windows 11 のみ有効）
        #[cfg(windows)]
        crate::platform::apply_acrylic_blur(&window);

        // wgpu を非同期で初期化する（tokio runtime が必要）
        let wgpu_state = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(WgpuState::new(Arc::clone(&window), &self.app.config.gpu))
        })
        .expect("Failed to initialize wgpu");

        let mut atlas =
            GlyphAtlas::new_with_config(&wgpu_state.device, self.app.config.gpu.atlas_size);

        // ASCII 印字可能文字（0x20-0x7E）をグリフアトラスに事前ロードする。
        // 初回のキーストローク遅延を排除し、起動直後からスムーズな描画を実現する。
        for ch in ' '..='~' {
            for bold in [false, true] {
                let key = GlyphKey {
                    ch,
                    bold,
                    italic: false,
                    wide: false,
                };
                let (w, h, pixels) =
                    self.app
                        .font
                        .rasterize_char(ch, bold, false, [220, 220, 220, 255], false);
                if w > 0 && h > 0 {
                    atlas.get_or_insert(key, &pixels, w, h, &wgpu_state.queue);
                }
            }
        }

        // ウィンドウサイズからセル数を計算してステートを初期化する
        // タブバー（上部）とステータスバー（下部1セル）を除いた領域でセル数を計算する
        let size = window.inner_size();
        let cell_h_init = self.app.font.cell_height();
        let tab_bar_h_init = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f32
        } else {
            0.0
        };
        let status_bar_h_init = cell_h_init;
        let pad_x = self.app.config.window.padding_x as f32;
        let pad_y = self.app.config.window.padding_y as f32;
        let cols = ((size.width as f32 - pad_x * 2.0) / self.app.font.cell_width()).max(1.0) as u16;
        let rows = ((size.height as f32 - tab_bar_h_init - status_bar_h_init - pad_y * 2.0)
            / cell_h_init)
            .max(1.0) as u16;
        self.app.state.resize(cols, rows);

        self.window = Some(Arc::clone(&window));
        self.atlas = Some(atlas);
        self.wgpu_state = Some(wgpu_state);

        // サーバーに接続してデフォルトセッションにアタッチする
        let conn = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                match Connection::connect_gpu().await {
                    Ok(conn) => {
                        // セッションにアタッチ → 実際のサイズを通知
                        let _ = conn.send_tx.try_send(ClientToServer::Attach {
                            session_name: "main".to_string(),
                        });
                        let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
                        info!("Connected to nexterm server");
                        Some(conn)
                    }
                    Err(e) => {
                        warn!("Failed to connect to server (offline mode): {}", e);
                        None
                    }
                }
            })
        });
        self.connection = conn;

        info!("wgpu renderer initialized");
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // サーバーからのメッセージをポーリングして状態を更新する
        // borrow checker のため、まず受信したメッセージを Vec に集めてから処理する
        let mut had_messages = false;
        let mut messages = Vec::new();
        if let Some(conn) = &mut self.connection {
            while let Ok(msg) = conn.recv_rx.try_recv() {
                messages.push(msg);
                had_messages = true;
            }
        }
        for msg in messages {
            // 機密操作要求は SecurityConfig ポリシーに従って処理する（Sprint 4-1）
            match msg {
                ServerToClient::DesktopNotification {
                    pane_id,
                    title,
                    body,
                } => {
                    self.handle_notification_request(pane_id, title, body);
                }
                ServerToClient::ClipboardWriteRequest { pane_id, text } => {
                    self.handle_clipboard_write_request(pane_id, text);
                }
                other => {
                    self.app.state.apply_server_message(other);
                }
            }
        }

        // BEL を受信していればウィンドウ注目要求を発行する
        if self.app.state.pending_bell {
            self.app.state.pending_bell = false;
            if let Some(w) = &self.window {
                w.request_user_attention(Some(winit::window::UserAttentionType::Informational));
            }
        }

        // 設定ホットリロードをポーリングする（最新の設定を適用する）
        if let Some(rx) = &mut self.config_rx
            && let Ok(new_config) = rx.try_recv()
        {
            info!(
                "Config reloaded: font={} {}pt",
                new_config.font.family, new_config.font.size
            );
            // フォントサイズ変更時はグリフアトラスも再生成する
            let font_changed = self.app.config.font != new_config.font;
            self.app.config = new_config;
            if font_changed {
                self.app.font = crate::font::FontManager::new(
                    &self.app.config.font.family,
                    self.app.config.font.size,
                    &self.app.config.font.font_fallbacks,
                    self.scale_factor,
                    self.app.config.font.ligatures,
                );
                let atlas_size = self.app.config.gpu.atlas_size;
                if let Some(wgpu) = &self.wgpu_state {
                    self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
                }
            }
            had_messages = true;
        }

        // カスタムシェーダーファイルの変更をポーリングしてパイプラインを再構築する
        if let Some(rx) = &mut self.shader_reload_rx
            && rx.try_recv().is_ok()
        {
            // チャネルをドレインして複数イベントを 1 回にまとめる
            while rx.try_recv().is_ok() {}
            if let Some(wgpu) = &mut self.wgpu_state {
                wgpu.reload_shader_pipelines(&self.app.config.gpu);
            }
            had_messages = true;
        }

        // ステータスバーを 1 秒ごとに再評価してキャッシュを更新する
        if self.app.config.status_bar.enabled
            && self.last_status_eval.elapsed() >= Duration::from_secs(1)
            && let Some(eval) = &self.status_eval
        {
            let ctx = nexterm_config::WidgetContext {
                session_name: Some("main".to_string()),
                pane_id: self.app.state.focused_pane_id,
            };
            let sep = &self.app.config.status_bar.separator;
            self.app.state.status_bar_text =
                eval.evaluate_with_context(&self.app.config.status_bar.widgets, &ctx, sep);
            self.app.state.status_bar_right_text =
                eval.evaluate_with_context(&self.app.config.status_bar.right_widgets, &ctx, sep);
            self.last_status_eval = Instant::now();
            had_messages = true;
        }

        // 更新チェッカーからの通知をポーリングしてバナーを表示する
        if self.update_rx.has_changed().unwrap_or(false)
            && let Some(ver) = self.update_rx.borrow_and_update().clone()
            && self.app.state.update_banner.is_none()
        {
            self.app.state.update_banner = Some(ver);
            had_messages = true;
        }

        if had_messages && let Some(w) = &self.window {
            w.request_redraw();
        }

        // 設定パネルの開閉アニメーションを進める（60fps 想定で約 8フレーム = 0.13秒）
        let sp = &mut self.app.state.settings_panel;
        if sp.is_open && sp.open_progress < 1.0 {
            sp.open_progress = (sp.open_progress + 0.15).min(1.0);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                // IPC 接続を先にドロップしてチャネルを閉じる（Windows でのハング防止）
                self.connection = None;
                // サーバータスクを abort してからイベントループを終了する
                self.server_handle.abort();
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                let cell_h_r = self.app.font.cell_height();
                let tab_bar_h_r = if self.app.config.tab_bar.enabled {
                    self.app.config.tab_bar.height as f32
                } else {
                    0.0
                };
                let pad_x_r = self.app.config.window.padding_x as f32;
                let pad_y_r = self.app.config.window.padding_y as f32;
                let cols = ((size.width as f32 - pad_x_r * 2.0) / self.app.font.cell_width())
                    .max(1.0) as u16;
                let rows = ((size.height as f32 - tab_bar_h_r - cell_h_r - pad_y_r * 2.0)
                    / cell_h_r)
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

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
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
                    let cols = ((size.width as f32 - pad_x_sf * 2.0) / self.app.font.cell_width())
                        .max(1.0) as u16;
                    let rows = ((size.height as f32 - tab_bar_h_sf - cell_h_sf - pad_y_sf * 2.0)
                        / cell_h_sf)
                        .max(1.0) as u16;
                    self.app.state.resize(cols, rows);
                    if let Some(conn) = &self.connection {
                        let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
                    }
                }
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            // マウスカーソル位置を追跡する（ドラッグ中は選択範囲を更新する）
            WindowEvent::CursorMoved { position, .. } => {
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

            // 右ボタン押下: コンテキストメニューを開く
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state: ElementState::Pressed,
                ..
            } => {
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

            // 左ボタン押下: タブバークリック判定 + 選択開始 + マウスレポート
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } => {
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
                                if let Some(cat) =
                                    crate::settings_panel::SettingsCategory::ALL.get(idx)
                                {
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
                            self.app.state.settings_panel.is_open =
                                !self.app.state.settings_panel.is_open;
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
                                        id == pane_id
                                            && now.duration_since(t) < Duration::from_millis(300)
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
                                        let _ = conn
                                            .send_tx
                                            .try_send(ClientToServer::FocusPane { pane_id });
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

            // 左ボタンリリース: 選択確定 → クリップボードコピー or フォーカス切替
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } => {
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

            // マウスホイールでスクロールバックをスクロールする
            WindowEvent::MouseWheel { delta, .. } => {
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

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key,
                        state: ElementState::Pressed,
                        text,
                        ..
                    },
                ..
            } => {
                // 検索モードの文字入力を処理する（PTY には転送しない）
                if self.app.state.search.is_active {
                    if matches!(physical_key, PhysicalKey::Code(WKeyCode::Backspace)) {
                        self.app.state.pop_search_char();
                    } else if let Some(ref t) = text
                        && !self.modifiers.control_key()
                    {
                        for ch in t.chars() {
                            self.app.state.push_search_char(ch);
                        }
                    }
                    // Escape / Enter は handle_key で処理する
                    if let PhysicalKey::Code(code) = physical_key
                        && matches!(code, WKeyCode::Escape | WKeyCode::Enter)
                    {
                        self.handle_key(code, event_loop);
                    }
                    return;
                }

                // ローカル操作（パレット・検索開始など）をチェックする
                let consumed = if let PhysicalKey::Code(code) = physical_key {
                    self.handle_key(code, event_loop)
                } else {
                    false
                };

                // 設定パネルのフォントファミリー入力中は文字をフィールドに追加する
                if !consumed
                    && self.app.state.settings_panel.is_open
                    && self.app.state.settings_panel.font_family_editing
                {
                    if let Some(ref t) = text
                        && !self.modifiers.control_key()
                        && !self.modifiers.alt_key()
                    {
                        for ch in t.chars() {
                            self.app.state.settings_panel.push_font_family_char(ch);
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                        return;
                    }
                    // テキストがない場合（矢印キー等）もサーバーへは転送しない
                    return;
                }

                // ローカルで消費されなかった場合はサーバーへ転送する
                if !consumed {
                    self.forward_key_to_server(physical_key, text.as_deref());
                }
            }

            // IME イベントを処理する（日本語・中国語などの入力に対応）
            WindowEvent::Ime(ime_event) => {
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

            WindowEvent::RedrawRequested => {
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

            _ => {}
        }

        // 毎フレーム再描画をリクエストする
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
