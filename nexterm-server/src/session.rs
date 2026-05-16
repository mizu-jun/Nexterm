//! セッション管理 — セッション/ウィンドウのライフサイクルを管理する

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Result, bail};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{info, warn};

use nexterm_proto::{ServerToClient, SessionInfo, WindowInfo, WorkspaceInfo};

use crate::snapshot::{
    DEFAULT_WORKSPACE, SNAPSHOT_VERSION, SNAPSHOT_VERSION_MIN, ServerSnapshot, SessionSnapshot,
};
use crate::window::Window;

static NEXT_WINDOW_ID: AtomicU32 = AtomicU32::new(1);

fn new_window_id() -> u32 {
    NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed)
}

/// スナップショット復元後にウィンドウ ID カウンターを更新する
pub fn set_min_window_id(min_id: u32) {
    NEXT_WINDOW_ID.fetch_max(min_id, Ordering::Relaxed);
}

/// セッション
pub struct Session {
    pub name: String,
    /// ウィンドウ一覧（ID → Window）
    windows: HashMap<u32, Window>,
    /// 現在フォーカス中のウィンドウ ID
    focused_window_id: u32,
    /// PTY 出力のブロードキャスト送信チャネル（全クライアントへ同時配信）
    broadcast_tx: broadcast::Sender<ServerToClient>,
    /// デフォルトシェル
    shell: String,
    /// デフォルトシェル引数
    shell_args: Vec<String>,
    /// デフォルト端末サイズ
    pub cols: u16,
    pub rows: u16,
    /// ブロードキャストモードフラグ（全ペインへの入力転送）
    broadcast: bool,
    /// 所属ワークスペース名（Sprint 5-7 / Phase 2-1）。
    /// デフォルトは `"default"`。`SessionManager` 経由で変更する。
    pub workspace_name: String,
}

impl Session {
    /// 最初のウィンドウを持つセッションを生成する（デフォルトワークスペース所属）
    ///
    /// 内部的には `new_in_workspace(.., DEFAULT_WORKSPACE)` を呼ぶ薄いラッパー。
    /// テストおよび後方互換 API のために残している。
    #[allow(dead_code)]
    pub fn new(
        name: String,
        cols: u16,
        rows: u16,
        shell: String,
        shell_args: Vec<String>,
    ) -> Result<Self> {
        Self::new_in_workspace(
            name,
            cols,
            rows,
            shell,
            shell_args,
            DEFAULT_WORKSPACE.to_string(),
        )
    }

    /// 指定ワークスペースに所属するセッションを生成する（Sprint 5-7 / Phase 2-1）
    pub fn new_in_workspace(
        name: String,
        cols: u16,
        rows: u16,
        shell: String,
        shell_args: Vec<String>,
        workspace_name: String,
    ) -> Result<Self> {
        let (broadcast_tx, _) = broadcast::channel::<ServerToClient>(2048);
        let window_id = new_window_id();
        let window = Window::new(
            window_id,
            "window-1".to_string(),
            cols,
            rows,
            broadcast_tx.clone(),
            &shell,
            &shell_args,
        )?;
        let mut windows = HashMap::new();
        windows.insert(window_id, window);

        Ok(Self {
            name,
            windows,
            focused_window_id: window_id,
            broadcast_tx,
            shell,
            shell_args,
            cols,
            rows,
            broadcast: false,
            workspace_name,
        })
    }

    /// セッション情報を返す
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            name: self.name.clone(),
            window_count: self.windows.len() as u32,
            attached: self.broadcast_tx.receiver_count() > 0,
            workspace_name: self.workspace_name.clone(),
        }
    }

    /// フォーカス中のウィンドウへの参照を返す
    pub fn focused_window(&self) -> Option<&Window> {
        self.windows.get(&self.focused_window_id)
    }

    /// フォーカス中のウィンドウへの可変参照を返す
    pub fn focused_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.get_mut(&self.focused_window_id)
    }

    /// クライアントをアタッチする — broadcast::Receiver を返す
    ///
    /// 複数クライアントの同時接続に対応する。PTY 出力は broadcast::Sender 経由で
    /// 全 Receiver に自動配信される。ファンアウトタスクの生成は不要。
    pub fn attach(&self) -> broadcast::Receiver<ServerToClient> {
        self.broadcast_tx.subscribe()
    }

    /// 指定クライアントをデタッチする — broadcast では Receiver を drop するだけでよいため no-op
    pub fn detach_one(&mut self, _tx: &mpsc::Sender<ServerToClient>) {
        // broadcast::Receiver は drop 時に自動的に購読解除される
    }

    /// 全クライアントをデタッチする — broadcast では Receiver を全て drop するだけ（no-op）
    pub fn detach_all(&mut self) {
        // broadcast チャネルは Sender が生きている限り継続する
        // クライアントが全員 Receiver を drop すると receiver_count() が 0 になる
    }

    /// アタッチ中かどうかを返す（broadcast の受信者数で判定）
    #[allow(dead_code)]
    pub fn is_attached(&self) -> bool {
        self.broadcast_tx.receiver_count() > 0
    }

    /// broadcast::Sender を返す（新規ペイン/ウィンドウ生成時に使用）
    pub fn broadcast_sender(&self) -> broadcast::Sender<ServerToClient> {
        self.broadcast_tx.clone()
    }

    /// デフォルトシェルを返す
    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// デフォルトシェル引数を返す
    pub fn shell_args(&self) -> &[String] {
        &self.shell_args
    }

    /// 新しいウィンドウを追加する
    pub fn add_window(&mut self) -> Result<u32> {
        let window_id = new_window_id();
        let name = format!("window-{}", window_id);
        let window = Window::new(
            window_id,
            name,
            self.cols,
            self.rows,
            self.broadcast_tx.clone(),
            &self.shell,
            &self.shell_args,
        )?;
        self.windows.insert(window_id, window);
        self.focused_window_id = window_id;
        Ok(window_id)
    }

    /// 指定ウィンドウを削除する（最後のウィンドウは削除不可）
    pub fn remove_window(&mut self, window_id: u32) -> Result<()> {
        if self.windows.len() <= 1 {
            return Err(anyhow::anyhow!("最後のウィンドウは削除できません"));
        }
        if !self.windows.contains_key(&window_id) {
            return Err(anyhow::anyhow!("ウィンドウ {} が見つかりません", window_id));
        }
        self.windows.remove(&window_id);
        // フォーカスが削除されたウィンドウにあった場合、残ったウィンドウに移す
        if self.focused_window_id == window_id {
            self.focused_window_id = *self
                .windows
                .keys()
                .next()
                .expect("windows が空でないことは len() > 1 チェック済み");
        }
        Ok(())
    }

    /// 指定ウィンドウにフォーカスを移動する
    pub fn focus_window(&mut self, window_id: u32) -> Result<()> {
        if !self.windows.contains_key(&window_id) {
            return Err(anyhow::anyhow!("ウィンドウ {} が見つかりません", window_id));
        }
        self.focused_window_id = window_id;
        Ok(())
    }

    /// 指定ウィンドウをリネームする
    pub fn rename_window(&mut self, window_id: u32, name: String) -> Result<()> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or_else(|| anyhow::anyhow!("ウィンドウ {} が見つかりません", window_id))?;
        window.name = name;
        Ok(())
    }

    /// ウィンドウ情報の一覧を返す
    pub fn window_list(&self) -> Vec<WindowInfo> {
        let mut list: Vec<WindowInfo> = self
            .windows
            .values()
            .map(|w| WindowInfo {
                window_id: w.id,
                name: w.name.clone(),
                pane_count: w.pane_count() as u32,
                is_focused: w.id == self.focused_window_id,
            })
            .collect();
        list.sort_by_key(|w| w.window_id);
        list
    }

    /// フォーカスペインを新しいウィンドウとして切り離す（break-pane）
    ///
    /// 成功した場合は新ウィンドウ ID を返す。
    /// フォーカスウィンドウにペインが 1 つしかない場合は `Err` を返す。
    pub fn break_pane(&mut self) -> Result<u32> {
        let cols = self.cols;
        let rows = self.rows;
        let pane = {
            let w = self
                .focused_window_mut()
                .ok_or_else(|| anyhow::anyhow!("フォーカスウィンドウが見つかりません"))?;
            w.take_focused_pane(cols, rows)
                .ok_or_else(|| anyhow::anyhow!("最後のペインは切り離せません"))?
        };
        let new_window_id = new_window_id();
        let new_window = Window::new_with_pane(new_window_id, "window-broken".to_string(), pane)?;
        self.windows.insert(new_window_id, new_window);
        self.focused_window_id = new_window_id;
        Ok(new_window_id)
    }

    /// フォーカスペインを指定ウィンドウに移動する（join-pane）
    ///
    /// 成功した場合は移動したペイン ID を返す。
    pub fn join_pane(&mut self, target_window_id: u32) -> Result<u32> {
        let cols = self.cols;
        let rows = self.rows;
        // フォーカスウィンドウ ID を退避（borrow checker 対策）
        let focused_win_id = self.focused_window_id;
        if focused_win_id == target_window_id {
            return Err(anyhow::anyhow!("移動先が現在のウィンドウと同じです"));
        }
        // ペインを取り出す
        let pane = {
            let w = self
                .windows
                .get_mut(&focused_win_id)
                .ok_or_else(|| anyhow::anyhow!("フォーカスウィンドウが見つかりません"))?;
            w.take_focused_pane(cols, rows)
                .ok_or_else(|| anyhow::anyhow!("最後のペインは移動できません"))?
        };
        let pane_id = pane.id;
        // 移動先ウィンドウに挿入する
        let target = self
            .windows
            .get_mut(&target_window_id)
            .ok_or_else(|| anyhow::anyhow!("ウィンドウ {} が見つかりません", target_window_id))?;
        target.insert_pane(pane, cols, rows, crate::window::SplitDir::Vertical);
        self.focused_window_id = target_window_id;
        Ok(pane_id)
    }

    /// ブロードキャストモードを設定する
    pub fn set_broadcast(&mut self, enabled: bool) {
        self.broadcast = enabled;
    }

    /// ブロードキャストモードかどうかを返す
    #[allow(dead_code)]
    pub fn is_broadcast(&self) -> bool {
        self.broadcast
    }

    /// ブロードキャストモード: フォーカスウィンドウの全ペインに書き込む
    pub fn write_to_all(&self, data: &[u8]) -> Result<()> {
        let window = self
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("フォーカスウィンドウが見つかりません"))?;
        for pane_id in window.pane_ids() {
            if let Some(pane) = window.pane(pane_id) {
                let _ = pane.write_input(data);
            }
        }
        Ok(())
    }

    /// フォーカスウィンドウのフォーカスペインに入力を書き込む
    pub fn write_to_focused(&self, data: &[u8]) -> Result<()> {
        if self.broadcast {
            self.write_to_all(data)
        } else {
            self.focused_window()
                .ok_or_else(|| anyhow::anyhow!("フォーカスウィンドウが見つかりません"))?
                .write_to_focused(data)
        }
    }

    /// フォーカスペインのブラケットペーストモードが有効かどうかを返す
    pub fn focused_bracketed_paste_mode(&self) -> bool {
        self.focused_window()
            .map(|w| w.focused_bracketed_paste_mode())
            .unwrap_or(false)
    }

    /// フォーカスペインのマウスレポーティングモードを返す（0=無効）
    pub fn focused_mouse_mode(&self) -> u8 {
        self.focused_window()
            .map(|w| w.focused_mouse_mode())
            .unwrap_or(0)
    }

    /// ウィンドウ全体をリサイズする（全ペインを BSP 計算で再配置する）
    pub fn resize_focused(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.cols = cols;
        self.rows = rows;
        let window = self
            .focused_window_mut()
            .ok_or_else(|| anyhow::anyhow!("フォーカスウィンドウが見つかりません"))?;
        window.resize_all_panes(cols, rows);
        Ok(())
    }

    // ---- スナップショット ----

    /// セッションをスナップショットに変換する
    pub fn to_snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            name: self.name.clone(),
            shell: self.shell.clone(),
            shell_args: self.shell_args.clone(),
            cols: self.cols,
            rows: self.rows,
            windows: self.windows.values().map(|w| w.to_snapshot()).collect(),
            focused_window_id: self.focused_window_id,
            session_title: None,
            workspace_name: self.workspace_name.clone(),
        }
    }

    /// スナップショットからセッションを復元する
    ///
    /// クライアントは未接続の状態で復元する。
    /// クライアントが接続したときに `attach()` で TX を設定する。
    pub fn restore_from_snapshot(snap: &SessionSnapshot) -> Result<Self> {
        // broadcast チャネルを生成する（クライアント未接続時は Receiver なし）
        let (broadcast_tx, _) = broadcast::channel::<ServerToClient>(2048);

        let mut windows = HashMap::new();
        for win_snap in &snap.windows {
            match Window::restore_from_snapshot(
                win_snap,
                &broadcast_tx,
                &snap.shell,
                snap.cols,
                snap.rows,
            ) {
                Ok(window) => {
                    windows.insert(win_snap.id, window);
                }
                Err(e) => {
                    warn!("ウィンドウ '{}' の復元に失敗しました: {}", win_snap.name, e);
                }
            }
        }

        if windows.is_empty() {
            bail!(
                "セッション '{}' のウィンドウが 1 つも復元できませんでした",
                snap.name
            );
        }

        Ok(Self {
            name: snap.name.clone(),
            windows,
            focused_window_id: snap.focused_window_id,
            broadcast_tx,
            shell: snap.shell.clone(),
            shell_args: snap.shell_args.clone(),
            cols: snap.cols,
            rows: snap.rows,
            broadcast: false,
            workspace_name: if snap.workspace_name.is_empty() {
                DEFAULT_WORKSPACE.to_string()
            } else {
                snap.workspace_name.clone()
            },
        })
    }
}

/// セッションマネージャー（全セッションを管理）
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    /// デフォルトシェル設定（設定ファイルから読み込む）
    shell_config: nexterm_config::ShellConfig,
    /// WASM プラグインマネージャー（IPC 経由でロード/アンロード操作を受け付ける）
    pub plugin_manager: Arc<std::sync::Mutex<Option<nexterm_plugin::PluginManager>>>,
    /// ワークスペース管理状態（Sprint 5-7 / Phase 2-1）。
    ///
    /// 既知のワークスペース集合（`default` を必ず含む）と、現在アクティブな名前を保持する。
    workspace_state: Arc<Mutex<WorkspaceState>>,
}

/// SessionManager 内部のワークスペース状態
struct WorkspaceState {
    /// 既知のワークスペース名（順序保持のため Vec を使用）
    known: Vec<String>,
    /// 現在アクティブなワークスペース名
    current: String,
}

impl WorkspaceState {
    fn new() -> Self {
        Self {
            known: vec![DEFAULT_WORKSPACE.to_string()],
            current: DEFAULT_WORKSPACE.to_string(),
        }
    }
}

impl SessionManager {
    pub fn new(shell_config: nexterm_config::ShellConfig) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            shell_config,
            plugin_manager: Arc::new(std::sync::Mutex::new(None)),
            workspace_state: Arc::new(Mutex::new(WorkspaceState::new())),
        }
    }

    /// プラグインマネージャーを設定する（サーバー起動時に呼ぶ）
    pub fn set_plugin_manager(&self, mgr: nexterm_plugin::PluginManager) {
        let mut lock = self.plugin_manager.lock().expect("plugin_manager poisoned");
        *lock = Some(mgr);
    }

    /// セッションへの Arc を返す（IPC ハンドラで使用）
    pub fn sessions(&self) -> Arc<Mutex<HashMap<String, Session>>> {
        Arc::clone(&self.sessions)
    }

    /// 新規セッションを作成する
    #[allow(dead_code)]
    pub async fn create_session(&self, name: String, cols: u16, rows: u16) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(&name) {
            bail!("セッション '{}' は既に存在します", name);
        }
        let shell = self.shell_config.program.clone();
        let args = self.shell_config.args.clone();
        let workspace = self.workspace_state.lock().await.current.clone();
        let session = Session::new_in_workspace(name.clone(), cols, rows, shell, args, workspace)?;
        sessions.insert(name.clone(), session);
        info!("セッション '{}' を作成しました", name);
        Ok(())
    }

    /// 既存セッションにアタッチする（broadcast::Receiver を返す）
    #[allow(dead_code)]
    pub async fn attach_session(&self, name: &str) -> Result<broadcast::Receiver<ServerToClient>> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("セッション '{}' が見つかりません", name))?;
        let rx = session.attach();
        info!("セッション '{}' にアタッチしました", name);
        Ok(rx)
    }

    /// セッション一覧を返す
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|s| s.info()).collect()
    }

    /// セッションが存在しない場合は新規作成してアタッチ、存在する場合は再アタッチする
    pub async fn get_or_create_and_attach(&self, name: &str, cols: u16, rows: u16) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(name) {
            info!("セッション '{}' に再アタッチしました", name);
        } else {
            let shell = self.shell_config.program.clone();
            let args = self.shell_config.args.clone();
            let workspace = self.workspace_state.lock().await.current.clone();
            let session =
                Session::new_in_workspace(name.to_string(), cols, rows, shell, args, workspace)?;
            sessions.insert(name.to_string(), session);
            info!("セッション '{}' を新規作成しました", name);
        }
        Ok(())
    }

    /// セッションを強制終了する（Drop によって PTY が閉じられる）
    pub async fn kill_session(&self, name: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if sessions.remove(name).is_some() {
            info!("セッション '{}' を終了しました", name);
            Ok(())
        } else {
            bail!("セッション '{}' が見つかりません", name)
        }
    }

    /// セッションのフォーカスペインで録音を開始する
    pub async fn start_recording(&self, name: &str, path: &str) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("セッション '{}' が見つかりません", name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("ウィンドウが見つかりません"))?;
        let pane_id = window.start_recording(path)?;
        Ok(pane_id)
    }

    /// ログ設定（テンプレート・バイナリログ）を使って録音を開始する
    ///
    /// `log_config.file_name_template` が設定されている場合はテンプレートを展開してファイル名を生成する。
    pub async fn start_recording_with_log_config(
        &self,
        session_name: &str,
        base_dir: &str,
        log_config: &nexterm_config::LogConfig,
    ) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_name)
            .ok_or_else(|| anyhow::anyhow!("セッション '{}' が見つかりません", session_name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("ウィンドウが見つかりません"))?;
        let pane = window
            .pane(window.focused_pane_id())
            .ok_or_else(|| anyhow::anyhow!("フォーカスペインが見つかりません"))?;
        pane.start_recording_with_config(base_dir, session_name, log_config)?;
        Ok(pane.id)
    }

    /// セッションのフォーカスペインで録音を停止する（Phase 5-A で完全実装）
    pub async fn stop_recording(&self, name: &str) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("セッション '{}' が見つかりません", name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("ウィンドウが見つかりません"))?;
        let pane_id = window.stop_recording()?;
        Ok(pane_id)
    }

    /// セッションのフォーカスペインで asciicast 録画を開始する
    pub async fn start_asciicast(&self, name: &str, path: &str) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("セッション '{}' が見つかりません", name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("ウィンドウが見つかりません"))?;
        let pane_id = window.start_asciicast(path)?;
        Ok(pane_id)
    }

    /// セッションのフォーカスペインで asciicast 録画を停止する
    pub async fn stop_asciicast(&self, name: &str) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("セッション '{}' が見つかりません", name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("ウィンドウが見つかりません"))?;
        let pane_id = window.stop_asciicast()?;
        Ok(pane_id)
    }

    /// シリアルポートペインを作成してフォーカスウィンドウに追加する
    pub async fn connect_serial(
        &self,
        session_name: &str,
        port: &str,
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: &str,
    ) -> Result<u32> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_name)
            .ok_or_else(|| anyhow::anyhow!("セッション '{}' が見つかりません", session_name))?;
        let cols = session.cols;
        let rows = session.rows;
        let tx = session.broadcast_sender();
        let window = session
            .focused_window_mut()
            .ok_or_else(|| anyhow::anyhow!("フォーカスウィンドウが見つかりません"))?;
        window.add_serial_pane(
            cols,
            rows,
            tx,
            port,
            baud_rate,
            data_bits,
            stop_bits,
            parity,
            crate::window::SplitDir::Vertical,
        )
    }

    // ---- ワークスペース管理（Sprint 5-7 / Phase 2-1） ----

    /// 現在アクティブなワークスペース名を返す（テスト・将来のフック用）
    #[allow(dead_code)]
    pub async fn current_workspace(&self) -> String {
        self.workspace_state.lock().await.current.clone()
    }

    /// 全ワークスペースの情報を返す（IPC `ListWorkspaces` 用）。
    ///
    /// 各ワークスペースのセッション数は `sessions` の `workspace_name` から集計する。
    /// 既知のワークスペース集合にはセッションが 0 件でも残る（明示削除されるまで保持）。
    pub async fn list_workspaces(&self) -> (String, Vec<WorkspaceInfo>) {
        let state = self.workspace_state.lock().await;
        let sessions = self.sessions.lock().await;

        // セッション数集計
        let mut counts: HashMap<String, u32> = HashMap::new();
        for session in sessions.values() {
            *counts.entry(session.workspace_name.clone()).or_insert(0) += 1;
        }

        let workspaces = state
            .known
            .iter()
            .map(|name| WorkspaceInfo {
                name: name.clone(),
                session_count: counts.get(name).copied().unwrap_or(0),
                is_active: name == &state.current,
            })
            .collect();
        (state.current.clone(), workspaces)
    }

    /// 新しいワークスペースを作成する
    pub async fn create_workspace(&self, name: &str) -> Result<()> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            bail!("ワークスペース名が空です");
        }
        let mut state = self.workspace_state.lock().await;
        if state.known.iter().any(|w| w == trimmed) {
            bail!("ワークスペース '{}' は既に存在します", trimmed);
        }
        state.known.push(trimmed.to_string());
        info!("ワークスペース '{}' を作成しました", trimmed);
        Ok(())
    }

    /// 現在アクティブなワークスペースを切り替える
    ///
    /// 切替後のワークスペース名を返す。
    pub async fn switch_workspace(&self, name: &str) -> Result<String> {
        let mut state = self.workspace_state.lock().await;
        if !state.known.iter().any(|w| w == name) {
            bail!("ワークスペース '{}' が見つかりません", name);
        }
        state.current = name.to_string();
        info!("ワークスペース '{}' に切り替えました", name);
        Ok(state.current.clone())
    }

    /// ワークスペースをリネームする
    ///
    /// 配下のセッションの `workspace_name` も一括更新する。
    pub async fn rename_workspace(&self, from: &str, to: &str) -> Result<()> {
        let to_trimmed = to.trim();
        if to_trimmed.is_empty() {
            bail!("新しいワークスペース名が空です");
        }
        if from == to_trimmed {
            return Ok(());
        }
        let mut state = self.workspace_state.lock().await;
        if !state.known.iter().any(|w| w == from) {
            bail!("ワークスペース '{}' が見つかりません", from);
        }
        if state.known.iter().any(|w| w == to_trimmed) {
            bail!("ワークスペース '{}' は既に存在します", to_trimmed);
        }
        if from == DEFAULT_WORKSPACE {
            bail!(
                "デフォルトワークスペース '{}' はリネームできません",
                DEFAULT_WORKSPACE
            );
        }

        // known 内の名前を置換
        for w in state.known.iter_mut() {
            if w == from {
                *w = to_trimmed.to_string();
            }
        }
        // current も更新
        if state.current == from {
            state.current = to_trimmed.to_string();
        }
        // セッション側の workspace_name も更新
        let mut sessions = self.sessions.lock().await;
        for session in sessions.values_mut() {
            if session.workspace_name == from {
                session.workspace_name = to_trimmed.to_string();
            }
        }
        info!(
            "ワークスペース '{}' を '{}' にリネームしました",
            from, to_trimmed
        );
        Ok(())
    }

    /// ワークスペースを削除する
    ///
    /// `default` は削除不可。配下にセッションがある場合は `force=true` で default に
    /// 退避させて削除を強行する。削除したワークスペースが current の場合、
    /// current は default に戻る。
    pub async fn delete_workspace(&self, name: &str, force: bool) -> Result<()> {
        if name == DEFAULT_WORKSPACE {
            bail!(
                "デフォルトワークスペース '{}' は削除できません",
                DEFAULT_WORKSPACE
            );
        }
        let mut state = self.workspace_state.lock().await;
        if !state.known.iter().any(|w| w == name) {
            bail!("ワークスペース '{}' が見つかりません", name);
        }
        // 配下セッション数チェック
        let mut sessions = self.sessions.lock().await;
        let session_count = sessions
            .values()
            .filter(|s| s.workspace_name == name)
            .count();
        if session_count > 0 && !force {
            bail!(
                "ワークスペース '{}' にはセッションが {} 件残っています。force=true で再試行してください",
                name,
                session_count
            );
        }
        // force=true: セッションを default に退避
        if force {
            for session in sessions.values_mut() {
                if session.workspace_name == name {
                    session.workspace_name = DEFAULT_WORKSPACE.to_string();
                }
            }
        }
        // known から除去
        state.known.retain(|w| w != name);
        // current だった場合は default に戻す
        if state.current == name {
            state.current = DEFAULT_WORKSPACE.to_string();
        }
        info!(
            "ワークスペース '{}' を削除しました（force={}）",
            name, force
        );
        Ok(())
    }

    // ---- スナップショット ----

    /// 全セッションをスナップショットに変換する
    pub async fn to_snapshot(&self) -> ServerSnapshot {
        let sessions = self.sessions.lock().await;
        let current_workspace = self.workspace_state.lock().await.current.clone();
        let saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        ServerSnapshot {
            version: SNAPSHOT_VERSION,
            sessions: sessions.values().map(|s| s.to_snapshot()).collect(),
            saved_at,
            current_workspace,
        }
    }

    /// スナップショットから全セッションを復元する
    ///
    /// バージョン不一致や復元エラーのセッションは警告を出してスキップする。
    /// 復元に成功したセッション名のリストを返す。
    pub async fn restore_from_snapshot(&self, snap: &ServerSnapshot) -> Vec<String> {
        // persist::load_snapshot() でマイグレーション済みのため、ここでは MIN〜MAX の範囲チェックのみ
        if snap.version < SNAPSHOT_VERSION_MIN || snap.version > SNAPSHOT_VERSION {
            warn!(
                "スナップショットのバージョンがサポート範囲外（got={}, supported={}〜{}）。復元をスキップします",
                snap.version, SNAPSHOT_VERSION_MIN, SNAPSHOT_VERSION
            );
            return Vec::new();
        }

        let mut sessions = self.sessions.lock().await;
        let mut restored = Vec::new();
        // 復元したセッションのワークスペース集合（既知集合に追加するため）
        let mut restored_workspaces: Vec<String> = Vec::new();

        for sess_snap in &snap.sessions {
            if sessions.contains_key(&sess_snap.name) {
                info!(
                    "セッション '{}' は既に存在するためスキップします",
                    sess_snap.name
                );
                continue;
            }
            match Session::restore_from_snapshot(sess_snap) {
                Ok(session) => {
                    let ws = session.workspace_name.clone();
                    if !ws.is_empty() && !restored_workspaces.contains(&ws) {
                        restored_workspaces.push(ws);
                    }
                    sessions.insert(sess_snap.name.clone(), session);
                    restored.push(sess_snap.name.clone());
                    info!("セッション '{}' を復元しました", sess_snap.name);
                }
                Err(e) => {
                    warn!(
                        "セッション '{}' の復元に失敗しました: {}",
                        sess_snap.name, e
                    );
                }
            }
        }
        drop(sessions);

        // ワークスペース状態を復元
        let mut state = self.workspace_state.lock().await;
        for ws in restored_workspaces {
            if !state.known.iter().any(|w| w == &ws) {
                state.known.push(ws);
            }
        }
        // current_workspace を復元（既知集合にあれば採用、なければ default のまま）
        if !snap.current_workspace.is_empty()
            && state.known.iter().any(|w| w == &snap.current_workspace)
        {
            state.current = snap.current_workspace.clone();
        }

        restored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_configデフォルトが空でない() {
        let cfg = nexterm_config::ShellConfig::default();
        assert!(!cfg.program.is_empty());
    }

    #[tokio::test]
    async fn セッション一覧が空で始まる() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let list = manager.list_sessions().await;
        assert!(list.is_empty());
    }

    #[tokio::test]
    #[allow(non_snake_case)]
    async fn セッション取得で存在しない名前はNone() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let arc = manager.sessions();
        let sessions = arc.lock().await;
        assert!(sessions.get("nonexistent").is_none());
    }

    #[tokio::test]
    #[allow(non_snake_case)]
    async fn セッション削除で存在しない名前はErr() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let result = manager.kill_session("nonexistent").await;
        assert!(
            result.is_err(),
            "存在しないセッションの kill は Err を返すべき"
        );
    }

    #[tokio::test]
    async fn セッション一覧が初期状態では空() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let list = manager.list_sessions().await;
        assert_eq!(list.len(), 0, "初期状態では空のリストを返すべき");
    }

    // ---- ワークスペース API ユニットテスト（PTY 不要） ----

    #[tokio::test]
    async fn ワークスペース初期状態() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        assert_eq!(manager.current_workspace().await, DEFAULT_WORKSPACE);
        let (current, list) = manager.list_workspaces().await;
        assert_eq!(current, DEFAULT_WORKSPACE);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, DEFAULT_WORKSPACE);
        assert!(list[0].is_active);
        assert_eq!(list[0].session_count, 0);
    }

    #[tokio::test]
    async fn ワークスペース作成と切替() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("dev").await.unwrap();

        let (_, list) = manager.list_workspaces().await;
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|w| w.name == "dev"));

        manager.switch_workspace("dev").await.unwrap();
        assert_eq!(manager.current_workspace().await, "dev");

        // 存在しないワークスペースへの切替はエラー
        assert!(manager.switch_workspace("unknown").await.is_err());
    }

    #[tokio::test]
    async fn ワークスペース重複作成はエラー() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("dev").await.unwrap();
        assert!(manager.create_workspace("dev").await.is_err());
        assert!(manager.create_workspace("").await.is_err());
        assert!(manager.create_workspace("   ").await.is_err());
    }

    #[tokio::test]
    async fn ワークスペース削除() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("tmp").await.unwrap();
        manager.delete_workspace("tmp", false).await.unwrap();
        let (_, list) = manager.list_workspaces().await;
        assert_eq!(list.len(), 1);

        // default は削除不可
        assert!(
            manager
                .delete_workspace(DEFAULT_WORKSPACE, true)
                .await
                .is_err()
        );

        // 存在しないワークスペース削除はエラー
        assert!(manager.delete_workspace("ghost", false).await.is_err());
    }

    #[tokio::test]
    async fn ワークスペース削除でアクティブが_default_に戻る() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("dev").await.unwrap();
        manager.switch_workspace("dev").await.unwrap();
        manager.delete_workspace("dev", false).await.unwrap();
        assert_eq!(manager.current_workspace().await, DEFAULT_WORKSPACE);
    }

    #[tokio::test]
    async fn ワークスペースリネーム() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("old").await.unwrap();
        manager.switch_workspace("old").await.unwrap();
        manager.rename_workspace("old", "new").await.unwrap();
        assert_eq!(manager.current_workspace().await, "new");
        let (_, list) = manager.list_workspaces().await;
        assert!(list.iter().any(|w| w.name == "new"));
        assert!(!list.iter().any(|w| w.name == "old"));

        // default のリネームは禁止
        assert!(
            manager
                .rename_workspace(DEFAULT_WORKSPACE, "x")
                .await
                .is_err()
        );

        // 存在しない名前のリネームはエラー
        assert!(manager.rename_workspace("ghost", "y").await.is_err());

        // 既存名との衝突はエラー
        manager.create_workspace("a").await.unwrap();
        manager.create_workspace("b").await.unwrap();
        assert!(manager.rename_workspace("a", "b").await.is_err());
    }
    // ── 実 PTY を spawn するテスト ────────────────────────────────────────────
    //
    // 以下 4 つのテストは実際にシェル (PowerShell / $SHELL) を spawn し、
    // テスト終了時に `Pane` の Drop が `MasterPty` を閉じる。
    //
    // 対話モードのシェルは終了コマンドを受け取らないため close 待ちが永遠に
    // 続き、`#[tokio::test]` の runtime が blocking task を待つ間にテストが
    // ハングする (Windows ConPTY / Linux pty 共に観測済み、macOS のみ偶然
    // EOF が早く伝播して通る環境がある)。
    //
    // CI を緑にする目的では `#[ignore]` で除外し、ローカル/手動検証時のみ
    // `cargo test --workspace --all-targets -- --include-ignored` で走らせる。
    //
    // 抜本対策は portable-pty の Drop ハングを回避する仕組み (e.g. 明示的
    // kill_child API の導入や別プロセス分離) を入れた後で `#[ignore]` を外す。

    #[tokio::test]
    #[ignore = "PTY を spawn する。対話シェルの close 待ちでハングするため通常 CI では skip"]
    async fn session_new_creates_valid_session() {
        let shell = nexterm_config::ShellConfig::default();
        let session = Session::new(
            "test-session".to_string(),
            80,
            24,
            shell.program,
            shell.args,
        )
        .unwrap();

        assert_eq!(session.name, "test-session");
        assert_eq!(session.cols, 80);
        assert_eq!(session.rows, 24);
        assert_eq!(session.windows.len(), 1);
        assert!(!session.broadcast);
    }

    #[tokio::test]
    #[ignore = "PTY を spawn する。対話シェルの close 待ちでハングするため通常 CI では skip"]
    async fn session_info_returns_correct_metadata() {
        let shell = nexterm_config::ShellConfig::default();
        let session = Session::new("test".to_string(), 80, 24, shell.program, shell.args).unwrap();

        let info = session.info();
        assert_eq!(info.name, "test");
        assert_eq!(info.window_count, 1);
        assert!(!info.attached);
        assert_eq!(info.workspace_name, DEFAULT_WORKSPACE);
    }

    #[tokio::test]
    #[ignore = "PTY を spawn する。対話シェルの close 待ちでハングするため通常 CI では skip"]
    async fn session_manager_create_new_session() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());

        let result = manager
            .get_or_create_and_attach("new-session", 80, 24)
            .await;
        assert!(result.is_ok());

        let list = manager.list_sessions().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "new-session");
    }

    #[tokio::test]
    #[ignore = "PTY を spawn する。対話シェルの close 待ちでハングするため通常 CI では skip"]
    async fn session_manager_kill_existing_session() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager
            .get_or_create_and_attach("to-kill", 80, 24)
            .await
            .unwrap();

        assert_eq!(manager.list_sessions().await.len(), 1);

        let result = manager.kill_session("to-kill").await;
        assert!(result.is_ok());

        assert_eq!(manager.list_sessions().await.len(), 0);
    }
}
