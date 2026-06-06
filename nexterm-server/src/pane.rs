//! Pane — the smallest unit, managing a PTY process and its virtual grid.
//!
//! The PTY output channel is held as `Arc<broadcast::Sender>`.
//! Broadcasting allows sending to multiple clients simultaneously, and no swap is needed on reattach.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use nexterm_proto::{Grid, ServerToClient};
use nexterm_vt::VtParser;

static NEXT_PANE_ID: AtomicU32 = AtomicU32::new(1);

/// PTY output log writer (`Some` only while recording).
struct LogWriterInner {
    writer: BufWriter<File>,
    /// Whether to prepend a timestamp to each line.
    timestamp: bool,
    /// Whether to strip ANSI escape sequences.
    strip_ansi: bool,
    /// Line buffer (accumulates until a newline is seen).
    line_buf: Vec<u8>,
    /// Log file path (for rotation).
    path: String,
    /// Number of bytes already written to the current file.
    written_bytes: u64,
    /// Rotation byte limit (0 = unlimited).
    max_bytes: u64,
    /// Maximum number of files to keep.
    max_files: u32,
}

impl LogWriterInner {
    fn new(
        file: File,
        timestamp: bool,
        strip_ansi: bool,
        path: String,
        max_bytes: u64,
        max_files: u32,
    ) -> Self {
        Self {
            writer: BufWriter::new(file),
            timestamp,
            strip_ansi,
            line_buf: Vec::new(),
            path,
            written_bytes: 0,
            max_bytes,
            max_files,
        }
    }

    /// Check whether rotation is needed and perform it if so.
    fn rotate_if_needed(&mut self) -> std::io::Result<()> {
        if self.max_bytes == 0 || self.written_bytes < self.max_bytes {
            return Ok(());
        }
        // Flush the buffer before rotating.
        self.writer.flush()?;
        // Shift the older files: delete `.{max_files-1}`, rename `.N` to `.{N+1}`.
        let path = self.path.clone();
        let max = self.max_files;
        // Delete the oldest file.
        let oldest = format!("{}.{}", path, max);
        let _ = std::fs::remove_file(&oldest);
        // Shift N-1 -> N.
        for i in (1..max).rev() {
            let from = format!("{}.{}", path, i);
            let to = format!("{}.{}", path, i + 1);
            let _ = std::fs::rename(&from, &to);
        }
        // Rename the current file to `.1`.
        let _ = std::fs::rename(&path, format!("{}.1", path));
        // Create the new file.
        let new_file = File::create(&path)?;
        self.writer = BufWriter::new(new_file);
        self.written_bytes = 0;
        Ok(())
    }

    /// Write bytes (applies per-line timestamp prefixing and ANSI stripping).
    fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        // Rotate if needed.
        self.rotate_if_needed()?;

        if !self.timestamp && !self.strip_ansi {
            // Fast path: no special processing, write directly.
            self.written_bytes += data.len() as u64;
            return self.writer.write_all(data);
        }

        for &byte in data {
            self.line_buf.push(byte);
            self.written_bytes += 1;
            if byte == b'\n' {
                self.flush_line()?;
            }
        }
        Ok(())
    }

    /// Process and write the accumulated line.
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
        // Write any remaining buffered bytes.
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

/// Expand the log filename template.
///
/// Available placeholders:
///   {session}  — session name
///   {pane}     — pane ID
///   {datetime} — start time (YYYYMMDD_HHMMSS)
pub fn expand_log_filename_template(template: &str, session: &str, pane_id: u32) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Compute the UTC time manually (no chrono dependency).
    let secs_in_day = now % 86400;
    let h = secs_in_day / 3600;
    let m = (secs_in_day / 60) % 60;
    let s = secs_in_day % 60;
    // Simple date calculation (days since the Unix epoch -> year/month/day).
    let days = now / 86400;
    let (year, month, day) = days_to_ymd(days);
    let datetime = format!("{:04}{:02}{:02}_{:02}{:02}{:02}", year, month, day, h, m, s);

    template
        .replace("{session}", session)
        .replace("{pane}", &pane_id.to_string())
        .replace("{datetime}", &datetime)
}

/// Convert days since the Unix epoch into a (year, month, day) tuple (Gregorian).
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Algorithm: http://howardhinnant.github.io/date_algorithms.html
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

/// Strip ANSI escape sequences (handles the `ESC[ ... terminator` form).
fn strip_ansi_escapes(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == 0x1b {
            i += 1;
            if i < input.len() {
                match input[i] {
                    b'[' => {
                        // CSI sequence: ESC [ ... terminator (0x40-0x7e).
                        i += 1;
                        while i < input.len() && !(0x40..=0x7e).contains(&input[i]) {
                            i += 1;
                        }
                        i += 1; // Skip the terminator.
                    }
                    b']' => {
                        // OSC sequence: ESC ] ... BEL or ST.
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
                        // Skip a single byte for any other ESC sequence.
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

/// asciicast v2 format writer.
pub struct AsciicastWriter {
    file: BufWriter<File>,
    started_at: Instant,
}

impl AsciicastWriter {
    /// Create a new `AsciicastWriter` and write the header line.
    pub fn new(path: &str, cols: u16, rows: u16) -> Result<Self> {
        let file = File::create(path)?;
        let mut w = BufWriter::new(file);
        let unix_start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        writeln!(
            w,
            r#"{{"version":2,"width":{},"height":{},"timestamp":{},"title":"nexterm"}}"#,
            cols, rows, unix_start
        )?;
        Ok(Self {
            file: w,
            started_at: Instant::now(),
        })
    }

    /// Write PTY output data as an asciicast event line.
    pub fn write_output(&mut self, data: &[u8]) -> std::io::Result<()> {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        let text = String::from_utf8_lossy(data);
        // Escape the text into a JSON string via serde_json.
        let escaped = serde_json::to_string(&*text).unwrap_or_else(|_| "\"\"".to_string());
        writeln!(self.file, "[{:.6},\"o\",{}]", elapsed, escaped)?;
        Ok(())
    }

    /// Flush the buffer.
    pub fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

type AsciicastWriterHandle = Arc<Mutex<Option<AsciicastWriter>>>;

/// Allocate a new pane ID.
pub fn new_pane_id() -> u32 {
    NEXT_PANE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Update the ID counter after restoring from a snapshot.
///
/// Bumps the counter to at least the highest restored pane ID + 1 to avoid ID collisions.
pub fn set_min_pane_id(min_id: u32) {
    NEXT_PANE_ID.fetch_max(min_id, Ordering::Relaxed);
}

/// Check whether `cwd` is a valid directory that the PTY can actually be
/// spawned into.
///
/// Sprint 5-14 / v1.7.8 — P2-2: used by [`Pane::spawn_with_cwd`] to detect
/// snapshot-restore cases where the directory has been deleted since the
/// snapshot was written (e.g. `cargo clean` removed a `target/` subdir, or
/// the user removed a scratch directory while the session was offline). The
/// caller falls back to `$HOME` / `%USERPROFILE%` when this returns `false`.
///
/// Returns `false` when the path does not exist, is not a directory, or
/// metadata cannot be read (the last case is treated conservatively because
/// `spawn_command` would almost certainly fail too).
pub(crate) fn cwd_is_usable(cwd: &Path) -> bool {
    match std::fs::metadata(cwd) {
        Ok(md) => md.is_dir(),
        Err(_) => false,
    }
}

/// Broadcast send channel to every client (sync send, no Mutex required).
type SharedTx = Arc<broadcast::Sender<ServerToClient>>;

/// Pane state.
pub struct Pane {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    /// Child process PID (Linux: used to read the working directory via `/proc/{pid}/cwd`).
    #[allow(dead_code)]
    pid: Option<u32>,
    /// PTY output destination channel (can be swapped on reattach).
    #[allow(dead_code)]
    shared_tx: SharedTx,
    /// PTY master (for resizing).
    master: Box<dyn MasterPty + Send>,
    /// PTY write handle (used to forward key input).
    writer: Mutex<Box<dyn Write + Send>>,
    /// Text-log file writer (`Some` only while recording).
    log_writer: LogWriter,
    /// Binary-log file writer (`Some` only when `binary_log=true`).
    binary_log_writer: LogWriter,
    /// asciicast v2 writer (`Some` only while recording).
    asciicast_writer: AsciicastWriterHandle,
    /// Whether bracketed-paste mode (DEC ?2004) is enabled.
    pub bracketed_paste: Arc<std::sync::atomic::AtomicBool>,
    /// Mouse-reporting mode (0 = disabled, 1 = X11 ?1000, 2 = SGR ?1006).
    pub mouse_mode: Arc<std::sync::atomic::AtomicU8>,
    /// Kitty keyboard protocol progressive-enhancement flags (bitmask, 0 = disabled).
    pub keyboard_protocol_flags: Arc<std::sync::atomic::AtomicU8>,
    /// Current working directory reported by OSC 7 (Sprint 5-2 / B2).
    ///
    /// Updated when the shell emits something like `printf '\033]7;file://...' "$PWD"`.
    /// Used to inherit the parent CWD when splitting into a new pane.
    /// `None` when OSC 7 has never been received (callers fall back to `working_dir()` =
    /// `/proc/{pid}/cwd`).
    pub current_cwd: Arc<Mutex<Option<std::path::PathBuf>>>,
    /// Most recent full grid snapshot maintained by the PTY reader thread
    /// (v1.9.3 fix).
    ///
    /// The PTY reader owns the `VtParser` locally and only emits
    /// `GridDiff` broadcasts. When a client attaches *after* the shell has
    /// already produced output (the standard case for a restored session),
    /// those diffs are dropped (no broadcast receivers yet) and the parser
    /// state stays trapped in the reader thread. Mirroring the screen here
    /// after every burst lets `make_full_refresh` hand the late-attaching
    /// client the actual current screen instead of a fresh empty grid.
    latest_grid: Arc<Mutex<Grid>>,
}

impl Pane {
    /// Create a new pane and launch the shell.
    pub fn spawn(
        cols: u16,
        rows: u16,
        initial_tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
    ) -> Result<Self> {
        Self::spawn_impl(new_pane_id(), cols, rows, initial_tx, shell, args, None)
    }

    /// Create a pane with the specified ID (used to fix the ID up front when splitting via BSP).
    pub fn spawn_with_id(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
    ) -> Result<Self> {
        Self::spawn_impl(id, cols, rows, initial_tx, shell, args, None)
    }

    /// Create a pane with a specific ID and working directory (used to restore a snapshot).
    ///
    /// Sprint 5-14 / v1.7.8 — P2-2: when the requested `cwd` no longer exists
    /// (a common case after a snapshot survives across a `cargo clean`,
    /// `git clean -fdx`, or a deleted scratch directory), fall back to
    /// spawning without a cwd so `spawn_impl` will substitute the user's
    /// `$HOME` / `%USERPROFILE%`. Previously this surfaced as
    /// `HRESULT -2147024809 (E_INVALIDARG)` on Windows ConPTY and the whole
    /// pane silently disappeared from the restored snapshot.
    pub fn spawn_with_cwd(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<Self> {
        let effective_cwd: Option<&Path> = if cwd_is_usable(cwd) {
            Some(cwd)
        } else {
            tracing::warn!(
                "restored cwd is missing or not a directory ({}); falling back to $HOME",
                cwd.display()
            );
            None
        };
        Self::spawn_impl(id, cols, rows, initial_tx, shell, args, effective_cwd)
    }

    /// Internal PTY launch implementation (CWD is optional).
    fn spawn_impl(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<Self> {
        let pty_system = NativePtySystem::default();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| {
                format!(
                    "openpty failed (cols={}, rows={}, shell={:?}); \
                     ConPTY on Windows rejects size 0 with E_INVALIDARG (HRESULT 0x80070057)",
                    cols, rows, shell
                )
            })?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.args(args);
        // Fall back to the user's home directory if no explicit CWD is given.
        let home_buf: Option<std::path::PathBuf> = cwd
            .is_none()
            .then(|| {
                #[cfg(windows)]
                {
                    std::env::var("USERPROFILE")
                        .ok()
                        .map(std::path::PathBuf::from)
                }
                #[cfg(not(windows))]
                {
                    std::env::var("HOME").ok().map(std::path::PathBuf::from)
                }
            })
            .flatten();
        let effective_cwd = cwd.or(home_buf.as_deref());
        if let Some(c) = effective_cwd {
            cmd.cwd(c);
        }

        let child = pair.slave.spawn_command(cmd).with_context(|| {
            format!(
                "spawn_command failed (shell={:?}, args={:?}, cwd={:?})",
                shell, args, effective_cwd
            )
        })?;
        // Save the child PID (the process keeps running even after `child` is dropped).
        let pid = child.process_id();

        // Acquire the write handle (one-shot) and the read handle.
        let writer = Mutex::new(pair.master.take_writer()?);
        let mut reader = pair.master.try_clone_reader()?;
        let master = pair.master;

        // Share the `broadcast::Sender` via `Arc` (no Mutex needed, sync send).
        let shared_tx: SharedTx = Arc::new(initial_tx);
        let shared_tx_clone = Arc::clone(&shared_tx);
        let pane_id = id;

        // Share the log writer via `Arc<Mutex>`.
        let log_writer: LogWriter = Arc::new(Mutex::new(None));
        let log_writer_clone = Arc::clone(&log_writer);

        // Share the binary log writer via `Arc<Mutex>`.
        let binary_log_writer: LogWriter = Arc::new(Mutex::new(None));
        let binary_log_writer_clone = Arc::clone(&binary_log_writer);

        // Share the asciicast writer via `Arc<Mutex>`.
        let asciicast_writer: AsciicastWriterHandle = Arc::new(Mutex::new(None));
        let asciicast_writer_clone = Arc::clone(&asciicast_writer);

        // Share the bracketed-paste mode flag via `Arc<AtomicBool>`.
        let bracketed_paste: Arc<std::sync::atomic::AtomicBool> =
            Arc::new(std::sync::atomic::AtomicBool::new(false));
        let bracketed_paste_clone = Arc::clone(&bracketed_paste);

        // Share the mouse-reporting mode via `Arc<AtomicU8>`.
        let mouse_mode: Arc<std::sync::atomic::AtomicU8> =
            Arc::new(std::sync::atomic::AtomicU8::new(0));
        let mouse_mode_clone = Arc::clone(&mouse_mode);

        // Share the Kitty keyboard protocol flags via `Arc<AtomicU8>`.
        let keyboard_protocol_flags: Arc<std::sync::atomic::AtomicU8> =
            Arc::new(std::sync::atomic::AtomicU8::new(0));
        let keyboard_protocol_flags_clone = Arc::clone(&keyboard_protocol_flags);

        // Share the OSC 7 CWD via `Arc<Mutex<Option<PathBuf>>>` (Sprint 5-2 / B2).
        let current_cwd: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::new(Mutex::new(None));
        let current_cwd_clone = Arc::clone(&current_cwd);

        // Share the latest full-grid snapshot (v1.9.3 fix). Initialised as an
        // empty grid; the reader thread overwrites it as bytes arrive.
        let latest_grid: Arc<Mutex<Grid>> = Arc::new(Mutex::new(Grid::new(cols, rows)));
        let latest_grid_clone = Arc::clone(&latest_grid);

        // Launch the PTY reader thread.
        tokio::task::spawn_blocking(move || {
            let mut parser = VtParser::new(cols, rows);
            let mut buf = [0u8; 4096];

            /// Helper that sends a message via the `broadcast::Sender` (sync, no waiting).
            fn send_msg(tx: &broadcast::Sender<ServerToClient>, msg: ServerToClient) {
                // Ignore when there are no receivers (no client attached).
                let _ = tx.send(msg);
            }

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        debug!("pane {}: PTY reached EOF", pane_id);
                        break;
                    }
                    Ok(n) => {
                        parser.advance(&buf[..n]);

                        // Reflect bracketed-paste mode changes into the AtomicBool.
                        bracketed_paste_clone.store(
                            parser.bracketed_paste_mode(),
                            std::sync::atomic::Ordering::Relaxed,
                        );

                        // Reflect mouse-reporting mode changes into the AtomicU8.
                        mouse_mode_clone.store(
                            parser.screen().mouse_mode,
                            std::sync::atomic::Ordering::Relaxed,
                        );

                        // Reflect Kitty keyboard protocol flag changes into the AtomicU8.
                        keyboard_protocol_flags_clone.store(
                            parser.screen().keyboard_protocol_flags(),
                            std::sync::atomic::Ordering::Relaxed,
                        );

                        // If recording, write the raw byte sequence to the log file.
                        if let Ok(mut guard) = log_writer_clone.lock()
                            && let Some(w) = guard.as_mut()
                            && let Err(e) = w.write(&buf[..n])
                        {
                            error!("log write error: {}", e);
                            *guard = None;
                        }

                        // Binary log: save raw PTY bytes verbatim.
                        if let Ok(mut guard) = binary_log_writer_clone.lock()
                            && let Some(w) = guard.as_mut()
                            && let Err(e) = w.write(&buf[..n])
                        {
                            error!("binary log write error: {}", e);
                            *guard = None;
                        }

                        // If asciicast recording is active, write to it.
                        if let Ok(mut guard) = asciicast_writer_clone.lock()
                            && let Some(w) = guard.as_mut()
                            && let Err(e) = w.write_output(&buf[..n])
                        {
                            error!("asciicast write error: {}", e);
                            *guard = None;
                        }

                        // Send the grid diff.
                        let dirty = parser.screen_mut().take_dirty_rows();
                        if !dirty.is_empty() {
                            // v1.9.3 fix: refresh the full-grid snapshot so a
                            // client attaching after this burst still sees the
                            // current screen via `make_full_refresh`. Without
                            // this the parser state is trapped in the reader
                            // thread and late-attachers get an empty grid.
                            if let Ok(mut g) = latest_grid_clone.lock() {
                                *g = parser.screen().full_refresh_grid();
                            }
                            let (cursor_col, cursor_row) = parser.screen().cursor();
                            let msg = ServerToClient::GridDiff {
                                pane_id,
                                dirty_rows: dirty,
                                cursor_col,
                                cursor_row,
                            };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Notify the client if a BEL was received.
                        if parser.screen_mut().take_pending_bell() {
                            let msg = ServerToClient::Bell { pane_id };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send a title-change notification (OSC 0/1/2).
                        if let Some(title) = parser.screen_mut().take_pending_title() {
                            let msg = ServerToClient::TitleChanged { pane_id, title };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send a desktop notification (OSC 9 / 777).
                        if let Some((title, body)) = parser.screen_mut().take_pending_notification()
                        {
                            let msg = ServerToClient::DesktopNotification {
                                pane_id,
                                title,
                                body,
                            };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send OSC 52 clipboard write requests (Sprint 4-1).
                        // The client honors the `SecurityConfig.osc52_clipboard` policy on its side.
                        for text in parser.screen_mut().take_pending_clipboard_writes() {
                            let msg = ServerToClient::ClipboardWriteRequest { pane_id, text };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send an OSC 7 CWD change notification (Sprint 5-2 / B2).
                        if let Some(cwd) = parser.screen_mut().take_pending_cwd() {
                            if let Ok(mut guard) = current_cwd_clone.lock() {
                                *guard = Some(std::path::PathBuf::from(&cwd));
                            }
                            let msg = ServerToClient::CwdChanged { pane_id, cwd };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send OSC 133 semantic-zone marks.
                        for mark in parser.screen_mut().take_semantic_marks() {
                            let kind = match mark.kind {
                                nexterm_vt::SemanticMarkKind::PromptStart => "A",
                                nexterm_vt::SemanticMarkKind::CommandStart => "B",
                                nexterm_vt::SemanticMarkKind::OutputStart => "C",
                                nexterm_vt::SemanticMarkKind::CommandEnd => "D",
                            };
                            let msg = ServerToClient::SemanticMark {
                                pane_id,
                                row: mark.row,
                                kind: kind.to_string(),
                                exit_code: mark.exit_code,
                            };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send image data (Sixel / Kitty).
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
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send OSC 66 text-sizing events (Kitty Text Sizing Protocol).
                        let text_events = parser.screen_mut().take_pending_text_sizing();
                        for ev in text_events {
                            let msg = ServerToClient::TextSized {
                                pane_id,
                                col: ev.col,
                                row: ev.row,
                                scale_num: ev.scale_num,
                                scale_den: ev.scale_den,
                                width_cells: ev.width_cells,
                                valign: ev.valign,
                                halign: ev.halign,
                                text: ev.text,
                            };
                            send_msg(&shared_tx_clone, msg);
                        }
                    }
                    Err(e) => {
                        error!("PTY read error: {}", e);
                        break;
                    }
                }
            }

            // Fix 2: send SIGHUP to the process group on PTY EOF to avoid zombie processes.
            #[cfg(unix)]
            if let Some(pid_val) = pid
                && pid_val > 0
            {
                // SAFETY: kill() is safe with a valid pid; pgid == pid (we did not call setsid).
                unsafe { libc::kill(pid_val as libc::pid_t, libc::SIGHUP) };
                debug!("pane {}: sent SIGHUP to PID {}", pane_id, pid_val);
            }

            // Fix 1: emit PaneClosed when the PTY reaches EOF or the shell exits.
            debug!("pane {}: PTY loop finished; sending PaneClosed", pane_id);
            send_msg(&shared_tx_clone, ServerToClient::PaneClosed { pane_id });
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
            binary_log_writer,
            asciicast_writer,
            bracketed_paste,
            mouse_mode,
            keyboard_protocol_flags,
            current_cwd,
            latest_grid,
        })
    }

    /// Return the most recent CWD reported via OSC 7 (`None` if never received).
    ///
    /// Used to inherit the CWD into a child pane when splitting. When OSC 7 is unavailable,
    /// callers fall back to `working_dir()` (e.g. `/proc/{pid}/cwd`).
    pub fn osc7_cwd(&self) -> Option<std::path::PathBuf> {
        self.current_cwd.lock().ok().and_then(|g| g.clone())
    }

    /// Build a Full Refresh grid (used on client attach).
    ///
    /// Returns a clone of the latest grid snapshot maintained by the PTY
    /// reader thread. Falls back to an empty grid of the current size if the
    /// shared lock is poisoned — this is the same fallback as the pre-v1.9.3
    /// implementation and keeps callers safe.
    pub fn make_full_refresh(&self) -> Grid {
        match self.latest_grid.lock() {
            Ok(g) => g.clone(),
            Err(_) => Grid::new(self.cols, self.rows),
        }
    }

    /// Swap the PTY output channel — for broadcast, no swap is needed on reattach (no-op).
    #[allow(dead_code)]
    pub fn update_tx(&self, _new_tx: broadcast::Sender<ServerToClient>) {
        // `broadcast::Sender` is shared, so reattaching does not require a swap.
    }

    /// Write data to the PTY (forwarded key input).
    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|e| anyhow::anyhow!("failed to acquire writer lock: {}", e))?;
        w.write_all(data)?;
        Ok(())
    }

    /// Start recording PTY output to a file.
    ///
    /// When already recording, the previous file is closed before opening the new one.
    pub fn start_recording(&self, path: &str) -> Result<()> {
        self.start_recording_with_options(path, false, false)
    }

    /// Start recording PTY output with options.
    pub fn start_recording_with_options(
        &self,
        path: &str,
        timestamp: bool,
        strip_ansi: bool,
    ) -> Result<()> {
        self.start_recording_with_rotation(path, timestamp, strip_ansi, 0, 5)
    }

    /// Start recording PTY output with rotation settings.
    ///
    /// When `max_size_mb` is 0, rotation is disabled. `max_files` is the number of files to keep.
    pub fn start_recording_with_rotation(
        &self,
        path: &str,
        timestamp: bool,
        strip_ansi: bool,
        max_size_mb: u64,
        max_files: u32,
    ) -> Result<()> {
        let file = File::create(path)?;
        let max_bytes = max_size_mb.saturating_mul(1024 * 1024);
        let mut guard = self
            .log_writer
            .lock()
            .map_err(|e| anyhow::anyhow!("failed to acquire log_writer lock: {}", e))?;
        *guard = Some(LogWriterInner::new(
            file,
            timestamp,
            strip_ansi,
            path.to_string(),
            max_bytes,
            max_files,
        ));
        info!("pane {}: started recording to {}", self.id, path);
        Ok(())
    }

    /// Stop recording PTY output to a file.
    ///
    /// Flushes the buffer before closing the file.
    pub fn stop_recording(&self) -> Result<()> {
        let mut guard = self
            .log_writer
            .lock()
            .map_err(|e| anyhow::anyhow!("failed to acquire log_writer lock: {}", e))?;
        if let Some(mut w) = guard.take() {
            w.flush()?;
            info!("pane {}: stopped recording", self.id);
        }
        Ok(())
    }

    /// Start recording using `LogConfig` (template, binary log, ...).
    ///
    /// `base_path` is the default path used when no template is set.
    pub fn start_recording_with_config(
        &self,
        base_path: &str,
        session: &str,
        log_config: &nexterm_config::LogConfig,
    ) -> Result<()> {
        // Expand the template if configured.
        let resolved_path = if let Some(ref tmpl) = log_config.file_name_template {
            // Use the template to generate the filename.
            let filename = expand_log_filename_template(tmpl, session, self.id);
            if let Some(log_dir) = &log_config.log_dir {
                format!("{}/{}", log_dir.trim_end_matches('/'), filename)
            } else {
                filename
            }
        } else {
            base_path.to_string()
        };

        // Create the parent directory.
        if let Some(parent) = std::path::Path::new(&resolved_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Start the text log.
        self.start_recording_with_options(
            &resolved_path,
            log_config.timestamp,
            log_config.strip_ansi,
        )?;

        // When binary logging is enabled, also start a raw-binary file.
        if log_config.binary_log {
            let bin_path = format!("{}.bin", resolved_path.trim_end_matches(".log"));
            let bin_file = File::create(&bin_path)?;
            // Binary log saves raw bytes without timestamp/strip_ansi.
            let mut guard = self
                .binary_log_writer
                .lock()
                .map_err(|e| anyhow::anyhow!("failed to acquire binary_log_writer lock: {}", e))?;
            *guard = Some(LogWriterInner::new(
                bin_file,
                false,
                false,
                bin_path.clone(),
                0,
                0,
            ));
            info!("pane {}: started binary log at {}", self.id, bin_path);
        }

        Ok(())
    }

    /// Start an asciicast v2 recording.
    pub fn start_asciicast(&self, path: &str) -> Result<()> {
        let writer = AsciicastWriter::new(path, self.cols, self.rows)?;
        let mut guard = self
            .asciicast_writer
            .lock()
            .map_err(|e| anyhow::anyhow!("failed to acquire asciicast_writer lock: {}", e))?;
        *guard = Some(writer);
        info!("pane {}: started asciicast recording at {}", self.id, path);
        Ok(())
    }

    /// Stop the asciicast v2 recording.
    ///
    /// Flushes the buffer before closing the file.
    pub fn stop_asciicast(&self) -> Result<()> {
        let mut guard = self
            .asciicast_writer
            .lock()
            .map_err(|e| anyhow::anyhow!("failed to acquire asciicast_writer lock: {}", e))?;
        if let Some(mut w) = guard.take() {
            w.flush()?;
            info!("pane {}: stopped asciicast recording", self.id);
        }
        Ok(())
    }

    /// Resize the PTY.
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

    /// Return the current working directory.
    ///
    /// Only Linux reads it via the `/proc/{pid}/cwd` symlink. Other platforms return `None`.
    pub fn working_dir(&self) -> Option<std::path::PathBuf> {
        self.read_working_dir()
    }

    /// Linux implementation: read the working directory from `/proc/{pid}/cwd`.
    #[cfg(target_os = "linux")]
    fn read_working_dir(&self) -> Option<std::path::PathBuf> {
        self.pid
            .and_then(|pid| std::fs::read_link(format!("/proc/{}/cwd", pid)).ok())
    }

    /// macOS implementation: read the CWD via `lsof`.
    #[cfg(target_os = "macos")]
    fn read_working_dir(&self) -> Option<std::path::PathBuf> {
        let pid = self.pid?;
        // Use lsof to get the CWD.
        let output = std::process::Command::new("lsof")
            .args(["-p", &pid.to_string(), "-a", "-d", "cwd", "-Fn"])
            .output()
            .ok()?;
        // Example output: "n/Users/jun/Documents\n"
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

    /// Windows implementation: ask PowerShell for the child process CWD.
    #[cfg(windows)]
    fn read_working_dir(&self) -> Option<std::path::PathBuf> {
        let pid = self.pid?;
        // `(Get-Process).Path` is the binary path, so take the parent via Split-Path.
        let script = format!(
            "(Get-Process -Id {} -ErrorAction SilentlyContinue).Path | Split-Path -Parent",
            pid
        );
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .ok()?;
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            let trimmed = path_str.trim();
            if !trimmed.is_empty() {
                return Some(std::path::PathBuf::from(trimmed));
            }
        }
        None
    }

    /// Other operating systems: working-directory detection is unsupported.
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    fn read_working_dir(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// Return whether a foreground process (a child other than the shell itself) is running in
    /// this pane.
    ///
    /// Implemented in Sprint 5-8 Phase 4-4. Used to decide whether to show the confirmation
    /// dialog when `close_action = "prompt"` and the OS window is being closed.
    ///
    /// Note: as of Phase 4-4, the only caller is `Window::has_foreground_process`, which itself
    /// is dead_code until the `QueryForegroundProcess` IPC is added in Phase 4-5.
    #[allow(dead_code)]
    ///
    /// **Linux implementation**:
    /// - Compare `tpgid` (foreground process group ID) and `pgrp` from `/proc/{pid}/stat`.
    /// - When `tpgid != pgrp`, a non-shell process (e.g. vim, ssh, long-running job) is in the
    ///   foreground.
    /// - `tpgid <= 0`: no controlling terminal -> `false`.
    ///
    /// **macOS implementation** (Phase 4-6): scan the child process tree with `ps -A -o pid=,ppid=`.
    ///
    /// **Windows implementation** (Phase 4-7): enumerate every process via
    /// `CreateToolhelp32Snapshot` + `Process32FirstW/NextW` and check whether any child has the
    /// shell PID as its parent. Depends on the `windows-sys` crate.
    ///
    /// Returns:
    /// - `true`: a confirmation dialog is required (long-running job, ssh session, ...).
    /// - `false`: safe to close immediately (sitting at the shell prompt, or the OS is unsupported).
    pub fn has_foreground_process(&self) -> bool {
        self.read_has_foreground_process()
    }

    /// Linux implementation: compare `tpgid` and `pgrp` from `/proc/{pid}/stat`.
    #[cfg(target_os = "linux")]
    fn read_has_foreground_process(&self) -> bool {
        let Some(pid) = self.pid else {
            return false;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", pid)) else {
            return false;
        };
        // stat format: "pid (comm) state ppid pgrp session tty_nr tpgid flags ..."
        // `comm` may contain spaces, parens, and newlines; split on the last `") "`.
        let Some((_, after)) = stat.rsplit_once(") ") else {
            return false;
        };
        let fields: Vec<&str> = after.split_whitespace().collect();
        // Indices in after_comm:
        // [0]=state, [1]=ppid, [2]=pgrp, [3]=session, [4]=tty_nr, [5]=tpgid
        let Some(pgrp) = fields.get(2).and_then(|s| s.parse::<i32>().ok()) else {
            return false;
        };
        let Some(tpgid) = fields.get(5).and_then(|s| s.parse::<i32>().ok()) else {
            return false;
        };
        // `tpgid <= 0` means no controlling terminal or unreadable.
        tpgid > 0 && tpgid != pgrp
    }

    /// macOS implementation (Sprint 5-9 Phase 4-6): scan children with `ps -A -o pid=,ppid=`.
    ///
    /// If at least one process has the shell PID as its parent, treat it as having a foreground
    /// process. Not a full POSIX `tcgetpgrp`-based check, but sufficient to detect "a child
    /// running directly under the shell" such as ssh / vim / long-running jobs.
    ///
    /// Caveats:
    /// - Spawning `ps` every time costs tens of milliseconds. Acceptable because
    ///   `QueryForegroundProcess` is invoked only when a window is closing.
    /// - When the shell has background jobs (long-running processes started with `&`), this
    ///   returns `true` even if no foreground job exists. A reasonable safe-side fallback
    ///   (a false positive only shows the confirmation dialog, which the user can dismiss).
    #[cfg(target_os = "macos")]
    fn read_has_foreground_process(&self) -> bool {
        let Some(pid) = self.pid else {
            return false;
        };
        let Ok(output) = std::process::Command::new("ps")
            .args(["-A", "-o", "pid=,ppid="])
            .output()
        else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.lines().any(|line| {
            // Format: "  1234   5678" (pid, ppid).
            let mut parts = line.split_whitespace();
            let _ = parts.next(); // pid
            parts.next().and_then(|s| s.parse::<u32>().ok()) == Some(pid)
        })
    }

    /// Windows implementation (Sprint 5-10 Phase 4-7): enumerate processes with
    /// `CreateToolhelp32Snapshot` and treat any process whose parent is the shell PID
    /// (`self.pid`) as a foreground process.
    ///
    /// Same as the macOS implementation: not a full ConPTY foreground-process-group check, but
    /// sufficient for ssh / vim / long-running jobs. The false-positive case (a shell that holds
    /// background jobs returning `true`) is a safe-side fallback.
    ///
    /// Performance: the snapshot has a few-millisecond overhead.
    /// Acceptable since `QueryForegroundProcess` is only invoked when a window is closing.
    #[cfg(windows)]
    fn read_has_foreground_process(&self) -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };

        let Some(pid) = self.pid else {
            return false;
        };

        // SAFETY: CreateToolhelp32Snapshot returns INVALID_HANDLE_VALUE on failure, so we always
        // check it before calling any subsequent API. Arguments follow the spec
        // (TH32CS_SNAPPROCESS, 0=system-wide).
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }

        // Prevent handle leak: even on early return / panic, Drop calls CloseHandle.
        struct HandleGuard(HANDLE);
        impl Drop for HandleGuard {
            fn drop(&mut self) {
                // SAFETY: HANDLE is a valid handle from CreateToolhelp32Snapshot; the
                // INVALID_HANDLE_VALUE case returned before the guard was created.
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
        let _guard = HandleGuard(snapshot);

        // SAFETY: PROCESSENTRY32W is POD (all numeric / fixed-length fields), so zero-init is fine.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        // SAFETY: snapshot is valid; entry has dwSize set, satisfying the API requirements.
        // On failure (return 0) treat it as "no foreground process" and return false.
        if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
            return false;
        }

        loop {
            if entry.th32ParentProcessID == pid {
                return true;
            }
            // SAFETY: snapshot is valid; entry can be reused inside the loop.
            // Process32NextW returning 0 (= no more entries) ends the loop.
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }

        false
    }

    /// Other operating systems: detection is unsupported.
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    fn read_has_foreground_process(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_id_increases_monotonically() {
        let id1 = new_pane_id();
        let id2 = new_pane_id();
        assert!(id2 > id1);
    }

    #[test]
    fn set_min_pane_id_updates_counter() {
        let current = new_pane_id();
        // Setting a value larger than the current counter takes effect.
        set_min_pane_id(current + 100);
        let next = new_pane_id();
        assert!(next >= current + 100);
    }

    // ---- cwd_is_usable tests (Sprint 5-14 / v1.7.8 — P2-2) ----

    #[test]
    fn cwd_is_usable_returns_true_for_existing_directory() {
        // The OS temp dir always exists and is a directory.
        let temp = std::env::temp_dir();
        assert!(cwd_is_usable(&temp));
    }

    #[test]
    fn cwd_is_usable_returns_false_for_missing_path() {
        // Build a path under temp dir that we know does not exist.
        let mut bogus = std::env::temp_dir();
        bogus.push("nexterm-cwd-fallback-test-does-not-exist-zzz-9384721");
        // Make sure it really does not exist before asserting.
        let _ = std::fs::remove_dir_all(&bogus);
        assert!(!cwd_is_usable(&bogus));
    }

    #[test]
    fn cwd_is_usable_returns_false_for_regular_file() {
        // Create a temporary file and verify it is rejected as a cwd.
        let mut path = std::env::temp_dir();
        path.push(format!("nexterm-cwd-fallback-{}.tmp", std::process::id()));
        std::fs::write(&path, b"test").expect("temp file write");
        assert!(!cwd_is_usable(&path));
        let _ = std::fs::remove_file(&path);
    }

    // ---- strip_ansi_escapes tests ----

    #[test]
    fn strip_ansi_escapes_removes_color_codes() {
        // ESC[31mred textESC[0m
        let input = b"\x1b[31mred text\x1b[0m";
        let output = strip_ansi_escapes(input);
        assert_eq!(output, b"red text");
    }

    #[test]
    fn strip_ansi_escapes_ignores_plain_text() {
        let input = b"plain text without escapes";
        let output = strip_ansi_escapes(input);
        assert_eq!(output, input);
    }

    #[test]
    fn strip_ansi_escapes_handles_empty_input() {
        let input: &[u8] = b"";
        let output = strip_ansi_escapes(input);
        assert!(output.is_empty());
    }

    #[test]
    fn strip_ansi_escapes_handles_partial_sequences() {
        // Incomplete sequence: just ESC[.
        let input = b"\x1b[";
        let output = strip_ansi_escapes(input);
        assert_eq!(output, b"");
    }

    #[test]
    fn strip_ansi_escapes_removes_cursor_position() {
        // ESC[H (move cursor home).
        let input = b"\x1b[HHello";
        let output = strip_ansi_escapes(input);
        assert_eq!(output, b"Hello");
    }

    #[test]
    fn strip_ansi_escapes_handles_styles() {
        // ESC[1m (bold), ESC[4m (underline).
        let input = b"\x1b[1mbold\x1b[0m_\x1b[4munderline\x1b[0m";
        let output = strip_ansi_escapes(input);
        assert_eq!(output, b"bold_underline");
    }

    #[test]
    fn strip_ansi_escapes_preserves_logo_and_newline() {
        // Special characters other than escape sequences are preserved.
        let input = b"line1\nline2\tdata";
        let output = strip_ansi_escapes(input);
        assert_eq!(output, b"line1\nline2\tdata");
    }

    #[test]
    fn strip_ansi_escapes_handles_multiple_sequences() {
        // Multiple sequences mixed together.
        let input = b"\x1b[31m\x1b[1m\x1b[4mred bold underline\x1b[0m";
        let output = strip_ansi_escapes(input);
        assert_eq!(output, b"red bold underline");
    }

    #[test]
    fn strip_ansi_escapes_handles_osc_sequences() {
        // OSC sequence: ESC]title BEL.
        let input = b"\x1b]0;window title\x07content";
        let output = strip_ansi_escapes(input);
        assert_eq!(output, b"content");
    }

    #[test]
    fn strip_ansi_escapes_handles_unicode() {
        // Includes Unicode text.
        let input = b"\x1b[31m\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e\x1b[0m"; // "日本語" (Japanese) colored red.
        let output = strip_ansi_escapes(input);
        assert_eq!(output, b"\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e"); // "日本語" (Japanese).
    }

    // ---- v1.9.3: make_full_refresh must reflect PTY output, not return an
    //              empty grid. This reproduces the "blank screen on restored
    //              session" bug where the shell prompt is emitted before any
    //              client attaches, the GridDiff broadcast is dropped (no
    //              receiver), and a late-attaching client sees an empty
    //              FullRefresh forever because the parser screen is trapped in
    //              the reader thread.

    #[cfg(not(windows))]
    fn grid_text(grid: &nexterm_proto::Grid) -> String {
        grid.rows
            .iter()
            .flat_map(|row| row.iter().map(|c| c.ch))
            .collect()
    }

    // Gated on Unix because, on Windows, portable-pty's blocking
    // `ConPtyMaster::read_full_one_message` (called from the reader thread's
    // `reader.read(&mut buf)`) does not always wake up when the child exits
    // — it can keep the `tokio::task::spawn_blocking` thread parked, so
    // `cargo test` hangs after the test logic itself succeeds. The fix in
    // this file is platform-independent (sharing the parser screen through
    // `Arc<Mutex<Grid>>`), so verifying it on Unix CI is sufficient. On
    // Windows the change should be verified by running the GUI build and
    // checking that the restored session is no longer blank.
    #[cfg(not(windows))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn make_full_refresh_reflects_pty_output_emitted_before_attach() {
        use std::time::{Duration, Instant};

        // Use a one-shot command so the child process exits on its own —
        // an interactive shell would hold the PTY open and hang the test.
        let marker = "NEXTERM_MARKER_FULLREFRESH_V193";
        let (shell, args): (&str, Vec<String>) =
            ("/bin/sh", vec!["-c".into(), format!("echo {}", marker)]);

        let (tx, _rx) = tokio::sync::broadcast::channel::<ServerToClient>(2048);
        let pane = Pane::spawn_with_id(1, 80, 24, tx, shell, &args).expect("spawn_with_id failed");

        // Poll up to 5 s. Before the fix `make_full_refresh` returns a fresh
        // empty grid, so the marker is never found and the assertion below
        // fails — that is the RED state we are reproducing.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if grid_text(&pane.make_full_refresh()).contains(marker) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let dump = grid_text(&pane.make_full_refresh());
        panic!(
            "make_full_refresh did not contain `{}` within 5 s. \
             Reproduces the blank-screen bug: the PTY reader received the \
             output but `make_full_refresh` returns an empty grid because \
             the parser screen is not shared. Grid (first 200 chars): {:?}",
            marker,
            dump.chars().take(200).collect::<String>()
        );
    }
}
