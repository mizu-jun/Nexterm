//! IPC メッセージディスパッチ — ClientToServer メッセージを各機能モジュールへルーティングする
//!
//! 実際のハンドラは以下のモジュールに分散している:
//! - `session_dispatch` — セッション管理 (Attach/Detach/ListSessions/...)
//! - `pane_dispatch`    — ペイン操作 (KeyEvent/Focus*/フローティング系)
//! - `window_dispatch`  — ウィンドウ操作 (Resize/Split/NewWindow/...)
//! - `file_dispatch`    — テンプレート / SSH / SFTP / Macro / Serial
//! - `plugin_dispatch`  — プラグイン管理
//!
//! 各ハンドラは `&mut DispatchContext<'_>` を受け取り、必要な依存にアクセスする。

use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::sync::mpsc;
use tracing::instrument;

use super::{file_dispatch, pane_dispatch, session_dispatch, window_dispatch};
use crate::session::SessionManager;
use crate::window::SplitDir;

/// ディスパッチ層全体で共有する文脈情報
///
/// 9 個の引数を 1 構造体に集約してシグネチャを簡潔にする。
/// `current_session` と `bcast_forwarder` は接続単位の可変状態。
/// その他は接続をまたいで共有される設定スナップショット。
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

/// クライアントからのメッセージをディスパッチする
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

/// `DispatchContext` を引数とする内部ディスパッチャ — 各機能モジュールから直接呼び出せる
///
/// `msg` のペイロードはパスワード等の機密を含み得るため `skip_all` で span に乗せない。
/// バリアント識別は個別ハンドラ側のログ出力に委ねる。
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
        KeyEvent { code, modifiers } => {
            pane_dispatch::handle_key_event(ctx, code, *modifiers).await
        }
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

        // ----- plugin_dispatch (既存) -----
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
