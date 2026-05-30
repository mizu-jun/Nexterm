//! Sprint 2-1 Phase B: the GPU application root.
//!
//! Extracted from `renderer/mod.rs`: the `NextermApp` struct and its
//! `into_event_handler()` method.

use anyhow::Result;
use nexterm_config::{Config, StatusBarEvaluator};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use winit::event_loop::EventLoopProxy;
use winit::keyboard::ModifiersState;

use crate::font::FontManager;
use crate::state::ClientState;

use super::event_handler::UserEvent;
use super::{EventHandler, start_shader_watcher};

// ---- Application root ----

/// The GPU application (handed to the winit EventLoop).
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
        // Hand the host list from the config file to the host manager
        state.host_manager = crate::host_manager::HostManager::new(config.hosts.clone());
        // Hand the Lua macro list from the config file to the macro picker
        state.macro_picker = crate::macro_picker::MacroPicker::new(config.macros.clone());
        // Initialize the settings panel from the config values
        state.settings_panel = crate::settings_panel::SettingsPanel::new(&config);
        Ok(Self {
            config,
            state,
            font,
        })
    }

    pub fn into_event_handler(
        self,
        proxy: EventLoopProxy<UserEvent>,
        config_rx: Option<tokio::sync::mpsc::Receiver<Config>>,
        config_watcher: Option<notify::RecommendedWatcher>,
        status_eval: Option<StatusBarEvaluator>,
        server_handle: tokio::task::JoinHandle<()>,
        update_rx: tokio::sync::watch::Receiver<Option<String>>,
    ) -> EventHandler {
        // Start watching the custom shader file if one is configured
        let (shader_reload_rx, _shader_watcher) = start_shader_watcher(&self.config.gpu);

        // Sprint 5-7 / Phase 2-2: initialize the Quake-mode runtime (registers a global hotkey)
        let quake = crate::quake::QuakeRuntime::new(&self.config.quake_mode);

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
            quake,
            // Sprint 5-8 Phase 4-1 Step 1.2 .. Phase 4-4: multi OS-window support
            windows: HashMap::new(),
            // Sprint 5-8 Phase 4-4: proxy used to spawn OS windows via UserEvent
            proxy,
            known_server_window_ids: HashSet::new(),
            pending_new_window_drop_pos: None,
            // Sprint 5-11-1 / H1 PoC: initialized when the actual Window is created in `on_resumed`
            accesskit_adapter: None,
            // Sprint 5-11-2 Step 2-5: throttle + hash comparison for live updates
            last_tree_update_at: None,
            last_tree_hash: None,
            // Sprint 5-11-3: per-pane grid row hash cache
            last_grid_row_hashes: HashMap::new(),
        }
    }
}
