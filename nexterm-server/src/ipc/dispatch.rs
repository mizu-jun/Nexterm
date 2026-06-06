//! IPC message dispatch — routes `ClientToServer` messages to feature modules.
//!
//! Actual handlers are split across the following modules:
//! - `session_dispatch` — session management (Attach/Detach/ListSessions/...).
//! - `pane_dispatch`    — pane operations (KeyEvent/Focus*/floating, ...).
//! - `window_dispatch`  — window operations (Resize/Split/NewWindow/...).
//! - `file_dispatch`    — templates / SSH / SFTP / macros / serial ports.
//! - `plugin_dispatch`  — plugin management.
//!
//! Every handler takes `&mut DispatchContext<'_>` to access the required dependencies.

use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::sync::mpsc;
use tracing::instrument;

use super::{file_dispatch, pane_dispatch, session_dispatch, window_dispatch};
use crate::session::SessionManager;
use crate::window::SplitDir;

/// Context shared across the dispatch layer.
///
/// Bundles nine arguments into one struct to keep handler signatures concise.
/// `current_session` and `bcast_forwarder` are per-connection mutable state.
/// The rest are configuration snapshots shared across connections.
pub(super) struct DispatchContext<'a> {
    pub manager: &'a SessionManager,
    pub tx: mpsc::Sender<ServerToClient>,
    pub current_session: &'a mut Option<String>,
    pub hooks: &'a nexterm_config::HooksConfig,
    pub lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    pub log_config: &'a nexterm_config::LogConfig,
    pub hosts: &'a [nexterm_config::HostConfig],
    pub bcast_forwarder: &'a mut Option<tokio::task::AbortHandle>,
}

/// Dispatch a message from the client.
#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch(
    msg: &ClientToServer,
    manager: &SessionManager,
    tx: mpsc::Sender<ServerToClient>,
    current_session: &mut Option<String>,
    hooks: &nexterm_config::HooksConfig,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    log_config: &nexterm_config::LogConfig,
    hosts: &[nexterm_config::HostConfig],
    bcast_forwarder: &mut Option<tokio::task::AbortHandle>,
) {
    let mut ctx = DispatchContext {
        manager,
        tx,
        current_session,
        hooks,
        lua,
        log_config,
        hosts,
        bcast_forwarder,
    };
    dispatch_inner(msg, &mut ctx).await;
}

/// Internal dispatcher that takes `DispatchContext` — callable directly from feature modules.
///
/// The `msg` payload may contain secrets (passwords etc.), so `skip_all` keeps it out of the span.
/// Variant identification is delegated to per-handler logging.
#[instrument(name = "ipc_dispatch", skip_all)]
pub(super) async fn dispatch_inner(msg: &ClientToServer, ctx: &mut DispatchContext<'_>) {
    use ClientToServer::*;

    match msg {
        // ----- session_dispatch -----
        Ping => session_dispatch::handle_ping(ctx).await,
        Hello { .. } => session_dispatch::handle_hello(),
        Attach { session_name } => session_dispatch::handle_attach(ctx, session_name).await,
        Detach => session_dispatch::handle_detach(ctx).await,
        ListSessions => session_dispatch::handle_list_sessions(ctx).await,
        KillSession { name } => session_dispatch::handle_kill_session(ctx, name).await,
        StartRecording {
            session_name,
            output_path,
        } => session_dispatch::handle_start_recording(ctx, session_name, output_path).await,
        StopRecording { session_name } => {
            session_dispatch::handle_stop_recording(ctx, session_name).await
        }
        StartAsciicast {
            session_name,
            output_path,
        } => session_dispatch::handle_start_asciicast(ctx, session_name, output_path).await,
        StopAsciicast { session_name } => {
            session_dispatch::handle_stop_asciicast(ctx, session_name).await
        }
        SetBroadcast { enabled } => session_dispatch::handle_set_broadcast(ctx, *enabled).await,
        DisplayPanes { .. } => session_dispatch::handle_display_panes(),
        ListWorkspaces => session_dispatch::handle_list_workspaces(ctx).await,
        CreateWorkspace { name } => session_dispatch::handle_create_workspace(ctx, name).await,
        SwitchWorkspace { name } => session_dispatch::handle_switch_workspace(ctx, name).await,
        RenameWorkspace { from, to } => {
            session_dispatch::handle_rename_workspace(ctx, from, to).await
        }
        DeleteWorkspace { name, force } => {
            session_dispatch::handle_delete_workspace(ctx, name, *force).await
        }
        QuakeToggle { action } => session_dispatch::handle_quake_toggle(ctx, action).await,

        // ----- pane_dispatch -----
        KeyEvent {
            code,
            modifiers,
            event_type,
        } => pane_dispatch::handle_key_event(ctx, code, *modifiers, *event_type).await,
        PasteText { text } => pane_dispatch::handle_paste_text(ctx, text).await,
        MouseReport {
            button,
            col,
            row,
            pressed,
            motion,
        } => pane_dispatch::handle_mouse_report(ctx, *button, *col, *row, *pressed, *motion).await,
        FocusNextPane => pane_dispatch::handle_focus_next_pane(ctx).await,
        FocusPrevPane => pane_dispatch::handle_focus_prev_pane(ctx).await,
        FocusPane { pane_id } => pane_dispatch::handle_focus_pane(ctx, *pane_id).await,
        ClosePane => pane_dispatch::handle_close_pane(ctx).await,
        ToggleZoom => pane_dispatch::handle_toggle_zoom(ctx).await,
        SwapPane { target_pane_id } => pane_dispatch::handle_swap_pane(ctx, *target_pane_id).await,
        ReorderPanes { pane_ids } => pane_dispatch::handle_reorder_panes(ctx, pane_ids).await,
        MovePaneToWindow {
            pane_id,
            target_window_id,
            insert_at,
        } => {
            pane_dispatch::handle_move_pane_to_window(ctx, *pane_id, *target_window_id, *insert_at)
                .await
        }
        QueryForegroundProcess { window_id } => {
            window_dispatch::handle_query_foreground_process(ctx, *window_id).await
        }
        BreakPane => pane_dispatch::handle_break_pane(ctx).await,
        JoinPane { target_window_id } => {
            pane_dispatch::handle_join_pane(ctx, *target_window_id).await
        }
        OpenFloatingPane => pane_dispatch::handle_open_floating_pane(ctx).await,
        CloseFloatingPane { pane_id } => {
            pane_dispatch::handle_close_floating_pane(ctx, *pane_id).await
        }
        MoveFloatingPane {
            pane_id,
            col_off,
            row_off,
        } => pane_dispatch::handle_move_floating_pane(ctx, *pane_id, *col_off, *row_off).await,
        ResizeFloatingPane {
            pane_id,
            cols,
            rows,
        } => pane_dispatch::handle_resize_floating_pane(ctx, *pane_id, *cols, *rows).await,

        // ----- window_dispatch -----
        Resize { cols, rows } => window_dispatch::handle_resize(ctx, *cols, *rows).await,
        SplitVertical => window_dispatch::handle_split(ctx, SplitDir::Vertical).await,
        SplitHorizontal => window_dispatch::handle_split(ctx, SplitDir::Horizontal).await,
        ResizeSplit { delta } => window_dispatch::handle_resize_split(ctx, *delta).await,
        NewWindow => window_dispatch::handle_new_window(ctx).await,
        CloseWindow { window_id } => window_dispatch::handle_close_window(ctx, *window_id).await,
        FocusWindow { window_id } => window_dispatch::handle_focus_window(ctx, *window_id).await,
        RenameWindow {
            window_id,
            name: new_name,
        } => window_dispatch::handle_rename_window(ctx, *window_id, new_name).await,
        SetLayoutMode { mode } => window_dispatch::handle_set_layout_mode(ctx, mode).await,

        // ----- file_dispatch -----
        SaveTemplate { name } => file_dispatch::handle_save_template(ctx, name).await,
        LoadTemplate { name } => file_dispatch::handle_load_template(ctx, name).await,
        ListTemplates => file_dispatch::handle_list_templates(ctx).await,
        ConnectSsh {
            host,
            port,
            username,
            auth_type,
            password_keyring_account,
            ephemeral_password,
            key_path,
            remote_forwards,
            x11_forward: _,
            x11_trusted: _,
        } => {
            file_dispatch::handle_connect_ssh(
                ctx,
                host,
                *port,
                username,
                auth_type,
                password_keyring_account,
                *ephemeral_password,
                key_path,
                remote_forwards,
            )
            .await
        }
        SftpUpload {
            host_name,
            local_path,
            remote_path,
        } => file_dispatch::handle_sftp_upload(ctx, host_name, local_path, remote_path).await,
        SftpDownload {
            host_name,
            remote_path,
            local_path,
        } => file_dispatch::handle_sftp_download(ctx, host_name, remote_path, local_path).await,
        RunMacro {
            macro_fn,
            display_name,
        } => file_dispatch::handle_run_macro(ctx, macro_fn, display_name).await,
        ConnectSerial {
            port,
            baud_rate,
            data_bits,
            stop_bits,
            parity,
        } => {
            file_dispatch::handle_connect_serial(
                ctx, port, *baud_rate, *data_bits, *stop_bits, parity,
            )
            .await
        }

        // ----- plugin_dispatch (existing) -----
        ListPlugins => super::plugin_dispatch::handle_list_plugins(ctx.manager, &ctx.tx).await,
        LoadPlugin { path } => {
            super::plugin_dispatch::handle_load_plugin(ctx.manager, &ctx.tx, path).await
        }
        UnloadPlugin { path } => {
            super::plugin_dispatch::handle_unload_plugin(ctx.manager, &ctx.tx, path).await
        }
        ReloadPlugin { path } => {
            super::plugin_dispatch::handle_reload_plugin(ctx.manager, &ctx.tx, path).await
        }
    }
}
