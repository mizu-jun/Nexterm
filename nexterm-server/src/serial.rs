//! Serial port connection — connect to COM / tty devices via the `serialport` crate.
//!
//! `SerialPane` uses a serial port as the I/O backend instead of a PTY. Integration into the BSP
//! layout tree follows the same mechanism as regular panes.

use std::io::Write;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use nexterm_proto::{Grid, ServerToClient};
use nexterm_vt::VtParser;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use crate::pane::new_pane_id;

/// Convert the parity string into a `serialport::Parity`.
fn parse_parity(parity: &str) -> serialport::Parity {
    match parity {
        "odd" => serialport::Parity::Odd,
        "even" => serialport::Parity::Even,
        _ => serialport::Parity::None,
    }
}

/// Convert the data-bits value into a `serialport::DataBits`.
fn parse_data_bits(data_bits: u8) -> serialport::DataBits {
    match data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    }
}

/// Convert the stop-bits value into a `serialport::StopBits`.
fn parse_stop_bits(stop_bits: u8) -> serialport::StopBits {
    match stop_bits {
        2 => serialport::StopBits::Two,
        _ => serialport::StopBits::One,
    }
}

/// Pane backed by a serial port.
pub struct SerialPane {
    pub id: u32,
    #[allow(dead_code)]
    pub cols: u16,
    #[allow(dead_code)]
    pub rows: u16,
    /// Write handle to the serial port.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl SerialPane {
    /// Connect to a serial port and create a pane.
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
            .map_err(|e| anyhow!("failed to open serial port '{}': {}", port_name, e))?;

        let pane_id = new_pane_id();
        info!(
            "started serial port '{}' as pane {} (baud={})",
            port_name, pane_id, baud_rate
        );

        // Clone for writes.
        let serial_write = serial
            .try_clone()
            .map_err(|e| anyhow!("failed to clone serial port: {}", e))?;

        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(serial_write)));

        // Send the initial grid via broadcast.
        let _ = tx.send(ServerToClient::FullRefresh {
            pane_id,
            grid: Grid::new(cols, rows),
        });

        let tx_clone = tx;

        // Launch the reader thread (serial uses blocking I/O, so it lives on its own thread).
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
                            debug!("serial port: EOF (pane={})", pane_id);
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
                            continue; // Polling timeouts are expected.
                        }
                        Err(e) => {
                            error!("serial port read error (pane={}): {}", pane_id, e);
                            break;
                        }
                    }
                }

                // Disconnect notification.
                let _ = tx_clone.send(ServerToClient::PaneClosed { pane_id });
                info!("serial port disconnected (pane={})", pane_id);
            })?;

        Ok(Self {
            id: pane_id,
            cols,
            rows,
            writer,
        })
    }

    /// Write data to the serial port (key input).
    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|e| anyhow!("failed to acquire serial writer lock: {}", e))?;
        w.write_all(data)?;
        Ok(())
    }

    /// Resizing is a no-op (a serial port has no concept of window size).
    pub fn resize_pty(&self, _cols: u16, _rows: u16) -> Result<()> {
        Ok(())
    }

    /// Build a Full Refresh grid.
    #[allow(dead_code)]
    pub fn make_full_refresh(&self) -> Grid {
        Grid::new(self.cols, self.rows)
    }

    /// Working directory is always `None` (a serial port has no CWD).
    pub fn working_dir(&self) -> Option<std::path::PathBuf> {
        None
    }
}
