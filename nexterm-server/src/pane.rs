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
struct LogWriterInner {
    writer: BufWriter<File>,
    /// タイムスタンプを各行先頭に付加するかどうか
    timestamp: bool,
    /// ANSI エスケープシーケンスを除去するかどうか
    strip_ansi: bool,
    /// 行バッファ（改行が来るまで蓄積する）
    line_buf: Vec<u8>,
}

impl LogWriterInner {
    fn new(file: File, timestamp: bool, strip_ansi: bool) -> Self {
        Self {
            writer: BufWriter::new(file),
            timestamp,
            strip_ansi,
            line_buf: Vec::new(),
        }
    }

    /// バイト列を書き込む（改行単位でタイムスタンプ付加・ANSI 除去を適用）
    fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        if !self.timestamp && !self.strip_ansi {
            // 最適化: 特別な処理なしに直接書き込む
            return self.writer.write_all(data);
        }

        for &byte in data {
            self.line_buf.push(byte);
            if byte == b'\n' {
                self.flush_line()?;
            }
        }
        Ok(())
    }

    /// 蓄積した行を処理して書き込む
    fn flush_line(&mut self) -> std::io::Result<()> {
        let line = std::mem::take(&mut self.line_buf);
        let processed = if self.strip_ansi {
            strip_ansi_escapes(&line)
        } else {
            line
        };

        if self.timestamp && !processed.is_empty() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let secs = now.as_secs();
            let h = (secs / 3600) % 24;
            let m = (secs / 60) % 60;
            let s = secs % 60;
            let prefix = format!("[{:02}:{:02}:{:02}] ", h, m, s);
            self.writer.write_all(prefix.as_bytes())?;
        }
        self.writer.write_all(&processed)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // 残りのバッファを書き込む
        if !self.line_buf.is_empty() {
            let line = std::mem::take(&mut self.line_buf);
            let processed = if self.strip_ansi {
                strip_ansi_escapes(&line)
            } else {
                line
            };
            if self.timestamp && !processed.is_empty() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let secs = now.as_secs();
                let h = (secs / 3600) % 24;
                let m = (secs / 60) % 60;
                let s = secs % 60;
                let prefix = format!("[{:02}:{:02}:{:02}] ", h, m, s);
                self.writer.write_all(prefix.as_bytes())?;
            }
            self.writer.write_all(&processed)?;
        }
        self.writer.flush()
    }
}

/// ANSI エスケープシーケンスを除去する（ESC[ ... 終端文字 の形式に対応）
fn strip_ansi_escapes(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == 0x1b {
            i += 1;
            if i < input.len() {
                match input[i] {
                    b'[' => {
                        // CSI シーケンス: ESC [ ... 終端文字（0x40-0x7e）
                        i += 1;
                        while i < input.len() && !(0x40..=0x7e).contains(&input[i]) {
                            i += 1;
                        }
                        i += 1; // 終端文字をスキップ
                    }
                    b']' => {
                        // OSC シーケンス: ESC ] ... BEL or ST
                        i += 1;
                        while i < input.len() {
                            if input[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if input[i] == 0x1b && i + 1 < input.len() && input[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        // その他の ESC シーケンスは次の 1 バイトをスキップ
                        i += 1;
                    }
                }
            }
        } else {
            output.push(input[i]);
            i += 1;
        }
    }
    output
}

type LogWriter = Arc<Mutex<Option<LogWriterInner>>>;

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
                                if let Err(e) = w.write(&buf[..n]) {
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
                            match shared_tx_clone.lock() {
                                Ok(tx) => { let _ = tx.blocking_send(msg); }
                                Err(e) => error!("ペイン {}: 送信チャネルのロック取得に失敗しました: {}", pane_id, e),
                            }
                        }

                        // BEL を受信していればクライアントに通知する
                        if parser.screen_mut().take_pending_bell() {
                            let msg = ServerToClient::Bell { pane_id };
                            match shared_tx_clone.lock() {
                                Ok(tx) => { let _ = tx.blocking_send(msg); }
                                Err(e) => error!("ペイン {}: BEL 送信チャネルのロック取得に失敗しました: {}", pane_id, e),
                            }
                        }

                        // タイトル変更通知を送信する（OSC 0/1/2）
                        if let Some(title) = parser.screen_mut().take_pending_title() {
                            let msg = ServerToClient::TitleChanged { pane_id, title };
                            match shared_tx_clone.lock() {
                                Ok(tx) => { let _ = tx.blocking_send(msg); }
                                Err(e) => error!("ペイン {}: タイトル送信チャネルのロック取得に失敗しました: {}", pane_id, e),
                            }
                        }

                        // デスクトップ通知を送信する（OSC 9）
                        if let Some((title, body)) = parser.screen_mut().take_pending_notification() {
                            let msg = ServerToClient::DesktopNotification { pane_id, title, body };
                            match shared_tx_clone.lock() {
                                Ok(tx) => { let _ = tx.blocking_send(msg); }
                                Err(e) => error!("ペイン {}: 通知送信チャネルのロック取得に失敗しました: {}", pane_id, e),
                            }
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
                            match shared_tx_clone.lock() {
                                Ok(tx) => { let _ = tx.blocking_send(msg); }
                                Err(e) => error!("ペイン {}: 画像送信チャネルのロック取得に失敗しました: {}", pane_id, e),
                            }
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
        match self.shared_tx.lock() {
            Ok(mut guard) => *guard = new_tx,
            Err(e) => error!("ペイン {}: shared_tx のロック取得に失敗しました: {}", self.id, e),
        }
    }

    /// PTY にデータを書き込む（キー入力転送）
    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut w = self.writer.lock()
            .map_err(|e| anyhow::anyhow!("writer ロック取得に失敗しました: {}", e))?;
        w.write_all(data)?;
        Ok(())
    }

    /// PTY 出力のファイル録音を開始する
    ///
    /// 録音中の場合は前のファイルを閉じてから新しいファイルを開く。
    pub fn start_recording(&self, path: &str) -> Result<()> {
        self.start_recording_with_options(path, false, false)
    }

    /// PTY 出力のファイル録音をオプション付きで開始する
    pub fn start_recording_with_options(
        &self,
        path: &str,
        timestamp: bool,
        strip_ansi: bool,
    ) -> Result<()> {
        let file = File::create(path)?;
        let mut guard = self.log_writer.lock()
            .map_err(|e| anyhow::anyhow!("log_writer ロック取得に失敗しました: {}", e))?;
        *guard = Some(LogWriterInner::new(file, timestamp, strip_ansi));
        info!("ペイン {} の録音を開始しました: {}", self.id, path);
        Ok(())
    }

    /// PTY 出力のファイル録音を停止する
    ///
    /// バッファをフラッシュしてからファイルを閉じる。
    pub fn stop_recording(&self) -> Result<()> {
        let mut guard = self.log_writer.lock()
            .map_err(|e| anyhow::anyhow!("log_writer ロック取得に失敗しました: {}", e))?;
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

    /// macOS 実装: lsof を使って CWD を取得する
    #[cfg(target_os = "macos")]
    fn read_working_dir(&self) -> Option<std::path::PathBuf> {
        let pid = self.pid?;
        // lsof を使って CWD を取得する
        let output = std::process::Command::new("lsof")
            .args(["-p", &pid.to_string(), "-a", "-d", "cwd", "-Fn"])
            .output()
            .ok()?;
        // 出力例: "n/Users/jun/Documents\n"
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(path_str) = line.strip_prefix('n') {
                let path = std::path::PathBuf::from(path_str);
                if path.is_absolute() {
                    return Some(path);
                }
            }
        }
        None
    }

    /// Linux・macOS 以外: 作業ディレクトリ取得は非対応
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
