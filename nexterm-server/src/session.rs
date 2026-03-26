//! セッション管理 — セッション/ウィンドウのライフサイクルを管理する

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use tokio::sync::{mpsc, Mutex};
use tracing::info;

use nexterm_proto::{ServerToClient, SessionInfo};

use crate::window::Window;

static NEXT_WINDOW_ID: AtomicU32 = AtomicU32::new(1);

fn new_window_id() -> u32 {
    NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed)
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
