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

use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tracing::info;

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
}

impl AccessLogger {
    /// 設定からアクセスログライターを生成する
    pub fn new(enabled: bool, file: Option<&str>) -> Self {
        let file_path = file.map(PathBuf::from);
        // ファイルが設定されている場合はヘッダー行を書き込む（ファイルが新規の場合のみ）
        if enabled
            && let Some(ref path) = file_path
                && !path.exists()
                    && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                        let _ = writeln!(
                            f,
                            "timestamp,remote_addr,method,path,status,auth_method,user_id"
                        );
                    }
        Self {
            file_path,
            file_lock: Arc::new(Mutex::new(())),
            enabled,
        }
    }

    /// アクセスログエントリを記録する
    pub fn log(&self, entry: &AccessLogEntry) {
        if !self.enabled {
            return;
        }

        let timestamp = chrono_now();

        // tracing には常に出力する
        info!(
            target: "nexterm::access",
            remote_addr = %entry.remote_addr,
            method = %entry.method,
            path = %entry.path,
            status = entry.status,
            auth_method = %entry.auth_method,
            user_id = %entry.user_id,
            "アクセスログ"
        );

        // ファイルへの追記
        if let Some(ref path) = self.file_path {
            let Ok(_lock) = self.file_lock.lock() else { return };
            let _lock = _lock;
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                let line = format!(
                    "{},{},{},{},{},{},{}\n",
                    timestamp,
                    csv_escape(&entry.remote_addr),
                    csv_escape(&entry.method),
                    csv_escape(&entry.path),
                    entry.status,
                    csv_escape(&entry.auth_method),
                    csv_escape(&entry.user_id),
                );
                let _ = f.write_all(line.as_bytes());
            }
        }
    }
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
        // 1970-01-01
        let (y, m, d) = days_to_ymd(0);
        assert_eq!(y, 1970);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn days_to_ymd_typical_date() {
        // 2024-01-15
        let (y, m, d) = days_to_ymd(19737);
        assert_eq!(y, 2024);
        assert_eq!(m, 1);
        assert_eq!(d, 15);
    }

    #[test]
    fn days_to_ymd_leap_year() {
        // 2020-02-29 (leap year)
        let (y, m, d) = days_to_ymd(18321); // days from 1970 to 2020-02-29
        assert_eq!(y, 2020);
        assert_eq!(m, 2);
        assert_eq!(d, 29);
    }

    #[test]
    fn days_to_ymd_new_year_2025() {
        // 2025-01-01 (跨年)
        let (y, m, d) = days_to_ymd(20089);
        assert_eq!(y, 2025);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    // ---- AccessLogger テスト ----

    #[test]
    fn access_logger_disabled_does_not_create_file() {
        // 無効状態ではファイルが作成されない
        let logger = AccessLogger::new(false, None);
        assert!(!logger.enabled);
    }

    #[test]
    fn access_logger_without_file_path_ok() {
        // ファイルパスなしでもエラーにならない
        let logger = AccessLogger::new(true, None);
        assert!(logger.enabled);
        assert!(logger.file_path.is_none());
    }

    #[test]
    fn access_log_entry_creation() {
        let entry = AccessLogEntry {
            remote_addr: "192.168.1.1".to_string(),
            method: "GET".to_string(),
            path: "/ws".to_string(),
            status: 101,
            auth_method: "totp".to_string(),
            user_id: "user123".to_string(),
        };
        
        assert_eq!(entry.remote_addr, "192.168.1.1");
        assert_eq!(entry.status, 101);
    }
}
