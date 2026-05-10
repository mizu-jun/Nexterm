//! Sprint 2-1 Phase B: GPU アプリケーション本体
//!
//! `renderer/mod.rs` から抽出した `NextermApp` 構造体と
//! `into_event_handler()` メソッド。

use anyhow::Result;
use nexterm_config::{Config, StatusBarEvaluator};
use std::time::Instant;

use winit::keyboard::ModifiersState;

use crate::font::FontManager;
use crate::state::ClientState;

use super::{EventHandler, start_shader_watcher};

// ---- アプリケーション本体 ----

/// GPU アプリケーション（winit EventLoop に渡す）
pub struct NextermApp {
    pub(super) config: Config,
    pub(super) state: ClientState,
    pub(super) font: FontManager,
}

impl NextermApp {
    pub async fn new(config: Config) -> Result<Self> {
        let font = FontManager::new(
            &config.font.family,
            config.font.size,
            &config.font.font_fallbacks,
            1.0,
            config.font.ligatures,
        );
        let mut state = ClientState::new(80, 24, config.scrollback_lines);
        // 設定ファイルのホスト一覧をホストマネージャに渡す
        state.host_manager = crate::host_manager::HostManager::new(config.hosts.clone());
        // 設定ファイルの Lua マクロ一覧をマクロピッカーに渡す
        state.macro_picker = crate::macro_picker::MacroPicker::new(config.macros.clone());
        // 設定パネルを設定値で初期化する
        state.settings_panel = crate::settings_panel::SettingsPanel::new(&config);
        Ok(Self {
            config,
            state,
            font,
        })
    }

    pub fn into_event_handler(
        self,
        config_rx: Option<tokio::sync::mpsc::Receiver<Config>>,
        config_watcher: Option<notify::RecommendedWatcher>,
        status_eval: Option<StatusBarEvaluator>,
        server_handle: tokio::task::JoinHandle<()>,
        update_rx: tokio::sync::watch::Receiver<Option<String>>,
    ) -> EventHandler {
        // カスタムシェーダーファイルが設定されていれば監視を開始する
        let (shader_reload_rx, _shader_watcher) = start_shader_watcher(&self.config.gpu);

        EventHandler {
            app: self,
            wgpu_state: None,
            atlas: None,
            window: None,
            modifiers: ModifiersState::empty(),
            connection: None,
            cursor_position: None,
            config_rx,
            _config_watcher: config_watcher,
            status_eval,
            last_status_eval: Instant::now(),
            scale_factor: 1.0,
            shader_reload_rx,
            _shader_watcher,
            last_tab_click: None,
            server_handle,
            pixel_scroll_accumulator: 0.0,
            update_rx,
        }
    }
}
