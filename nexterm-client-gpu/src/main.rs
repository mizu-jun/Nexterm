#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! nexterm entry point — single-binary wgpu + winit desktop client.
//!
//! Runs nexterm-server's logic on its own OS thread with a dedicated Tokio
//! runtime (Sprint 5-13 / v1.7.7). The previous design spawned the server as
//! a `tokio::task` on the same runtime that drove winit, which meant winit's
//! main-thread occupation could starve the server task for seconds at a time
//! on lower-core machines. See
//! `memory/project_windows_powershell_startup_investigation.md` problem 3.

// Sprint 5-11-1 / H1 PoC: scaffolding for screen-reader support (accesskit node tree).
mod accessibility;
mod animations;
mod color_util;
mod command_blocks;
mod connection;
mod cursor_motion;
mod drop_target;
mod font;
mod glyph_atlas;
mod host_manager;
mod key_map;
mod macro_picker;
mod named_blocks;
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
mod tab_icons;
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

    // Load the config (TOML → Lua) BEFORE spawning the server thread so we can
    // hand the parsed `Config` and a `SharedRuntimeConfig` to it. Previously
    // the client and the embedded server each called `ConfigLoader::load()`
    // independently — visible in `nexterm-client.log.2026-06-05` as two
    // `Loaded the TOML configuration` lines fired within microseconds of each
    // other, plus a duplicated `Watching the configuration directory`. The
    // redundant read doubled startup file IO; the duplicated watcher kept
    // both file-system handles open for the lifetime of the process.
    let config = ConfigLoader::load()?;

    // Sprint 5-13 / v1.7.7 problem 2: the client owns the `SharedRuntimeConfig`
    // and the `notify::Watcher`. The embedded server reuses the same shared
    // handle (no second watcher inside the server).
    let runtime_cfg = nexterm_server::build_shared_runtime_config(&config);

    // Sprint 5-13 / v1.7.7 problem 3: spawn the server on a dedicated OS thread
    // with its own multi-thread Tokio runtime so winit's main-thread occupation
    // cannot starve the server task. The previous `tokio::spawn` on the same
    // runtime caused a ~1.6 s stall between `restored sessions` and
    // `ipc::serve` on systems where worker threads were busy with winit / wgpu
    // init (see `nexterm-client.log.2026-06-05`).
    let server_config = config.clone();
    let server_runtime_cfg = std::sync::Arc::clone(&runtime_cfg);
    let (server_shutdown_tx, server_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_thread = std::thread::Builder::new()
        .name("nexterm-server".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("nexterm-server-worker")
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("failed to build server Tokio runtime: {}", e);
                    return;
                }
            };
            if let Err(e) = rt.block_on(nexterm_server::run_server_with_config_and_runtime(
                server_config,
                server_runtime_cfg,
                server_shutdown_rx,
            )) {
                tracing::error!("nexterm-server error: {}", e);
            }
        })?;

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
    //
    // This is now the ONLY `notify::Watcher` in the process — the embedded
    // server skips its own `spawn_watcher` when a `SharedRuntimeConfig` is
    // supplied. The event_handler forwards each `Config` it receives both to
    // its UI reload logic AND to `runtime_cfg.store(...)` so the server's
    // dispatch layer sees the change.
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
        Some(server_shutdown_tx),
        Some(runtime_cfg),
        update_rx,
    ))?;

    // The event loop returned (event_loop.exit() was called). The event handler
    // should have already sent `()` on `server_shutdown_tx`, so this join will
    // complete once the server finishes saving its snapshot and tearing down.
    if let Err(panic) = server_thread.join() {
        tracing::warn!("server thread panicked: {:?}", panic);
    }

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
