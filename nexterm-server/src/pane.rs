//! ペイン — PTY プロセスと仮想グリッドを管理する最小単位
//!
//! PTY 出力チャネルは `Arc<Mutex<Sender>>` で保持し、
//! クライアント再アタッチ時に `update_tx` で差し替えられる。

use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::mpsc;
use tracing::{debug, error};

use nexterm_proto::{Grid, ServerToClient};
use nexterm_vt::VtParser;

static NEXT_PANE_ID: AtomicU32 = AtomicU32::new(1);

/// ペイン ID を新規発行する
pub fn new_pane_id() -> u32 {
    NEXT_PANE_ID.fetch_add(1, Ordering::Relaxed)
}

/// 動的に差し替え可能な送信チャネル
type SharedTx = Arc<Mutex<mpsc::Sender<ServerToClient>>>;

/// ペインの状態
pub struct Pane {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    /// PTY 出力先チャネル（再アタッチ時に差し替え可能）
    shared_tx: SharedTx,
    /// PTY マスタ（リサイズ用）
    master: Box<dyn MasterPty + Send>,
    /// PTY 書き込みハンドル（キー入力転送用）
    writer: Mutex<Box<dyn Write + Send>>,
}

impl Pane {
    /// 新しいペインを生成してシェルを起動する
    pub fn spawn(
        cols: u16,
        rows: u16,
        initial_tx: mpsc::Sender<ServerToClient>,
        shell: &str,
    ) -> Result<Self> {
        Self::spawn_with_id(new_pane_id(), cols, rows, initial_tx, shell)
    }

    /// 指定 ID でペインを生成する（BSP 分割時に ID を事前確定するために使用）
    pub fn spawn_with_id(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: mpsc::Sender<ServerToClient>,
        shell: &str,
    ) -> Result<Self> {
        let pty_system = NativePtySystem::default();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let cmd = CommandBuilder::new(shell);
        let _child = pair.slave.spawn_command(cmd)?;

        // 書き込みハンドル（1 度だけ取得可）と読み取りハンドルを取得する
        let writer = Mutex::new(pair.master.take_writer()?);
        let mut reader = pair.master.try_clone_reader()?;
        let master = pair.master;

        // 動的チャネルを Arc<Mutex> で共有する
        let shared_tx: SharedTx = Arc::new(Mutex::new(initial_tx));
        let shared_tx_clone = Arc::clone(&shared_tx);
        let pane_id = id;

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
            shared_tx,
            master,
            writer,
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
}
