//! ペイン — PTY プロセスと仮想グリッドを管理する最小単位
//!
//! PTY 出力チャネルは `Arc<Mutex<Sender>>` で保持し、
//! クライアント再アタッチ時に `update_tx` で差し替えられる。

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use nexterm_proto::{Grid, ServerToClient};
use nexterm_vt::VtParser;

static NEXT_PANE_ID: AtomicU32 = AtomicU32::new(1);

/// PTY 出力のログライター（録音中のみ Some）
type LogWriter = Arc<Mutex<Option<BufWriter<File>>>>;

/// ペイン ID を新規発行する
pub fn new_pane_id() -> u32 {
    NEXT_PANE_ID.fetch_add(1, Ordering::Relaxed)
}

/// スナップショット復元後に ID カウンターを更新する
///
/// 復元したペインの最大 ID + 1 以上になるよう調整して ID 衝突を防ぐ。
pub fn set_min_pane_id(min_id: u32) {
    NEXT_PANE_ID.fetch_max(min_id, Ordering::Relaxed);
}

/// 動的に差し替え可能な送信チャネル
type SharedTx = Arc<Mutex<mpsc::Sender<ServerToClient>>>;

/// ペインの状態
pub struct Pane {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    /// 子プロセスの PID（Linux: /proc/{pid}/cwd から作業ディレクトリ取得に使用）
    #[allow(dead_code)]
    pid: Option<u32>,
    /// PTY 出力先チャネル（再アタッチ時に差し替え可能）
    shared_tx: SharedTx,
    /// PTY マスタ（リサイズ用）
    master: Box<dyn MasterPty + Send>,
    /// PTY 書き込みハンドル（キー入力転送用）
    writer: Mutex<Box<dyn Write + Send>>,
    /// ログファイルライター（録音中のみ Some）
    log_writer: LogWriter,
}

impl Pane {
    /// 新しいペインを生成してシェルを起動する
    pub fn spawn(
        cols: u16,
        rows: u16,
        initial_tx: mpsc::Sender<ServerToClient>,
        shell: &str,
    ) -> Result<Self> {
        Self::spawn_impl(new_pane_id(), cols, rows, initial_tx, shell, None)
    }

    /// 指定 ID でペインを生成する（BSP 分割時に ID を事前確定するために使用）
    pub fn spawn_with_id(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: mpsc::Sender<ServerToClient>,
        shell: &str,
    ) -> Result<Self> {
        Self::spawn_impl(id, cols, rows, initial_tx, shell, None)
    }

    /// 指定 ID・作業ディレクトリでペインを生成する（スナップショット復元時に使用）
    pub fn spawn_with_cwd(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: mpsc::Sender<ServerToClient>,
        shell: &str,
        cwd: &Path,
    ) -> Result<Self> {
        Self::spawn_impl(id, cols, rows, initial_tx, shell, Some(cwd))
    }

    /// 内部 PTY 起動実装（CWD はオプション）
    fn spawn_impl(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: mpsc::Sender<ServerToClient>,
        shell: &str,
        cwd: Option<&Path>,
    ) -> Result<Self> {
        let pty_system = NativePtySystem::default();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }

        let child = pair.slave.spawn_command(cmd)?;
        // 子プロセスの PID を保存する（child を drop してもプロセスは継続する）
        let pid = child.process_id();

        // 書き込みハンドル（1 度だけ取得可）と読み取りハンドルを取得する
        let writer = Mutex::new(pair.master.take_writer()?);
        let mut reader = pair.master.try_clone_reader()?;
        let master = pair.master;

        // 動的チャネルを Arc<Mutex> で共有する
        let shared_tx: SharedTx = Arc::new(Mutex::new(initial_tx));
        let shared_tx_clone = Arc::clone(&shared_tx);
        let pane_id = id;

        // ログライターを Arc<Mutex> で共有する
        let log_writer: LogWriter = Arc::new(Mutex::new(None));
        let log_writer_clone = Arc::clone(&log_writer);

        // PTY 読み取りスレッドを起動する
        tokio::task::spawn_blocking(move || {
            let mut parser = VtParser::new(cols, rows);
            let mut buf = [0u8; 4096];

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        debug!("ペイン {} の PTY が EOF になりました", pane_id);
                        break;
                    }
                    Ok(n) => {
                        parser.advance(&buf[..n]);

                        // 録音中であれば生バイト列をログファイルに書き込む
                        if let Ok(mut guard) = log_writer_clone.lock() {
                            if let Some(w) = guard.as_mut() {
                                if let Err(e) = w.write_all(&buf[..n]) {
                                    error!("ログ書き込みエラー: {}", e);
                                    // エラー時は録音を停止する
                                    *guard = None;
                                }
                            }
                        }

                        // グリッド差分を送信する
                        let dirty = parser.screen_mut().take_dirty_rows();
                        if !dirty.is_empty() {
                            let (cursor_col, cursor_row) = parser.screen().cursor();
                            let msg = ServerToClient::GridDiff {
                                pane_id,
                                dirty_rows: dirty,
                                cursor_col,
                                cursor_row,
                            };
                            let _ = shared_tx_clone.lock().unwrap().blocking_send(msg);
                        }

                        // BEL を受信していればクライアントに通知する
                        if parser.screen_mut().take_pending_bell() {
                            let msg = ServerToClient::Bell { pane_id };
                            let _ = shared_tx_clone.lock().unwrap().blocking_send(msg);
                        }

                        // 画像データを送信する（Sixel / Kitty）
                        let images = parser.screen_mut().take_pending_images();
                        for img in images {
                            let msg = ServerToClient::ImagePlaced {
                                pane_id,
                                image_id: img.id,
                                col: img.col,
                                row: img.row,
                                width: img.width,
                                height: img.height,
                                rgba: img.rgba,
                            };
                            let _ = shared_tx_clone.lock().unwrap().blocking_send(msg);
                        }
                    }
                    Err(e) => {
                        error!("PTY 読み取りエラー: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            id,
            cols,
            rows,
            pid,
            shared_tx,
            master,
            writer,
            log_writer,
        })
    }

    /// Full Refresh グリッドを生成する（アタッチ時用）
    pub fn make_full_refresh(&self) -> Grid {
        Grid::new(self.cols, self.rows)
    }

    /// PTY 出力チャネルを差し替える（クライアント再アタッチ時）
    pub fn update_tx(&self, new_tx: mpsc::Sender<ServerToClient>) {
        *self.shared_tx.lock().unwrap() = new_tx;
    }

    /// PTY にデータを書き込む（キー入力転送）
    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(data)?;
        Ok(())
    }

    /// PTY 出力のファイル録音を開始する
    ///
    /// 録音中の場合は前のファイルを閉じてから新しいファイルを開く。
    pub fn start_recording(&self, path: &str) -> Result<()> {
        let file = File::create(path)?;
        let mut guard = self.log_writer.lock().expect("log_writer ロック取得");
        *guard = Some(BufWriter::new(file));
        info!("ペイン {} の録音を開始しました: {}", self.id, path);
        Ok(())
    }

    /// PTY 出力のファイル録音を停止する
    ///
    /// バッファをフラッシュしてからファイルを閉じる。
    pub fn stop_recording(&self) -> Result<()> {
        let mut guard = self.log_writer.lock().expect("log_writer ロック取得");
        if let Some(mut w) = guard.take() {
            w.flush()?;
            info!("ペイン {} の録音を停止しました", self.id);
        }
        Ok(())
    }

    /// PTY をリサイズする
    pub fn resize_pty(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.cols = cols;
        self.rows = rows;
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// 現在の作業ディレクトリを返す
    ///
    /// Linux のみ `/proc/{pid}/cwd` シンボリックリンクから取得する。
    /// 他の環境では `None` を返す。
    pub fn working_dir(&self) -> Option<std::path::PathBuf> {
        self.read_working_dir()
    }

    /// Linux 実装: /proc/{pid}/cwd から作業ディレクトリを取得する
    #[cfg(target_os = "linux")]
    fn read_working_dir(&self) -> Option<std::path::PathBuf> {
        self.pid
            .and_then(|pid| std::fs::read_link(format!("/proc/{}/cwd", pid)).ok())
    }

    /// Linux 以外: 作業ディレクトリ取得は非対応
    #[cfg(not(target_os = "linux"))]
    fn read_working_dir(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_idは単調増加する() {
        let id1 = new_pane_id();
        let id2 = new_pane_id();
        assert!(id2 > id1);
    }

    #[test]
    fn set_min_pane_id_でカウンターが更新される() {
        let current = new_pane_id();
        // 現在値より大きい値を設定すると反映される
        set_min_pane_id(current + 100);
        let next = new_pane_id();
        assert!(next >= current + 100);
    }
}
