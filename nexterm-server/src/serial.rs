//! シリアルポート接続 — serialport クレートを使って COM/tty デバイスに接続する
//!
//! `SerialPane` は PTY の代わりにシリアルポートを入出力バックエンドとして使用する。
//! BSP レイアウトツリーへの統合は通常ペインと同一の仕組みで行う。

use std::io::Write;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use nexterm_proto::{Grid, ServerToClient};
use nexterm_vt::VtParser;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use crate::pane::new_pane_id;

/// シリアルポートのパリティ設定を serialport 型に変換する
fn parse_parity(parity: &str) -> serialport::Parity {
    match parity {
        "odd" => serialport::Parity::Odd,
        "even" => serialport::Parity::Even,
        _ => serialport::Parity::None,
    }
}

/// シリアルポートのデータビットを serialport 型に変換する
fn parse_data_bits(data_bits: u8) -> serialport::DataBits {
    match data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    }
}

/// シリアルポートのストップビットを serialport 型に変換する
fn parse_stop_bits(stop_bits: u8) -> serialport::StopBits {
    match stop_bits {
        2 => serialport::StopBits::Two,
        _ => serialport::StopBits::One,
    }
}

/// シリアルポートバックエンドのペイン
pub struct SerialPane {
    pub id: u32,
    #[allow(dead_code)]
    pub cols: u16,
    #[allow(dead_code)]
    pub rows: u16,
    /// シリアルポートへの書き込みハンドル
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl SerialPane {
    /// シリアルポートに接続してペインを生成する
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        port_name: &str,
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: &str,
        cols: u16,
        rows: u16,
        tx: broadcast::Sender<ServerToClient>,
    ) -> Result<Self> {
        let serial = serialport::new(port_name, baud_rate)
            .data_bits(parse_data_bits(data_bits))
            .stop_bits(parse_stop_bits(stop_bits))
            .parity(parse_parity(parity))
            .timeout(std::time::Duration::from_millis(10))
            .open()
            .map_err(|e| anyhow!("シリアルポート '{}' を開けませんでした: {}", port_name, e))?;

        let pane_id = new_pane_id();
        info!(
            "シリアルポート '{}' をペイン {} として起動しました (baud={})",
            port_name, pane_id, baud_rate
        );

        // 書き込み用クローン
        let serial_write = serial
            .try_clone()
            .map_err(|e| anyhow!("シリアルポートのクローンに失敗しました: {}", e))?;

        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(serial_write)));

        // 初期グリッドを broadcast に送信する
        let _ = tx.send(ServerToClient::FullRefresh {
            pane_id,
            grid: Grid::new(cols, rows),
        });

        let tx_clone = tx;

        // 読み取りスレッドを起動する（serial は blocking I/O のため専用スレッド）
        std::thread::Builder::new()
            .name(format!("nexterm-serial-{}", pane_id))
            .spawn(move || {
                let mut parser = VtParser::new(cols, rows);
                let mut buf = vec![0u8; 4096];
                let mut read_port = serial;

                loop {
                    use std::io::Read;
                    match read_port.read(&mut buf) {
                        Ok(0) => {
                            debug!("シリアルポート: EOF (pane={})", pane_id);
                            break;
                        }
                        Ok(n) => {
                            parser.advance(&buf[..n]);
                            let dirty = parser.screen_mut().take_dirty_rows();
                            if !dirty.is_empty() {
                                let (cursor_col, cursor_row) = parser.screen().cursor();
                                let _ = tx_clone.send(ServerToClient::GridDiff {
                                    pane_id,
                                    dirty_rows: dirty,
                                    cursor_col,
                                    cursor_row,
                                });
                            }
                            if parser.screen_mut().take_pending_bell() {
                                let _ = tx_clone.send(ServerToClient::Bell { pane_id });
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                            continue; // ポーリングタイムアウトは正常
                        }
                        Err(e) => {
                            error!("シリアルポート読み取りエラー (pane={}): {}", pane_id, e);
                            break;
                        }
                    }
                }

                // 切断通知
                let _ = tx_clone.send(ServerToClient::PaneClosed { pane_id });
                info!("シリアルポート切断 (pane={})", pane_id);
            })?;

        Ok(Self {
            id: pane_id,
            cols,
            rows,
            writer,
        })
    }

    /// データをシリアルポートに書き込む（キー入力）
    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|e| anyhow!("シリアルライターのロック取得失敗: {}", e))?;
        w.write_all(data)?;
        Ok(())
    }

    /// リサイズは no-op（シリアルポートにウィンドウサイズの概念はない）
    pub fn resize_pty(&self, _cols: u16, _rows: u16) -> Result<()> {
        Ok(())
    }

    /// Full Refresh グリッドを生成する
    #[allow(dead_code)]
    pub fn make_full_refresh(&self) -> Grid {
        Grid::new(self.cols, self.rows)
    }

    /// 作業ディレクトリは常に None（シリアルポートには CWD がない）
    pub fn working_dir(&self) -> Option<std::path::PathBuf> {
        None
    }
}
