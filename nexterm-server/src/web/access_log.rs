//! Access log module.
//!
//! Records per-request access info as a structured log.
//! When a log file is configured, entries are appended in CSV form.
//! Otherwise they are emitted to the server log via tracing.
//!
//! # Output format (CSV)
//! ```csv
//! timestamp,remote_addr,method,path,status,auth_method,user_id
//! 2024-01-01T12:00:00Z,192.168.1.1,GET,/ws,101,totp,
//! 2024-01-01T12:00:01Z,192.168.1.2,POST,/auth/login,302,oauth:github,octocat
//! ```
//!
//! # Rotation (Sprint 3-3 second half)
//!
//! Setting `AccessLogConfig.max_size_mib` and `max_generations` enables size-based rotation.
//! The file size is checked before each write and, when the threshold is exceeded,
//! rotated as `<file>.1`, `<file>.2`, ... `<file>.{max_generations}`.
//! With `compress = true`, generation files are gzip-compressed.

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use flate2::{Compression, write::GzEncoder};
use nexterm_config::AccessLogConfig;
use tracing::{info, warn};

/// CSV header line.
const CSV_HEADER: &str = "timestamp,remote_addr,method,path,status,auth_method,user_id";

/// Access log entry.
#[derive(Debug, Clone)]
pub struct AccessLogEntry {
    pub remote_addr: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub auth_method: String,
    pub user_id: String,
}

/// Access log writer (cloneable).
#[derive(Clone)]
pub struct AccessLogger {
    /// Log file path (`None` = tracing only).
    file_path: Option<PathBuf>,
    /// Exclusive control for file writes.
    file_lock: Arc<Mutex<()>>,
    enabled: bool,
    /// Rotation threshold in bytes. `0` = rotation disabled.
    max_size_bytes: u64,
    /// Number of generations to keep (`0` = rotation disabled).
    max_generations: u32,
    /// Whether to enable gzip compression.
    compress: bool,
}

impl AccessLogger {
    /// Construct the access log writer from configuration.
    pub fn new(config: &AccessLogConfig) -> Self {
        let file_path = config.file.as_ref().map(PathBuf::from);
        let max_size_bytes = config.max_size_mib.saturating_mul(1024 * 1024);

        // Write the header line when the file is configured (only when it does not exist yet).
        if config.enabled
            && let Some(ref path) = file_path
            && !path.exists()
        {
            ensure_header(path);
        }

        Self {
            file_path,
            file_lock: Arc::new(Mutex::new(())),
            enabled: config.enabled,
            max_size_bytes,
            max_generations: config.max_generations,
            compress: config.compress,
        }
    }

    /// Record an access log entry.
    ///
    /// HIGH H-7: strip the query string (`?...`) from `path` so that the OAuth callback
    /// (`?code=...&state=...`) or TOTP redirect (`?token=...`) does not leak secrets into the log.
    pub fn log(&self, entry: &AccessLogEntry) {
        if !self.enabled {
            return;
        }

        let timestamp = chrono_now();
        // Strip the query string (prevents leaking OAuth code / state / token).
        let safe_path = strip_query_string(&entry.path);

        // Always emit via tracing.
        info!(
            target: "nexterm::access",
            remote_addr = %entry.remote_addr,
            method = %entry.method,
            path = %safe_path,
            status = entry.status,
            auth_method = %entry.auth_method,
            user_id = %entry.user_id,
            "access log"
        );

        // Append to file.
        if let Some(ref path) = self.file_path {
            let Ok(_lock) = self.file_lock.lock() else {
                return;
            };
            let _lock = _lock;

            // Decide rotation (only when the size threshold is exceeded).
            if self.rotation_enabled()
                && let Err(e) = self.rotate_if_needed(path)
            {
                warn!("access log rotation failed (continuing): {}", e);
            }

            // If the file disappeared after rotation, rewrite the header.
            if !path.exists() {
                ensure_header(path);
            }

            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                let line = format!(
                    "{},{},{},{},{},{},{}\n",
                    timestamp,
                    csv_escape(&entry.remote_addr),
                    csv_escape(&entry.method),
                    csv_escape(&safe_path),
                    entry.status,
                    csv_escape(&entry.auth_method),
                    csv_escape(&entry.user_id),
                );
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    /// Whether rotation is enabled.
    fn rotation_enabled(&self) -> bool {
        self.max_size_bytes > 0 && self.max_generations > 0
    }

    /// Rotate when the file size exceeds the threshold.
    fn rotate_if_needed(&self, path: &Path) -> std::io::Result<()> {
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if metadata.len() < self.max_size_bytes {
            return Ok(());
        }
        rotate_files(path, self.max_generations, self.compress)
    }
}

/// Rotation core (split out of `AccessLogger` for testability).
///
/// 1. Delete the oldest generation file (`.{N}` or `.{N}.gz`).
/// 2. Rename older generations in order: `.{i}` -> `.{i+1}`.
/// 3. Move the original file to `.1` (gzip-compressed to `.1.gz` when enabled).
fn rotate_files(path: &Path, max_generations: u32, compress: bool) -> std::io::Result<()> {
    if max_generations == 0 {
        return Ok(());
    }

    // Delete the oldest generation (account for both compressed and uncompressed forms).
    let oldest = generation_path(path, max_generations, compress);
    if oldest.exists() {
        std::fs::remove_file(&oldest)?;
    }
    // Also clean up the other extension in case the compress setting was toggled.
    let oldest_alt = generation_path(path, max_generations, !compress);
    if oldest_alt.exists() {
        std::fs::remove_file(&oldest_alt)?;
    }

    // Rename in sequence: `.{N-1}` -> `.{N}`, ..., `.1` -> `.2` (move both compressed and uncompressed).
    for i in (1..max_generations).rev() {
        for ext_compress in [compress, !compress] {
            let from = generation_path(path, i, ext_compress);
            if from.exists() {
                let to = generation_path(path, i + 1, ext_compress);
                if to.exists() {
                    let _ = std::fs::remove_file(&to);
                }
                std::fs::rename(&from, &to)?;
            }
        }
    }

    // Move the original file to `.1` (gzip-compress if requested).
    let dest = generation_path(path, 1, compress);
    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
    }
    if compress {
        gzip_file(path, &dest)?;
        std::fs::remove_file(path)?;
    } else {
        std::fs::rename(path, &dest)?;
    }
    Ok(())
}

/// Return the file path that corresponds to generation number `n`.
fn generation_path(base: &Path, n: u32, compress: bool) -> PathBuf {
    let mut s = base.as_os_str().to_owned();
    s.push(format!(".{n}"));
    if compress {
        s.push(".gz");
    }
    PathBuf::from(s)
}

/// Gzip-compress `src` to `dest` (the caller deletes `src`).
fn gzip_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    let input = std::fs::read(src)?;
    let out = std::fs::File::create(dest)?;
    let mut encoder = GzEncoder::new(out, Compression::default());
    encoder.write_all(&input)?;
    encoder.finish()?;
    Ok(())
}

/// Create the CSV header line (when the existing file is empty or absent).
fn ensure_header(path: &Path) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{CSV_HEADER}");
    }
}

/// Strip the query string (`?...`) and fragment (`#...`) from a path.
///
/// HIGH H-7: prevents OAuth callback / TOTP redirect secrets from leaking into the access log.
fn strip_query_string(path: &str) -> String {
    let without_fragment = path.split('#').next().unwrap_or(path);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    without_query.to_string()
}

/// Escape a CSV field (wraps in quotes when commas or newlines are present).
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Return the current time in ISO 8601 format (simple implementation, no external crate).
fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Unix timestamp -> UTC date/time conversion (ignores leap seconds).
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;

    // Simple conversion into the Gregorian calendar (starting from 1970-01-01).
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}

/// Convert days since the Unix epoch into a (year, month, day) tuple.
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm: http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::TempDir;

    fn config_with(
        file: &Path,
        max_size_mib: u64,
        max_gen: u32,
        compress: bool,
    ) -> AccessLogConfig {
        AccessLogConfig {
            enabled: true,
            file: Some(file.to_string_lossy().into_owned()),
            max_size_mib,
            max_generations: max_gen,
            compress,
        }
    }

    fn sample_entry() -> AccessLogEntry {
        AccessLogEntry {
            remote_addr: "192.168.1.1".to_string(),
            method: "GET".to_string(),
            path: "/ws".to_string(),
            status: 101,
            auth_method: "totp".to_string(),
            user_id: "user123".to_string(),
        }
    }

    #[test]
    fn csv_escape_no_escaping_needed() {
        let input = "simple text";
        assert_eq!(csv_escape(input), "simple text");
    }

    #[test]
    fn csv_escape_escapes_comma() {
        let input = "192.168.1.1,port 8080";
        assert_eq!(csv_escape(input), "\"192.168.1.1,port 8080\"");
    }

    #[test]
    fn csv_escape_escapes_quote() {
        let input = r#"user "admin""#;
        assert_eq!(csv_escape(input), "\"user \"\"admin\"\"\"");
    }

    #[test]
    fn csv_escape_escapes_newline() {
        let input = "line1\nline2";
        assert_eq!(csv_escape(input), "\"line1\nline2\"");
    }

    #[test]
    fn csv_escape_empty_string() {
        assert_eq!(csv_escape(""), "");
    }

    // ---- days_to_ymd tests ----

    #[test]
    fn days_to_ymd_epoch_day_0() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!(y, 1970);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn days_to_ymd_typical_date() {
        let (y, m, d) = days_to_ymd(19737);
        assert_eq!(y, 2024);
        assert_eq!(m, 1);
        assert_eq!(d, 15);
    }

    #[test]
    fn days_to_ymd_leap_year() {
        let (y, m, d) = days_to_ymd(18321);
        assert_eq!(y, 2020);
        assert_eq!(m, 2);
        assert_eq!(d, 29);
    }

    #[test]
    fn days_to_ymd_new_year_2025() {
        let (y, m, d) = days_to_ymd(20089);
        assert_eq!(y, 2025);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    // ---- AccessLogger tests ----

    #[test]
    fn access_logger_disabled_does_not_create_file() {
        let logger = AccessLogger::new(&AccessLogConfig::default());
        assert!(!logger.enabled);
    }

    #[test]
    fn access_logger_without_file_path_ok() {
        let cfg = AccessLogConfig {
            enabled: true,
            ..Default::default()
        };
        let logger = AccessLogger::new(&cfg);
        assert!(logger.enabled);
        assert!(logger.file_path.is_none());
    }

    #[test]
    fn access_log_entry_creation() {
        let entry = sample_entry();
        assert_eq!(entry.remote_addr, "192.168.1.1");
        assert_eq!(entry.status, 101);
    }

    // ---- HIGH H-7: query-string stripping tests ----

    #[test]
    fn strip_query_string_returns_plain_path_unchanged() {
        assert_eq!(strip_query_string("/ws"), "/ws");
        assert_eq!(strip_query_string("/auth/login"), "/auth/login");
    }

    #[test]
    fn strip_query_string_removes_oauth_code() {
        let input = "/auth/callback?code=abc123secret&state=xyz789";
        assert_eq!(strip_query_string(input), "/auth/callback");
    }

    #[test]
    fn strip_query_string_removes_token_query() {
        let input = "/ws?session=main&token=verysecretvalue";
        assert_eq!(strip_query_string(input), "/ws");
    }

    #[test]
    fn strip_query_string_removes_fragment_too() {
        let input = "/page#section";
        assert_eq!(strip_query_string(input), "/page");
    }

    #[test]
    fn strip_query_string_accepts_empty_string() {
        assert_eq!(strip_query_string(""), "");
    }

    // ---- Rotation (Sprint 3-3 second half) tests ----

    #[test]
    fn rotation_disabled_when_max_size_is_zero() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("access.log");
        let cfg = config_with(&path, 0, 7, false);
        let logger = AccessLogger::new(&cfg);
        assert!(!logger.rotation_enabled());
    }

    #[test]
    fn rotation_disabled_when_max_generations_is_zero() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("access.log");
        let cfg = config_with(&path, 10, 0, false);
        let logger = AccessLogger::new(&cfg);
        assert!(!logger.rotation_enabled());
    }

    #[test]
    fn generation_path_applies_without_extension() {
        let p = generation_path(Path::new("/var/log/access.log"), 1, false);
        assert_eq!(p, PathBuf::from("/var/log/access.log.1"));
    }

    #[test]
    fn generation_path_applies_with_gzip_extension() {
        let p = generation_path(Path::new("/var/log/access.log"), 3, true);
        assert_eq!(p, PathBuf::from("/var/log/access.log.3.gz"));
    }

    #[test]
    fn rotate_files_moves_original_to_dot_one_uncompressed() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("access.log");
        std::fs::write(&base, b"old content").unwrap();

        rotate_files(&base, 3, false).unwrap();

        assert!(!base.exists(), "the original file should disappear");
        let gen1 = generation_path(&base, 1, false);
        assert!(gen1.exists(), ".1 should be created");
        assert_eq!(std::fs::read(&gen1).unwrap(), b"old content");
    }

    #[test]
    fn rotate_files_shifts_older_generations() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("access.log");
        // Create existing generation files.
        std::fs::write(generation_path(&base, 1, false), b"gen1").unwrap();
        std::fs::write(generation_path(&base, 2, false), b"gen2").unwrap();
        // Original file.
        std::fs::write(&base, b"current").unwrap();

        rotate_files(&base, 3, false).unwrap();

        // After shifting: .1 = current, .2 = gen1, .3 = gen2.
        assert_eq!(
            std::fs::read(generation_path(&base, 1, false)).unwrap(),
            b"current"
        );
        assert_eq!(
            std::fs::read(generation_path(&base, 2, false)).unwrap(),
            b"gen1"
        );
        assert_eq!(
            std::fs::read(generation_path(&base, 3, false)).unwrap(),
            b"gen2"
        );
    }

    #[test]
    fn rotate_files_deletes_generations_beyond_max() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("access.log");
        // Already have files up to .3 (max_generations = 3).
        std::fs::write(generation_path(&base, 1, false), b"gen1").unwrap();
        std::fs::write(generation_path(&base, 2, false), b"gen2").unwrap();
        std::fs::write(generation_path(&base, 3, false), b"gen3-oldest").unwrap();
        std::fs::write(&base, b"current").unwrap();

        rotate_files(&base, 3, false).unwrap();

        // .3 (oldest gen3) is deleted; .1 = current, .2 = old gen1, .3 = old gen2.
        assert_eq!(
            std::fs::read(generation_path(&base, 1, false)).unwrap(),
            b"current"
        );
        assert_eq!(
            std::fs::read(generation_path(&base, 2, false)).unwrap(),
            b"gen1"
        );
        assert_eq!(
            std::fs::read(generation_path(&base, 3, false)).unwrap(),
            b"gen2"
        );
    }

    #[test]
    fn rotate_files_applies_gzip_compression() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("access.log");
        let content = b"hello world this is access log content";
        std::fs::write(&base, content).unwrap();

        rotate_files(&base, 3, true).unwrap();

        assert!(!base.exists(), "the original file should disappear");
        let gen1_gz = generation_path(&base, 1, true);
        assert!(gen1_gz.exists(), ".1.gz should be created");

        // Decode the gzip file and verify it matches the original bytes.
        let raw = std::fs::read(&gen1_gz).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(&raw[..]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, content);
    }

    #[test]
    fn log_triggers_rotation_when_threshold_exceeded() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("access.log");

        // 1 MiB threshold, keep 2 generations, no compression.
        let cfg = config_with(&path, 1, 2, false);
        let logger = AccessLogger::new(&cfg);

        // Inflate the file beyond 1 MiB.
        let large = vec![b'x'; 1024 * 1024 + 100];
        std::fs::write(&path, &large).unwrap();

        // A single `log()` call should trigger rotation.
        logger.log(&sample_entry());

        let gen1 = generation_path(&path, 1, false);
        assert!(gen1.exists(), ".1 should have been rotated");
        // The original file is recreated with the header + one entry.
        let new_content = std::fs::read_to_string(&path).unwrap();
        assert!(new_content.starts_with(CSV_HEADER));
        assert!(new_content.contains("/ws"));
    }

    #[test]
    fn log_does_not_rotate_below_threshold() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("access.log");
        let cfg = config_with(&path, 1, 2, false);
        let logger = AccessLogger::new(&cfg);

        // Small existing file.
        std::fs::write(&path, b"existing\n").unwrap();
        logger.log(&sample_entry());

        let gen1 = generation_path(&path, 1, false);
        assert!(!gen1.exists(), "should not have rotated");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("existing"));
        assert!(content.contains("/ws"));
    }
}
