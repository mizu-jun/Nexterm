//! winit `ApplicationHandler` のライフサイクルフック
//!
//! `event_handler.rs` から抽出した:
//! - `on_new_events` — 60fps タイマー設定
//! - `on_resumed` — ウィンドウ・wgpu 初期化・サーバー接続
//! - `on_about_to_wait` — サーバーメッセージポーリング・ホットリロード処理

use std::sync::Arc;
use std::time::{Duration, Instant};

use nexterm_proto::{ClientToServer, ServerToClient};
use tracing::{info, warn};
use winit::{
    dpi::PhysicalSize,
    event::StartCause,
    event_loop::{ActiveEventLoop, ControlFlow},
    window::Window,
};

use super::EventHandler;
use crate::connection::{Connection, ConnectionExt};
use crate::font::FontManager;
use crate::glyph_atlas::{GlyphAtlas, GlyphKey};
use crate::renderer::WgpuState;

impl EventHandler {
    /// `ApplicationHandler::new_events` の実装
    pub(super) fn on_new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        // PTY 出力を 16ms ごとにポーリングする（約 60fps）
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(16),
        ));
    }

    /// `ApplicationHandler::resumed` の実装
    pub(super) fn on_resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ウィンドウを作成する（設定に従って透過・ぼかし・装飾を適用する）
        use nexterm_config::WindowDecorations;
        let win_cfg = &self.app.config.window;
        let transparent = win_cfg.background_opacity < 1.0;
        let decorations = !matches!(win_cfg.decorations, WindowDecorations::None);

        // Sprint 5-11-1 / H1 PoC: AccessKit Adapter は **Window を可視化する前** に
        // 作成する必要がある（accesskit_winit::Adapter::new のドキュメント参照）。
        // そのため `with_visible(false)` で不可視作成 → Adapter 初期化 → `set_visible(true)` の
        // 順序を守る。可視化前に Adapter を入れないとプラットフォーム側 a11y ツリーが
        // 正しく初期化されない。
        let attrs = Window::default_attributes()
            .with_title("Nexterm")
            .with_inner_size(PhysicalSize::new(1280u32, 800u32))
            .with_transparent(transparent)
            .with_decorations(decorations)
            .with_visible(false);

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("Failed to create window"),
        );

        // Sprint 5-11-1 / H1 PoC: AccessKit Adapter を初期化（可視化前）。
        //
        // `Adapter::with_event_loop_proxy` は内部で activation / action / deactivation の
        // 3 種ハンドラを `EventLoopProxy<UserEvent>` 経由でイベント送信するよう設定する。
        // この `proxy` は `From<accesskit_winit::Event> for UserEvent` を経由するため
        // `user_event` ハンドラが `UserEvent::Accessibility(...)` として受け取る。
        //
        // **既定 ON 方針 (Q2=a 自動検出)**: スクリーンリーダーが起動していない場合は
        // プラットフォーム側で adapter が非アクティブ状態を保つため、CPU/メモリオーバーヘッドは
        // ほぼゼロ。明示的な opt-in 設定は不要。
        let accesskit_adapter = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        info!("AccessKit Adapter を初期化（スクリーンリーダー接続待ち）");
        self.accesskit_adapter = Some(accesskit_adapter);

        // Adapter 初期化が完了したので Window を可視化する
        window.set_visible(true);

        // アプリケーションアイコンを設定する
        {
            let icon_bytes = include_bytes!("../../../../assets/nexterm-source.png");
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
        let mut wgpu_state = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(WgpuState::new(Arc::clone(&window), &self.app.config.gpu))
        })
        .expect("Failed to initialize wgpu");

        // 背景画像をロード（Sprint 5-7 / Phase 3-1）。失敗時は内部で warn ログ
        wgpu_state.load_background(&self.app.config.window);

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

        // Sprint 5-8 Phase 4-1 Step 1.5: Quake モードの対象 OS Window を **主 Window** に固定する。
        // Phase 4-2 で複数 OS Window が実装された後も、Quake 表示・隠蔽は常にここで設定した
        // `target_window_id` 経由でのみ行う（他の Window が誤って Quake トグルされない設計）。
        self.quake.target_window_id = Some(window.id());

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

    /// `ApplicationHandler::about_to_wait` の実装
    pub(super) fn on_about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
            // Sprint 5-8 Phase 4-4 Step C: `WindowListChanged` を検出して、
            // タブ外ドロップで生まれた新規 Window があれば OS Window をスポーン要求する。
            // `pending_new_window_drop_pos` が `Some` のときのみ発火する（手動 break_pane や
            // 他クライアント由来の Window 生成では OS Window スポーンしない設計）。
            if let ServerToClient::WindowListChanged { ref windows } = msg {
                let current_ids: std::collections::HashSet<u32> =
                    windows.iter().map(|w| w.window_id).collect();
                let new_ids: Vec<u32> = current_ids
                    .difference(&self.known_server_window_ids)
                    .copied()
                    .collect();
                if !new_ids.is_empty()
                    && let Some(pos) = self.pending_new_window_drop_pos.take()
                {
                    // 最小 ID（= 最も新しい Window）を採用。複数同時生成は想定外
                    let server_window_id = *new_ids.iter().min().expect("new_ids 非空");
                    if let Err(e) =
                        self.proxy
                            .send_event(crate::renderer::UserEvent::SpawnOsWindow {
                                server_window_id,
                                pos: Some(pos),
                            })
                    {
                        tracing::warn!("SpawnOsWindow UserEvent 送信失敗: {}", e);
                    } else {
                        tracing::info!(
                            "新規 OS Window スポーン要求送信 (server_window_id={}, pos={:?})",
                            server_window_id,
                            pos
                        );
                    }
                }
                self.known_server_window_ids = current_ids;
            }

            // 機密操作要求は SecurityConfig ポリシーに従って処理する（Sprint 4-1）
            match msg {
                ServerToClient::DesktopNotification {
                    pane_id,
                    title,
                    body,
                } => {
                    // Sprint 5-11-5: SR には consent 設定に関わらず通知する。
                    // SR は OS 通知の代替ではなくアクセシビリティ手段のため、
                    // 同意ポリシーで OS 通知を抑止していても SR には届く必要がある。
                    self.app.state.add_alert(
                        crate::state::AlertKind::Notification,
                        pane_id,
                        title.clone(),
                        body.clone(),
                    );
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

        // Phase 4-5: 保留中の Window 閉じ要求を処理する
        // （QueryForegroundProcess の応答受信 → 確認ダイアログ表示 or 即時 Kill）
        self.poll_pending_close_request(event_loop);

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
            // フォーカスペインの cwd を取得して WidgetContext に詰める
            // （Sprint 5-7 / UI-1-2: cwd / cwd_short / git_branch ウィジェット用）
            let cwd = self
                .app
                .state
                .focused_pane_id
                .and_then(|id| self.app.state.panes.get(&id))
                .and_then(|p| p.cwd.clone());
            let ctx = nexterm_config::WidgetContext {
                session_name: Some("main".to_string()),
                pane_id: self.app.state.focused_pane_id,
                cwd,
                workspace_name: Some(self.app.state.current_workspace.clone()),
            };
            let sep = &self.app.config.status_bar.separator;
            self.app.state.status_bar_text =
                eval.evaluate_with_context(&self.app.config.status_bar.widgets, &ctx, sep);
            self.app.state.status_bar_right_text =
                eval.evaluate_with_context(&self.app.config.status_bar.right_widgets, &ctx, sep);
            self.last_status_eval = Instant::now();
            had_messages = true;
        }

        // Sprint 5-7 / UI-1-4: キーヒントオーバーレイの期限切れ判定
        if let Some(deadline) = self.app.state.key_hint_visible_until
            && Instant::now() >= deadline
        {
            self.app.state.key_hint_visible_until = None;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        // Sprint 5-7 / UI-1-4 bug fix: prefix モードの期限切れ判定（タイムアウト 2 秒）
        if let Some(deadline) = self.app.state.prefix_pending_until
            && Instant::now() >= deadline
        {
            self.app.state.prefix_pending_until = None;
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

        // Sprint 5-7 / Phase 2-2: Quake モード処理
        // 1) global-hotkey 押下イベントを排出し、押下があれば "toggle" として扱う
        // 2) サーバー経由のトグル要求 (`pending_quake_action`) を取り出す
        // 3) 両者を統合して 1 回だけウィンドウ操作する（同フレームで複数発火しても 1 回）
        self.handle_quake_tick();

        // Sprint 5-11-2 Step 2-5: AccessKit ツリーのライブ更新。
        // 100ms スロットリング + 状態ハッシュ比較を内部で実施するため、毎フレーム呼んで安全。
        // SR が非接続なら `update_if_active` が no-op になるためオーバーヘッドはほぼゼロ。
        self.update_accesskit_tree_if_needed();
    }

    /// Quake モードのトグル要求を 1 フレームに 1 回処理する
    pub(super) fn handle_quake_tick(&mut self) {
        // ホットキー押下を排出
        let hotkey_pressed = self.quake.drain_pressed();
        // IPC 経由の保留アクション
        let pending = self.app.state.pending_quake_action.take();

        // 何も無ければ早期 return
        if !hotkey_pressed && pending.is_none() {
            return;
        }

        // 統合: ホットキーは常に "toggle"。IPC 経由のアクション指定があればそちらを優先
        let action = pending.unwrap_or_else(|| "toggle".to_string());

        let Some(window) = self.window.as_ref().cloned() else {
            warn!("Quake トグル要求を受け取りましたがウィンドウが未初期化です");
            return;
        };

        let cfg = self.app.config.quake_mode.clone();
        match action.as_str() {
            "toggle" => {
                if self.quake.visible {
                    crate::quake::hide_window(&window, &cfg, self.quake.saved.as_ref());
                    self.quake.visible = false;
                } else {
                    let saved = crate::quake::show_window(&window, &cfg);
                    if saved.is_some() {
                        self.quake.saved = saved;
                    }
                    self.quake.visible = true;
                }
            }
            "show" => {
                if !self.quake.visible {
                    let saved = crate::quake::show_window(&window, &cfg);
                    if saved.is_some() {
                        self.quake.saved = saved;
                    }
                    self.quake.visible = true;
                } else {
                    window.focus_window();
                }
            }
            "hide" => {
                if self.quake.visible {
                    crate::quake::hide_window(&window, &cfg, self.quake.saved.as_ref());
                    self.quake.visible = false;
                }
            }
            other => {
                warn!("未知の Quake action '{}' を受信。無視します", other);
            }
        }
        window.request_redraw();
    }
}
