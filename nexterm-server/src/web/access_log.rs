//! アクセスログ記録モジュール
//!
//! 各リクエストのアクセス情報を構造化ログとして記録する。
//! ログファイルが設定されている場合は CSV 形式で追記し、
//! 未設定の場合は tracing を通じてサーバーログに出力する。
//!
//! # 出力フォーマット（CSV）
//! ```csv
//! timestamp,remote_addr,method,path,status,auth_method,user_id
//! 2024-01-01T12:00:00Z,192.168.1.1,GET,/ws,101,totp,
//! 2024-01-01T12:00:01Z,192.168.1.2,POST,/auth/login,302,oauth:github,octocat
//! ```
//!
//! # ローテーション（Sprint 3-3 後半）
//!
//! `AccessLogConfig.max_size_mib` と `max_generations` を設定するとサイズベースの
//! ローテーションが有効になる。書き込み前にファイルサイズをチェックし、閾値を
//! 超えていれば `<file>.1`, `<file>.2`, ... `<file>.{max_generations}` の順に
//! ローテーションする。`compress = true` の場合は世代ファイルを `.gz` 圧縮する。

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use flate2::{Compression, write::GzEncoder};
use nexterm_config::AccessLogConfig;
use tracing::{info, warn};

/// CSV ヘッダー行
const CSV_HEADER: &str = "timestamp,remote_addr,method,path,status,auth_method,user_id";

/// アクセスログエントリ
#[derive(Debug, Clone)]
pub struct AccessLogEntry {
    pub remote_addr: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub auth_method: String,
    pub user_id: String,
}

/// アクセスログライター（共有可能）
#[derive(Clone)]
pub struct AccessLogger {
    /// ログファイルパス（None = tracing のみ）
    file_path: Option<PathBuf>,
    /// ファイル書き込みの排他制御
    file_lock: Arc<Mutex<()>>,
    enabled: bool,
    /// ローテーション閾値（バイト単位）。0 = ローテーション無効
    max_size_bytes: u64,
    /// 保持する世代数（0 = ローテーション無効）
    max_generations: u32,
    /// gzip 圧縮を有効化するか
    compress: bool,
}

impl AccessLogger {
    /// 設定からアクセスログライターを生成する
    pub fn new(config: &AccessLogConfig) -> Self {
        let file_path = config.file.as_ref().map(PathBuf::from);
        let max_size_bytes = config.max_size_mib.saturating_mul(1024 * 1024);

        // ファイルが設定されている場合はヘッダー行を書き込む（ファイルが新規の場合のみ）
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

    /// アクセスログエントリを記録する
    ///
    /// HIGH H-7 対策: `path` のクエリ文字列（`?...`）は除去する。
    /// OAuth コールバックの `?code=...&state=...` や TOTP リダイレクトの
    /// `?token=...` 等の機密情報がログに残るのを防ぐ。
    pub fn log(&self, entry: &AccessLogEntry) {
        if !self.enabled {
            return;
        }

        let timestamp = chrono_now();
        // クエリ文字列を除去（OAuth code / state / token 等の機密漏れ防止）
        let safe_path = strip_query_string(&entry.path);

        // tracing には常に出力する
        info!(
            target: "nexterm::access",
            remote_addr = %entry.remote_addr,
            method = %entry.method,
            path = %safe_path,
            status = entry.status,
            auth_method = %entry.auth_method,
            user_id = %entry.user_id,
            "アクセスログ"
        );

        // ファイルへの追記
        if let Some(ref path) = self.file_path {
            let Ok(_lock) = self.file_lock.lock() else {
                return;
            };
            let _lock = _lock;

            // ローテーション判定（サイズ超過時のみ）
            if self.rotation_enabled()
                && let Err(e) = self.rotate_if_needed(path)
            {
                warn!(
                    "アクセスログのローテーションに失敗しました（処理は継続）: {}",
                    e
                );
            }

            // ローテーション後にファイルが消えていればヘッダーを再書き込み
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

    /// ローテーションが有効か
    fn rotation_enabled(&self) -> bool {
        self.max_size_bytes > 0 && self.max_generations > 0
    }

    /// ファイルサイズが閾値を超えていればローテーションを実行する
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

/// ローテーション本体（テスト容易性のため AccessLogger から分離）
///
/// 1. 最古の世代ファイル（`.{N}` または `.{N}.gz`）を削除
/// 2. 古いものから順に `.{i}` → `.{i+1}` にリネーム
/// 3. 元ファイルを `.1`（または gzip 圧縮して `.1.gz`）に移動
fn rotate_files(path: &Path, max_generations: u32, compress: bool) -> std::io::Result<()> {
    if max_generations == 0 {
        return Ok(());
    }

    // 最古の世代を削除（圧縮/非圧縮両方の可能性を考慮）
    let oldest = generation_path(path, max_generations, compress);
    if oldest.exists() {
        std::fs::remove_file(&oldest)?;
    }
    // 圧縮設定切り替え時の古い拡張子も掃除する
    let oldest_alt = generation_path(path, max_generations, !compress);
    if oldest_alt.exists() {
        std::fs::remove_file(&oldest_alt)?;
    }

    // .{N-1} → .{N}, ..., .1 → .2 へ順次リネーム（圧縮/非圧縮両方を移動）
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

    // 元ファイルを .1 に移動（必要なら gzip 圧縮）
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

/// 世代番号 `n` に対応するファイルパスを返す
fn generation_path(base: &Path, n: u32, compress: bool) -> PathBuf {
    let mut s = base.as_os_str().to_owned();
    s.push(format!(".{n}"));
    if compress {
        s.push(".gz");
    }
    PathBuf::from(s)
}

/// `src` を gzip 圧縮して `dest` に書き出す（src は呼び出し側で削除）
fn gzip_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    let input = std::fs::read(src)?;
    let out = std::fs::File::create(dest)?;
    let mut encoder = GzEncoder::new(out, Compression::default());
    encoder.write_all(&input)?;
    encoder.finish()?;
    Ok(())
}

/// CSV ヘッダー行を作成する（既存ファイルが空 or 不在の場合）
fn ensure_header(path: &Path) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{CSV_HEADER}");
    }
}

/// パスからクエリ文字列（`?...`）とフラグメント（`#...`）を除去する。
///
/// HIGH H-7: OAuth コールバック・TOTP リダイレクト等の機密情報が
/// アクセスログに残るのを防ぐ。
fn strip_query_string(path: &str) -> String {
    let without_fragment = path.split('#').next().unwrap_or(path);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    without_query.to_string()
}

/// CSV フィールドのエスケープ（カンマや改行を含む場合はクォートで囲む）
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 現在時刻を ISO 8601 形式で返す（外部クレートなしの簡易実装）
fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Unix timestamp → UTC 日時に変換（うるう秒は無視）
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;

    // グレゴリオ暦への簡易変換（1970-01-01 起点）
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}

/// Unix エポックからの日数を (年, 月, 日) に変換する
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // アルゴリズム: http://howardhinnant.github.io/date_algorithms.html
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

    // ---- days_to_ymd テスト ----

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

    // ---- AccessLogger テスト ----

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

    // ---- HIGH H-7: クエリ文字列除去テスト ----

    #[test]
    fn strip_query_string_は通常パスをそのまま返す() {
        assert_eq!(strip_query_string("/ws"), "/ws");
        assert_eq!(strip_query_string("/auth/login"), "/auth/login");
    }

    #[test]
    fn strip_query_string_は_oauth_code_を除去する() {
        let input = "/auth/callback?code=abc123secret&state=xyz789";
        assert_eq!(strip_query_string(input), "/auth/callback");
    }

    #[test]
    fn strip_query_string_は_token_クエリを除去する() {
        let input = "/ws?session=main&token=verysecretvalue";
        assert_eq!(strip_query_string(input), "/ws");
    }

    #[test]
    fn strip_query_string_はフラグメントも除去する() {
        let input = "/page#section";
        assert_eq!(strip_query_string(input), "/page");
    }

    #[test]
    fn strip_query_string_は空文字列を許容する() {
        assert_eq!(strip_query_string(""), "");
    }

    // ---- ローテーション機能（Sprint 3-3 後半）テスト ----

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
    fn generation_path_適用_拡張子なし() {
        let p = generation_path(Path::new("/var/log/access.log"), 1, false);
        assert_eq!(p, PathBuf::from("/var/log/access.log.1"));
    }

    #[test]
    fn generation_path_適用_gzip拡張子付き() {
        let p = generation_path(Path::new("/var/log/access.log"), 3, true);
        assert_eq!(p, PathBuf::from("/var/log/access.log.3.gz"));
    }

    #[test]
    fn rotate_files_は元ファイルを_1_に移動する_非圧縮() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("access.log");
        std::fs::write(&base, b"old content").unwrap();

        rotate_files(&base, 3, false).unwrap();

        assert!(!base.exists(), "元ファイルは消えるはず");
        let gen1 = generation_path(&base, 1, false);
        assert!(gen1.exists(), ".1 が作成されるはず");
        assert_eq!(std::fs::read(&gen1).unwrap(), b"old content");
    }

    #[test]
    fn rotate_files_は古い世代を順送りする() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("access.log");
        // 既存の世代ファイルを作成
        std::fs::write(generation_path(&base, 1, false), b"gen1").unwrap();
        std::fs::write(generation_path(&base, 2, false), b"gen2").unwrap();
        // 元ファイル
        std::fs::write(&base, b"current").unwrap();

        rotate_files(&base, 3, false).unwrap();

        // 順送り後: .1 = current, .2 = gen1, .3 = gen2
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
    fn rotate_files_は_max_generations_を超えた古い世代を削除する() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("access.log");
        // 既に .3 まで存在（max_generations = 3）
        std::fs::write(generation_path(&base, 1, false), b"gen1").unwrap();
        std::fs::write(generation_path(&base, 2, false), b"gen2").unwrap();
        std::fs::write(generation_path(&base, 3, false), b"gen3-oldest").unwrap();
        std::fs::write(&base, b"current").unwrap();

        rotate_files(&base, 3, false).unwrap();

        // .3 (最古 gen3) は削除、.1 = current, .2 = 旧 gen1, .3 = 旧 gen2
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
    fn rotate_files_は_gzip_圧縮を適用する() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("access.log");
        let content = b"hello world this is access log content";
        std::fs::write(&base, content).unwrap();

        rotate_files(&base, 3, true).unwrap();

        assert!(!base.exists(), "元ファイルは消えるはず");
        let gen1_gz = generation_path(&base, 1, true);
        assert!(gen1_gz.exists(), ".1.gz が作成されるはず");

        // gzip ファイルをデコードして元のバイト列と一致することを確認
        let raw = std::fs::read(&gen1_gz).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(&raw[..]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, content);
    }

    #[test]
    fn log_は閾値超過時にローテーションを実行する() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("access.log");

        // 1 MiB 閾値・2 世代保持・非圧縮
        let cfg = config_with(&path, 1, 2, false);
        let logger = AccessLogger::new(&cfg);

        // ファイルを 1 MiB 超のサイズにする
        let large = vec![b'x'; 1024 * 1024 + 100];
        std::fs::write(&path, &large).unwrap();

        // log() 1 回でローテーションが発動する
        logger.log(&sample_entry());

        let gen1 = generation_path(&path, 1, false);
        assert!(gen1.exists(), ".1 にローテーションされているはず");
        // 元ファイルは新規作成されてヘッダー + 1 行のみ
        let new_content = std::fs::read_to_string(&path).unwrap();
        assert!(new_content.starts_with(CSV_HEADER));
        assert!(new_content.contains("/ws"));
    }

    #[test]
    fn log_は閾値未満ではローテーションしない() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("access.log");
        let cfg = config_with(&path, 1, 2, false);
        let logger = AccessLogger::new(&cfg);

        // 小さい既存ファイル
        std::fs::write(&path, b"existing\n").unwrap();
        logger.log(&sample_entry());

        let gen1 = generation_path(&path, 1, false);
        assert!(!gen1.exists(), "ローテーションされないはず");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("existing"));
        assert!(content.contains("/ws"));
    }
}
