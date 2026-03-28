//! セッション管理 — セッション/ウィンドウのライフサイクルを管理する

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

use nexterm_proto::{ServerToClient, SessionInfo};

use crate::snapshot::{
    ServerSnapshot, SessionSnapshot, SNAPSHOT_VERSION,
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
    /// クライアントへの送信チャネル（アタッチ中は Some）
    pub client_tx: Option<mpsc::Sender<ServerToClient>>,
    /// デフォルトシェル
    shell: String,
    /// デフォルト端末サイズ
    pub cols: u16,
    pub rows: u16,
}

impl Session {
    /// 最初のウィンドウを持つセッションを生成する
    pub fn new(
        name: String,
        cols: u16,
        rows: u16,
        tx: mpsc::Sender<ServerToClient>,
        shell: String,
    ) -> Result<Self> {
        let window_id = new_window_id();
        let window = Window::new(window_id, "window-1".to_string(), cols, rows, tx.clone(), &shell)?;
        let mut windows = HashMap::new();
        windows.insert(window_id, window);

        Ok(Self {
            name,
            windows,
            focused_window_id: window_id,
            client_tx: Some(tx),
            shell,
            cols,
            rows,
        })
    }

    /// セッション情報を返す
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            name: self.name.clone(),
            window_count: self.windows.len() as u32,
            attached: self.client_tx.is_some(),
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

    /// クライアントをアタッチする（再接続）
    pub fn attach(&mut self, tx: mpsc::Sender<ServerToClient>) {
        // 全ウィンドウの全ペインの PTY 出力チャネルを新しいクライアントへ向ける
        for window in self.windows.values() {
            window.update_tx_for_all(&tx);
        }
        self.client_tx = Some(tx);
    }

    /// クライアントをデタッチする
    pub fn detach(&mut self) {
        self.client_tx = None;
    }

    /// アタッチ中かどうかを返す
    pub fn is_attached(&self) -> bool {
        self.client_tx.is_some()
    }

    /// デフォルトシェルを返す
    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// フォーカスウィンドウのフォーカスペインに入力を書き込む
    pub fn write_to_focused(&self, data: &[u8]) -> Result<()> {
        self.focused_window()
            .ok_or_else(|| anyhow::anyhow!("フォーカスウィンドウが見つかりません"))?
            .write_to_focused(data)
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
            cols: self.cols,
            rows: self.rows,
            windows: self.windows.values().map(|w| w.to_snapshot()).collect(),
            focused_window_id: self.focused_window_id,
        }
    }

    /// スナップショットからセッションを復元する
    ///
    /// クライアントは未接続の状態で復元する。
    /// クライアントが接続したときに `attach()` で TX を設定する。
    pub fn restore_from_snapshot(snap: &SessionSnapshot) -> Result<Self> {
        // PTY 出力を受け取る一時チャネル（受信側を即 drop → 送信は無視される）
        let (tx, _rx) = mpsc::channel::<ServerToClient>(64);

        let mut windows = HashMap::new();
        for win_snap in &snap.windows {
            match Window::restore_from_snapshot(win_snap, &tx, &snap.shell, snap.cols, snap.rows) {
                Ok(window) => {
                    windows.insert(win_snap.id, window);
                }
                Err(e) => {
                    warn!(
                        "ウィンドウ '{}' の復元に失敗しました: {}",
                        win_snap.name, e
                    );
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
            client_tx: None,
            shell: snap.shell.clone(),
            cols: snap.cols,
            rows: snap.rows,
        })
    }
}

/// セッションマネージャー（全セッションを管理）
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// セッションへの Arc を返す（IPC ハンドラで使用）
    pub fn sessions(&self) -> Arc<Mutex<HashMap<String, Session>>> {
        Arc::clone(&self.sessions)
    }

    /// 新規セッションを作成する
    pub async fn create_session(
        &self,
        name: String,
        cols: u16,
        rows: u16,
        tx: mpsc::Sender<ServerToClient>,
    ) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(&name) {
            bail!("セッション '{}' は既に存在します", name);
        }
        // デフォルトシェルを決定する（OS 依存）
        let shell = default_shell();
        let session = Session::new(name.clone(), cols, rows, tx, shell)?;
        sessions.insert(name.clone(), session);
        info!("セッション '{}' を作成しました", name);
        Ok(())
    }

    /// 既存セッションにアタッチする
    pub async fn attach_session(
        &self,
        name: &str,
        tx: mpsc::Sender<ServerToClient>,
    ) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("セッション '{}' が見つかりません", name))?;
        session.attach(tx);
        info!("セッション '{}' にアタッチしました", name);
        Ok(())
    }

    /// セッション一覧を返す
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|s| s.info()).collect()
    }

    /// セッションが存在しない場合は新規作成してアタッチ、存在する場合は再アタッチする
    pub async fn get_or_create_and_attach(
        &self,
        name: &str,
        cols: u16,
        rows: u16,
        tx: mpsc::Sender<ServerToClient>,
    ) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(name) {
            session.attach(tx);
            info!("セッション '{}' に再アタッチしました", name);
        } else {
            let shell = default_shell();
            let session = Session::new(name.to_string(), cols, rows, tx, shell)?;
            sessions.insert(name.to_string(), session);
            info!("セッション '{}' を新規作成してアタッチしました", name);
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

    /// セッションのフォーカスペインで録音を開始する（Phase 5-A で完全実装）
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

    // ---- スナップショット ----

    /// 全セッションをスナップショットに変換する
    pub async fn to_snapshot(&self) -> ServerSnapshot {
        let sessions = self.sessions.lock().await;
        let saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        ServerSnapshot {
            version: SNAPSHOT_VERSION,
            sessions: sessions.values().map(|s| s.to_snapshot()).collect(),
            saved_at,
        }
    }

    /// スナップショットから全セッションを復元する
    ///
    /// バージョン不一致や復元エラーのセッションは警告を出してスキップする。
    /// 復元に成功したセッション名のリストを返す。
    pub async fn restore_from_snapshot(&self, snap: &ServerSnapshot) -> Vec<String> {
        if snap.version != SNAPSHOT_VERSION {
            warn!(
                "スナップショットのバージョン不一致（expected={}, got={}）。復元をスキップします",
                SNAPSHOT_VERSION, snap.version
            );
            return Vec::new();
        }

        let mut sessions = self.sessions.lock().await;
        let mut restored = Vec::new();

        for sess_snap in &snap.sessions {
            if sessions.contains_key(&sess_snap.name) {
                info!("セッション '{}' は既に存在するためスキップします", sess_snap.name);
                continue;
            }
            match Session::restore_from_snapshot(sess_snap) {
                Ok(session) => {
                    sessions.insert(sess_snap.name.clone(), session);
                    restored.push(sess_snap.name.clone());
                    info!("セッション '{}' を復元しました", sess_snap.name);
                }
                Err(e) => {
                    warn!("セッション '{}' の復元に失敗しました: {}", sess_snap.name, e);
                }
            }
        }

        restored
    }
}

/// OS に応じたデフォルトシェルを返す
fn default_shell() -> String {
    #[cfg(windows)]
    {
        // PowerShell 7 が優先。なければ Windows PowerShell
        if std::path::Path::new("C:\\Program Files\\PowerShell\\7\\pwsh.exe").exists() {
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe".to_string()
        } else {
            "powershell.exe".to_string()
        }
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shellが空でない() {
        let shell = default_shell();
        assert!(!shell.is_empty());
    }

    #[tokio::test]
    async fn セッション一覧が空で始まる() {
        let manager = SessionManager::new();
        let list = manager.list_sessions().await;
        assert!(list.is_empty());
    }
}
