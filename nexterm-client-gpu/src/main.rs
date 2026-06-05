#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! nexterm entry point — single-binary wgpu + winit desktop client.
//!
//! Runs nexterm-server's logic as an internal Tokio task so the full feature
//! set is available in one process.

// Sprint 5-11-1 / H1 PoC: scaffolding for screen-reader support (accesskit node tree).
mod accessibility;
mod animations;
mod color_util;
mod connection;
mod drop_target;
mod font;
mod glyph_atlas;
mod host_manager;
mod key_map;
mod macro_picker;
mod notification;
mod palette;
mod platform;
mod quake;
mod renderer;
mod scrollback;
mod settings_panel;
mod shaders;
mod signature_verify;
mod state;
mod update_checker;
mod vertex_util;

use anyhow::Result;
use nexterm_config::{ConfigLoader, StatusBarEvaluator, watch_config};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use winit::event_loop::EventLoop;

use crate::renderer::UserEvent;

#[tokio::main]
async fn main() -> Result<()> {
    let _log_guard = init_tracing();

    // Load the config (TOML → Lua) BEFORE spawning the server task so we can
    // hand the parsed `Config` to it via `run_server_with_config`. Previously
    // the client and the embedded server each called `ConfigLoader::load()`
    // independently — visible in `nexterm-client.log.2026-06-05` as two
    // `Loaded the TOML configuration` log lines fired within microseconds of
    // each other. The redundant read doubled startup file IO.
    let config = ConfigLoader::load()?;

    // Start the server as an internal Tokio task (no separate process needed).
    // The IPC socket uses the same protocol regardless.
    let server_config = config.clone();
    let server_handle = tokio::spawn(async move {
        if let Err(e) = nexterm_server::run_server_with_config(server_config).await {
            tracing::error!("nexterm-server error: {}", e);
        }
    });
    // The config's `language` field wins; "auto" falls back to OS locale detection.
    if config.language == "auto" {
        nexterm_i18n::init();
    } else {
        nexterm_i18n::set_locale(&config.language);
    }

    info!(
        "Config loaded: font={} {}pt",
        config.font.family, config.font.size
    );

    // Start the hot-reload config watcher.
    let (config_tx, config_rx) = mpsc::channel(8);
    let config_watcher = watch_config(config_tx).ok();

    // Lua status-bar evaluator (only constructed when status_bar.enabled is true).
    let status_eval = if config.status_bar.enabled {
        Some(StatusBarEvaluator::new())
    } else {
        None
    };

    // Start the background update checker (polls the GitHub Releases API after 5 s).
    let update_rx = update_checker::start(env!("CARGO_PKG_VERSION"), config.auto_check_update);

    // Build the winit event loop with the UserEvent type (Sprint 5-8 Phase 4-4).
    // `EventLoopProxy<UserEvent>` lets contexts without a `&ActiveEventLoop`
    // (e.g. mouse handlers or the network receive thread) request OS-window
    // operations.
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    // Start the GPU application.
    let app = renderer::NextermApp::new(config).await?;
    event_loop.run_app(&mut app.into_event_handler(
        proxy,
        Some(config_rx),
        config_watcher,
        status_eval,
        server_handle,
        update_rx,
    ))?;

    Ok(())
}

/// Default tracing directive used when `NEXTERM_LOG` is unset.
///
/// `info` keeps nexterm's own logs visible. The remaining overrides silence
/// per-frame log floods from upstream wgpu / naga:
///
/// - `wgpu_core=warn` muzzles the
///   `wgpu_core::device::resource: Device::maintain: waiting for submission
///   index N` flood emitted at INFO every frame (≈60 Hz).
/// - `wgpu_hal=warn` / `naga=warn` keep the rest of those crates terse.
/// - `wgpu_hal::vulkan::conv=error` silences the
///   `Unrecognized present mode 1000361000` (VK_PRESENT_MODE_FIFO_LATEST_READY_EXT)
///   WARN that newer NVIDIA Vulkan drivers emit on every frame because wgpu
///   has not added the enum yet. The targeted `=error` keeps the rest of
///   `wgpu_hal`'s legitimate WARNs visible.
/// - `fontdb=error` silences the `Failed to load a font face ... malformed font`
///   WARN that fontdb emits for proprietary/broken system fonts (e.g. Windows'
///   `mstmc.ttf`). These are unactionable third-party fonts we cannot parse and
///   the warning is pure noise.
///
/// Without these overrides a 4-minute session bloats the client log file past
/// 1 MB and drowns out useful diagnostics.
const DEFAULT_LOG_DIRECTIVES: &str =
    "info,wgpu_core=warn,wgpu_hal=warn,wgpu_hal::vulkan::conv=error,naga=warn,fontdb=error";

/// Initialize logging. Windows release builds log to a file
/// (`%LOCALAPPDATA%\nexterm\nexterm-client.log`); all other configurations log to stdout.
#[cfg(all(windows, not(debug_assertions)))]
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("nexterm");
    std::fs::create_dir_all(&log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "nexterm-client.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("NEXTERM_LOG")
                .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_DIRECTIVES)),
        )
        .with_writer(non_blocking)
        .init();
    Some(guard)
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("NEXTERM_LOG")
                .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_DIRECTIVES)),
        )
        .init();
    None
}
