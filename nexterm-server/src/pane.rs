//! Pane — the smallest unit, managing a PTY process and its virtual grid.
//!
//! The PTY output channel is held as `Arc<broadcast::Sender>`.
//! Broadcasting allows sending to multiple clients simultaneously, and no swap is needed on reattach.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use nexterm_proto::{Cell, Grid, ServerToClient};
use nexterm_vt::VtParser;

/// Default cap on the per-pane scrollback mirror (lines). The server plumbs the
/// `scrollback_lines` config value over this via [`Pane::set_scrollback_limit`];
/// this constant is the fallback before that is called.
const DEFAULT_PANE_SCROLLBACK_LINES: usize = 10_000;

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

/// Standard base64 encoding (with padding). Local helper — the workspace has
/// a decoder in `nexterm-vt` but no encoder dependency.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Maximum OSC 72 payload bytes per escape (protocol limit; applies to the
/// base64-encoded form).
const DND_CHUNK_BYTES: usize = 4096;

/// Builds the OSC 72 replies for one data request (kitty DnD protocol).
///
/// Only MIME index 1 (`text/uri-list`, the single type we offer) is served;
/// anything else — including a request with no stored payload — gets a
/// `t=R` error reply with a POSIX error name. Successful payloads are
/// base64-encoded and chunked at [`DND_CHUNK_BYTES`], ending with an empty
/// `m=0` escape.
fn build_dnd_data_replies(index: u32, payload: Option<&str>) -> Vec<Vec<u8>> {
    let payload = match (index, payload) {
        (1, Some(p)) => p,
        _ => {
            return vec![format!("\x1b]72;t=R:x={index};ENOENT\x1b\\").into_bytes()];
        }
    };
    let encoded = base64_encode(payload.as_bytes());
    let mut replies: Vec<Vec<u8>> = encoded
        .as_bytes()
        .chunks(DND_CHUNK_BYTES)
        .map(|chunk| {
            let mut r = format!("\x1b]72;t=r:x={index}:m=1;").into_bytes();
            r.extend_from_slice(chunk);
            r.extend_from_slice(b"\x1b\\");
            r
        })
        .collect();
    replies.push(format!("\x1b]72;t=r:x={index}:m=0;\x1b\\").into_bytes());
    replies
}

/// Converts an absolute filesystem path into a `text/uri-list` entry
/// (a percent-encoded `file://` URI). Windows drive paths become
/// `file:///C:/...` with forward slashes.
pub fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut uri = String::from("file://");
    if !normalized.starts_with('/') {
        // Windows drive form (`C:/...`) needs the extra root slash.
        uri.push('/');
    }
    for b in normalized.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => uri.push(b as char),
            b'/' | b':' | b'-' | b'_' | b'.' | b'~' => uri.push(b as char),
            _ => uri.push_str(&format!("%{b:02X}")),
        }
    }
    uri
}

/// Process-global theme default colors reported by the client
/// (`ClientToServer::SetThemeColors`, roadmap #10b).
///
/// The theme is process-global by design — the most recent client report
/// wins, exactly like the message semantics — so a global cell avoids
/// threading a handle through every `Pane::spawn*` call site. Each PTY
/// reader applies the value to its `Screen` before parsing a burst, so OSC
/// 10/11 queries answer with the colors that are actually rendered.
pub fn theme_default_colors() -> &'static Mutex<Option<([u8; 3], [u8; 3])>> {
    static THEME_DEFAULT_COLORS: Mutex<Option<([u8; 3], [u8; 3])>> = Mutex::new(None);
    &THEME_DEFAULT_COLORS
}

/// Store the client-reported theme defaults (called from the IPC dispatcher).
pub fn set_theme_default_colors(fg: [u8; 3], bg: [u8; 3]) {
    if let Ok(mut guard) = theme_default_colors().lock() {
        *guard = Some((fg, bg));
    }
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
    ///
    /// Shared with the reader thread via `Arc` so it can write
    /// device-attribute / DSR replies back to the PTY (v1.9.5 fix —
    /// previously the reader had no way to answer pwsh's startup queries).
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
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
    /// Whether the running application opted in to the kitty drag-and-drop
    /// protocol (OSC 72 `t=a`). Mirrored from the VT screen by the reader
    /// thread; consulted by the IPC dispatcher on a file drop.
    dnd_enabled: Arc<std::sync::atomic::AtomicBool>,
    /// Stored drop payload (`text/uri-list`) awaiting the application's
    /// OSC 72 data request; cleared on completion.
    dnd_payload: Arc<Mutex<Option<String>>>,
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
    /// Per-pane scrollback mirror (F3 / ADR-0008), oldest line first.
    ///
    /// The PTY reader owns the `VtParser`, which emits lines as they scroll off
    /// the top of the primary screen. The reader appends those to this mirror
    /// every burst (capped at `scrollback_limit`, oldest dropped), so the
    /// main-thread plugin read API (`read_scrollback`) can serve history that
    /// would otherwise stay trapped in the reader thread. The visible screen is
    /// served from `latest_grid`; this covers only what has scrolled away.
    latest_scrollback: Arc<Mutex<VecDeque<Vec<Cell>>>>,
    /// Retention cap for `latest_scrollback` (shared with the reader thread).
    //
    // `allow(dead_code)`: mutated only through `set_scrollback_limit()`, which
    // the server does not call yet (the mirror uses its default cap); the
    // Unix-only scrollback test exercises it. Remove once config plumbs it.
    #[allow(dead_code)]
    scrollback_limit: Arc<AtomicUsize>,
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
        // v1.9.4 — log enough about the spawned child to confirm later from
        // user logs whether the shell process actually started, with what
        // command line and working directory, and at what size.
        info!(
            "pane {}: spawned shell={:?} args={:?} pid={:?} cols={} rows={} cwd={:?}",
            id, shell, args, pid, cols, rows, effective_cwd
        );

        // Acquire the write handle (one-shot) and the read handle.
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer()?));
        let writer_clone = Arc::clone(&writer);
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

        // kitty drag-and-drop protocol (OSC 72): opt-in flag mirror + stored
        // drop payload, both shared with the reader thread.
        let dnd_enabled: Arc<std::sync::atomic::AtomicBool> =
            Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dnd_enabled_clone = Arc::clone(&dnd_enabled);
        let dnd_payload: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let dnd_payload_clone = Arc::clone(&dnd_payload);
        let current_cwd_clone = Arc::clone(&current_cwd);

        // Share the latest full-grid snapshot (v1.9.3 fix). Initialised as an
        // empty grid; the reader thread overwrites it as bytes arrive.
        let latest_grid: Arc<Mutex<Grid>> = Arc::new(Mutex::new(Grid::new(cols, rows)));
        let latest_grid_clone = Arc::clone(&latest_grid);

        // Per-pane scrollback mirror (F3 / ADR-0008). The reader thread appends
        // scrolled-off lines here every burst so the main-thread read API can
        // serve them.
        let latest_scrollback: Arc<Mutex<VecDeque<Vec<Cell>>>> =
            Arc::new(Mutex::new(VecDeque::new()));
        let latest_scrollback_clone = Arc::clone(&latest_scrollback);
        let scrollback_limit: Arc<AtomicUsize> =
            Arc::new(AtomicUsize::new(DEFAULT_PANE_SCROLLBACK_LINES));
        let scrollback_limit_clone = Arc::clone(&scrollback_limit);

        // Launch the PTY reader thread.
        tokio::task::spawn_blocking(move || {
            // v1.9.4 — confirm the reader actually started. Critical for
            // diagnosing the case where `spawn_blocking` itself fails to
            // schedule (silent stall).
            info!("pane {}: PTY reader thread started", pane_id);
            let mut parser = VtParser::new(cols, rows);
            let mut buf = [0u8; 4096];
            // v1.9.4 — once-only flags for diagnostic logs so production
            // sessions emit one `first PTY output` / `first GridDiff` line
            // and then stay quiet.
            let mut logged_first_output = false;
            let mut logged_first_diff = false;

            /// Helper that sends a message via the `broadcast::Sender` (sync, no waiting).
            fn send_msg(tx: &broadcast::Sender<ServerToClient>, msg: ServerToClient) {
                // Ignore when there are no receivers (no client attached).
                let _ = tx.send(msg);
            }

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        info!("pane {}: PTY reached EOF", pane_id);
                        break;
                    }
                    Ok(n) => {
                        if !logged_first_output {
                            // v1.9.5 — include a short hex preview (first
                            // 32 bytes) so the user log shows exactly which
                            // control sequences the shell sent. Previously
                            // only the byte count was logged, leaving us
                            // blind to whether the chunk contained a DA/DSR
                            // query that demanded a reply.
                            let preview: String = buf[..n.min(32)]
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(" ");
                            info!(
                                "pane {}: first PTY output ({} bytes) hex={}{}",
                                pane_id,
                                n,
                                preview,
                                if n > 32 { " …" } else { "" }
                            );
                            logged_first_output = true;
                        }
                        // Roadmap #10b — apply the client-reported theme
                        // defaults before parsing so OSC 10/11 queries inside
                        // this burst answer with the rendered colors.
                        // Idempotent and cheap (one mutex read per burst).
                        if let Ok(guard) = theme_default_colors().lock()
                            && let Some((fg, bg)) = *guard
                        {
                            parser.screen_mut().set_default_colors(fg, bg);
                        }
                        parser.advance(&buf[..n]);

                        // v1.9.5 — drain Primary/Secondary DA + DSR replies
                        // the parser queued and write them back to the PTY.
                        // Doing it right after `advance` minimises the time
                        // PowerShell + PSReadLine spends waiting on the
                        // reply before drawing the prompt.
                        let responses = parser.screen_mut().take_pending_responses();
                        if !responses.is_empty() {
                            if let Ok(mut w) = writer_clone.lock() {
                                for reply in &responses {
                                    if let Err(e) = w.write_all(reply) {
                                        error!(
                                            "pane {}: PTY response write failed: {}",
                                            pane_id, e
                                        );
                                        break;
                                    }
                                }
                                let _ = w.flush();
                            }
                            info!(
                                "pane {}: replied to {} terminal-capability query/queries",
                                pane_id,
                                responses.len()
                            );
                        }

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

                        // Mirror the kitty DnD opt-in flag (OSC 72 t=a / t=A)
                        // and answer queued data requests from the stored
                        // drop payload.
                        dnd_enabled_clone.store(
                            parser.screen().dnd_enabled(),
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        for req in parser.screen_mut().take_pending_dnd_requests() {
                            let replies = match req {
                                nexterm_vt::DndRequest::Data { index } => {
                                    let payload =
                                        dnd_payload_clone.lock().ok().and_then(|g| g.clone());
                                    build_dnd_data_replies(index, payload.as_deref())
                                }
                                nexterm_vt::DndRequest::Complete => {
                                    if let Ok(mut g) = dnd_payload_clone.lock() {
                                        *g = None;
                                    }
                                    Vec::new()
                                }
                            };
                            if !replies.is_empty()
                                && let Ok(mut w) = writer_clone.lock()
                            {
                                for r in &replies {
                                    let _ = w.write_all(r);
                                }
                                let _ = w.flush();
                            }
                        }

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
                            if !logged_first_diff {
                                info!(
                                    "pane {}: first GridDiff ({} dirty rows)",
                                    pane_id,
                                    dirty.len()
                                );
                                logged_first_diff = true;
                            }
                            let (cursor_col, cursor_row) = parser.screen().cursor();

                            // v1.9.3 fix: keep the full-grid snapshot fresh so a
                            // client attaching after this burst still sees the
                            // current screen via `make_full_refresh`. Without
                            // this the parser state is trapped in the reader
                            // thread and late-attachers get an empty grid.
                            //
                            // Audit round 3 (P1): apply only the changed rows
                            // instead of cloning the entire parser grid on every
                            // burst. Under heavy output (`yes`, `cat largefile`)
                            // the full clone copied every cell each time; here we
                            // touch only the dirty rows we already computed. The
                            // reader's parser is never resized, so `latest_grid`
                            // and the dirty rows always share dimensions. Cursor
                            // and hyperlinks are synced too because `GridDiff`
                            // carries neither, so a late attach relies on this
                            // snapshot for both.
                            if let Ok(mut g) = latest_grid_clone.lock() {
                                for d in &dirty {
                                    g.apply_dirty_row(d);
                                }
                                g.cursor_col = cursor_col;
                                g.cursor_row = cursor_row;
                                g.hyperlinks.clone_from(&parser.screen().grid().hyperlinks);
                            }

                            // F3 / ADR-0008: move lines that scrolled off during
                            // this burst into the pane-side scrollback mirror,
                            // capped at the configured limit (oldest dropped).
                            let scrolled = parser.screen_mut().take_scrolled_off_lines();
                            if !scrolled.is_empty()
                                && let Ok(mut sb) = latest_scrollback_clone.lock()
                            {
                                let limit = scrollback_limit_clone.load(Ordering::Relaxed);
                                if limit == 0 {
                                    sb.clear();
                                } else {
                                    sb.extend(scrolled);
                                    while sb.len() > limit {
                                        sb.pop_front();
                                    }
                                }
                            }
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

                        // Send a pointer-shape change (OSC 22).
                        if let Some(shape) = parser.screen_mut().take_pending_pointer_shape() {
                            let msg = ServerToClient::PointerShapeChanged { pane_id, shape };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send an OSC 9;4 progress report.
                        if let Some((state, progress)) = parser.screen_mut().take_pending_progress()
                        {
                            let msg = ServerToClient::ProgressChanged {
                                pane_id,
                                state,
                                progress,
                            };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // Send a dynamic-color override snapshot (OSC 4/10/11,
                        // roadmap #10b).
                        if let Some(colors) = parser.screen_mut().take_color_overrides_if_changed()
                        {
                            let msg = ServerToClient::PaneColorsChanged {
                                pane_id,
                                fg: colors.fg,
                                bg: colors.bg,
                                palette: colors.palette,
                            };
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
            latest_scrollback,
            scrollback_limit,
            dnd_enabled,
            dnd_payload,
        })
    }

    /// Sets the scrollback mirror retention cap (lines). The server calls this
    /// from the `scrollback_lines` config value; 0 disables scrollback
    /// retention. Trimming to a smaller cap happens on the next burst.
    // `allow(dead_code)`: consumed by F3 Phase 3 server wiring (and by the
    // Unix-only scrollback test). Remove the allow when Phase 3 lands.
    #[allow(dead_code)]
    pub fn set_scrollback_limit(&self, limit: usize) {
        self.scrollback_limit.store(limit, Ordering::Relaxed);
    }

    /// Returns a snapshot of the pane's scrollback mirror, oldest line first
    /// (F3 / ADR-0008). Used by `SessionManager::pane_snapshot` to serve the
    /// plugin `read_scrollback` host import.
    pub fn scrollback_snapshot(&self) -> Vec<Vec<Cell>> {
        match self.latest_scrollback.lock() {
            Ok(sb) => sb.iter().cloned().collect(),
            Err(poisoned) => poisoned.into_inner().iter().cloned().collect(),
        }
    }

    /// Whether the running application opted in to the kitty drag-and-drop
    /// protocol (OSC 72 `t=a`).
    pub fn dnd_enabled(&self) -> bool {
        self.dnd_enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Offer a drop to the opted-in application (kitty DnD protocol).
    ///
    /// Stores the `text/uri-list` payload for the application's upcoming
    /// data request and sends the drop event. Coordinates are reported as
    /// (0,0): winit does not expose the drop position, and the protocol
    /// treats coordinates as advisory.
    pub fn offer_dnd_drop(&self, uri_list: String) -> Result<()> {
        if let Ok(mut guard) = self.dnd_payload.lock() {
            *guard = Some(uri_list);
        }
        self.write_input(b"\x1b]72;t=M:x=0:y=0:o=1;text/uri-list\x1b\\")
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

    // ------------------------------------------------------------------------
    // Phase 2c (UI/UX v2): foreground process name lookup.
    //
    // Returns the executable name (e.g. "vim", "ssh", "node") of the topmost
    // foreground descendant of this pane's shell, or `None` when the shell is
    // sitting at the prompt or detection failed. Used by `SessionManager`'s
    // 1 Hz ticker to broadcast `ServerToClient::ProcessChanged` updates so
    // the client can render a Nerd Font glyph next to each tab label.
    //
    // Errors / unsupported OSes return `None` instead of panicking, matching
    // the `has_foreground_process` discipline above.
    // ------------------------------------------------------------------------

    /// Public entry point. Dispatches to the OS-specific implementation.
    pub fn foreground_process_name(&self) -> Option<String> {
        self.read_foreground_process_name()
    }

    /// Linux: read `/proc/{pid}/stat` for `tpgid`, then `/proc/{tpgid}/comm`.
    ///
    /// `tpgid` is the controlling terminal's foreground process group ID. When
    /// it equals the shell's own `pgrp`, the shell itself owns the terminal —
    /// the user is at the prompt and there is no foreground job. Returning
    /// `None` in that case keeps the icon off.
    ///
    /// `comm` is truncated to 15 chars by the kernel — that is what the glyph
    /// map keys against.
    #[cfg(target_os = "linux")]
    fn read_foreground_process_name(&self) -> Option<String> {
        let pid = self.pid?;
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
        // stat format: "pid (comm) state ppid pgrp session tty_nr tpgid flags ..."
        // `comm` may itself contain spaces and parens, so split on the last ") ".
        let (_, after) = stat.rsplit_once(") ")?;
        let fields: Vec<&str> = after.split_whitespace().collect();
        // [0]=state, [1]=ppid, [2]=pgrp, [3]=session, [4]=tty_nr, [5]=tpgid
        let pgrp = fields.get(2).and_then(|s| s.parse::<i32>().ok())?;
        let tpgid = fields.get(5).and_then(|s| s.parse::<i32>().ok())?;
        // `tpgid <= 0`: no controlling terminal; `tpgid == pgrp`: shell at prompt.
        if tpgid <= 0 || tpgid == pgrp {
            return None;
        }
        let comm = std::fs::read_to_string(format!("/proc/{}/comm", tpgid)).ok()?;
        let trimmed = comm.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// macOS: `ps -A -o pid=,ppid=,comm=` once; walk the parent-map from the
    /// shell PID and return the deepest descendant's `comm` basename.
    ///
    /// Trade-off: spawning `ps` costs ~10-30 ms. The 1 Hz session-wide ticker
    /// in `SessionManager` calls this per pane, so total cost is bounded by
    /// `pane_count × 30 ms` per second on macOS. The `ps` fan-out can be
    /// reused across panes — a future optimisation if pane counts grow.
    #[cfg(target_os = "macos")]
    fn read_foreground_process_name(&self) -> Option<String> {
        let pid = self.pid?;
        let output = std::process::Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,comm="])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse into (pid, ppid, comm) triples.
        let entries: Vec<(u32, u32, String)> = stdout
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let pid_v = parts.next()?.parse::<u32>().ok()?;
                let ppid_v = parts.next()?.parse::<u32>().ok()?;
                // Rest is the comm (may contain spaces / paths).
                let comm = parts.collect::<Vec<_>>().join(" ");
                Some((pid_v, ppid_v, comm))
            })
            .collect();
        // Walk down from `pid`, taking the first matching child each step
        // until no further descendant exists. The leaf is the foreground.
        let mut current_pid = pid;
        let mut leaf_comm: Option<String> = None;
        // Cap iterations to avoid pathological cycles (shouldn't happen).
        for _ in 0..64 {
            let child = entries.iter().find(|(_, ppid, _)| *ppid == current_pid);
            match child {
                Some((cpid, _, comm)) => {
                    // Strip leading path so "/usr/bin/vim" → "vim".
                    let basename = std::path::Path::new(comm.as_str())
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(comm.as_str())
                        .to_string();
                    leaf_comm = Some(basename);
                    current_pid = *cpid;
                }
                None => break,
            }
        }
        leaf_comm
    }

    /// Windows: enumerate every process with `Toolhelp32Snapshot`, build a
    /// (pid → first_child) shortcut, and walk from the shell PID to the
    /// deepest descendant. Strip `.exe` so the glyph map matches "vim"
    /// rather than "vim.exe".
    ///
    /// The snapshot is system-wide; enumerating 1000+ processes typically
    /// costs ~1 ms. Acceptable at the 1 Hz polling cadence.
    #[cfg(windows)]
    fn read_foreground_process_name(&self) -> Option<String> {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };

        let shell_pid = self.pid?;

        // SAFETY: same contract as `read_has_foreground_process` (above) —
        // the snapshot handle is checked for INVALID_HANDLE_VALUE before
        // any subsequent API call, and `HandleGuard` closes it on drop.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        struct HandleGuard(HANDLE);
        impl Drop for HandleGuard {
            fn drop(&mut self) {
                // SAFETY: HANDLE is valid from CreateToolhelp32Snapshot;
                // the INVALID_HANDLE_VALUE branch returned earlier.
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
        let _guard = HandleGuard(snapshot);

        // SAFETY: PROCESSENTRY32W is POD; zero-init is fine.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: snapshot is valid; entry.dwSize is set.
        if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
            return None;
        }

        // Collect every (pid, ppid, exe_name) triple. Building the full list
        // (instead of streaming a single descent) costs one allocation but
        // keeps the descent logic identical to the macOS path.
        let mut entries: Vec<(u32, u32, String)> = Vec::new();
        loop {
            // Process32 returns a UTF-16 wide string; the length is implicit
            // (zero-terminated). Find the terminator and decode the prefix.
            let name_len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
            entries.push((entry.th32ProcessID, entry.th32ParentProcessID, exe));
            // SAFETY: snapshot + entry valid; loop exits on Process32NextW = 0.
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }

        // Descend from the shell PID. Same logic as macOS.
        let mut current_pid = shell_pid;
        let mut leaf: Option<String> = None;
        for _ in 0..64 {
            let child = entries.iter().find(|(_, ppid, _)| *ppid == current_pid);
            match child {
                Some((cpid, _, name)) => {
                    // Strip `.exe` (case-insensitive) so "Code.exe" → "Code".
                    let bare = if let Some(stripped) = name.strip_suffix(".exe") {
                        stripped
                    } else if let Some(stripped) = name.strip_suffix(".EXE") {
                        stripped
                    } else {
                        name.as_str()
                    };
                    leaf = Some(bare.to_string());
                    current_pid = *cpid;
                }
                None => break,
            }
        }
        leaf
    }

    /// Other OSes: not supported.
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    fn read_foreground_process_name(&self) -> Option<String> {
        None
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

    /// F3 / ADR-0008: lines that scroll off the top must land in the pane's
    /// scrollback mirror so the main-thread read API can serve them.
    #[cfg(not(windows))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scrollback_mirror_captures_scrolled_off_lines() {
        use std::time::{Duration, Instant};

        // Print 40 numbered lines into a 6-row terminal so ~34 lines scroll off.
        let (shell, args): (&str, Vec<String>) = (
            "/bin/sh",
            vec![
                "-c".into(),
                "for i in $(seq 1 40); do echo LINE$i; done".into(),
            ],
        );
        let (tx, _rx) = tokio::sync::broadcast::channel::<ServerToClient>(2048);
        let pane = Pane::spawn_with_id(1, 20, 6, tx, shell, &args).expect("spawn failed");
        pane.set_scrollback_limit(1000);

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let sb = pane.scrollback_snapshot();
            let text: String = sb.iter().flat_map(|row| row.iter().map(|c| c.ch)).collect();
            // The earliest lines must have scrolled into the mirror.
            if text.contains("LINE1") && text.contains("LINE30") {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "scrollback mirror missing expected lines within 5 s ({} lines captured): {:?}",
                    sb.len(),
                    text.chars().take(200).collect::<String>()
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[cfg(test)]
mod dnd_tests {
    use super::{base64_encode, build_dnd_data_replies, path_to_file_uri};

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn data_replies_are_chunked_and_terminated() {
        // A payload whose base64 form exceeds one chunk must produce
        // multiple m=1 escapes followed by the empty m=0 terminator.
        let payload = "x".repeat(6000); // → 8000 base64 bytes → 2 chunks
        let replies = build_dnd_data_replies(1, Some(&payload));
        assert_eq!(replies.len(), 3);
        assert!(replies[0].starts_with(b"\x1b]72;t=r:x=1:m=1;"));
        assert!(replies[1].starts_with(b"\x1b]72;t=r:x=1:m=1;"));
        assert_eq!(replies[2], b"\x1b]72;t=r:x=1:m=0;\x1b\\".to_vec());
        // Every escape stays within the protocol payload limit.
        for r in &replies {
            assert!(r.len() <= 4096 + 32, "oversized escape: {}", r.len());
        }
    }

    #[test]
    fn unknown_index_or_missing_payload_get_an_error_reply() {
        let replies = build_dnd_data_replies(2, Some("data"));
        assert_eq!(replies, vec![b"\x1b]72;t=R:x=2;ENOENT\x1b\\".to_vec()]);
        let replies = build_dnd_data_replies(1, None);
        assert_eq!(replies, vec![b"\x1b]72;t=R:x=1;ENOENT\x1b\\".to_vec()]);
    }

    #[test]
    fn file_uris_are_percent_encoded_per_platform_shape() {
        assert_eq!(
            path_to_file_uri(r"C:\Users\alice\my file.txt"),
            "file:///C:/Users/alice/my%20file.txt"
        );
        assert_eq!(
            path_to_file_uri("/home/alice/a.txt"),
            "file:///home/alice/a.txt"
        );
        // Non-ASCII bytes are percent-encoded (UTF-8, e.g. "é" = C3 A9).
        assert_eq!(path_to_file_uri("/tmp/é"), "file:///tmp/%C3%A9");
    }
}
