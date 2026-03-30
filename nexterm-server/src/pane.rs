//! ペイン — PTY プロセスと仮想グリッドを管理する最小単位
//!
//! PTY 出力チャネルは `Arc<broadcast::Sender>` で保持する。
//! broadcast により複数クライアントへ同時送信でき、再アタッチ時の差し替え不要。

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
#[cfg(unix)]
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::broadcast;
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
    /// ログファイルのパス（ローテーション用）
    path: String,
    /// 現在のファイルに書き込んだバイト数
    written_bytes: u64,
    /// ローテーション上限バイト数（0 = 無制限）
    max_bytes: u64,
    /// 保持する最大ファイル数
    max_files: u32,
}

impl LogWriterInner {
    fn new(file: File, timestamp: bool, strip_ansi: bool, path: String, max_bytes: u64, max_files: u32) -> Self {
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

    /// ローテーションが必要かどうかを確認して実行する
    fn rotate_if_needed(&mut self) -> std::io::Result<()> {
        if self.max_bytes == 0 || self.written_bytes < self.max_bytes {
            return Ok(());
        }
        // バッファをフラッシュしてからローテーション
        self.writer.flush()?;
        // 古いファイルをシフト: .{max_files-1} を削除、.N を .{N+1} にリネーム
        let path = self.path.clone();
        let max = self.max_files;
        // 一番古いファイルを削除
        let oldest = format!("{}.{}", path, max);
        let _ = std::fs::remove_file(&oldest);
        // N-1 → N にシフト
        for i in (1..max).rev() {
            let from = format!("{}.{}", path, i);
            let to = format!("{}.{}", path, i + 1);
            let _ = std::fs::rename(&from, &to);
        }
        // 現在のファイルを .1 にリネーム
        let _ = std::fs::rename(&path, format!("{}.1", path));
        // 新しいファイルを作成
        let new_file = File::create(&path)?;
        self.writer = BufWriter::new(new_file);
        self.written_bytes = 0;
        Ok(())
    }

    /// バイト列を書き込む（改行単位でタイムスタンプ付加・ANSI 除去を適用）
    fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        // ローテーションが必要か確認する
        self.rotate_if_needed()?;

        if !self.timestamp && !self.strip_ansi {
            // 最適化: 特別な処理なしに直接書き込む
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

/// ログファイル名テンプレートを展開する
///
/// 利用可能なプレースホルダー:
///   {session}  — セッション名
///   {pane}     — ペイン ID
///   {datetime} — 起動時刻 (YYYYMMDD_HHMMSS)
pub fn expand_log_filename_template(
    template: &str,
    session: &str,
    pane_id: u32,
) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // UTC 時刻を手動計算する（chrono 不使用）
    let secs_in_day = now % 86400;
    let h = secs_in_day / 3600;
    let m = (secs_in_day / 60) % 60;
    let s = secs_in_day % 60;
    // 簡易日付計算（Unix エポックから日数 → 年月日）
    let days = now / 86400;
    let (year, month, day) = days_to_ymd(days);
    let datetime = format!("{:04}{:02}{:02}_{:02}{:02}{:02}", year, month, day, h, m, s);

    template
        .replace("{session}", session)
        .replace("{pane}", &pane_id.to_string())
        .replace("{datetime}", &datetime)
}

/// Unix エポックからの日数を年月日に変換する（グレゴリオ暦）
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // アルゴリズム: http://howardhinnant.github.io/date_algorithms.html
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

/// asciicast v2 形式ライター
pub struct AsciicastWriter {
    file: BufWriter<File>,
    started_at: Instant,
}

impl AsciicastWriter {
    /// 新しい AsciicastWriter を作成してヘッダー行を書き込む
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
        Ok(Self { file: w, started_at: Instant::now() })
    }

    /// PTY 出力データを asciicast イベント行として書き込む
    pub fn write_output(&mut self, data: &[u8]) -> std::io::Result<()> {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        let text = String::from_utf8_lossy(data);
        // serde_json で JSON 文字列にエスケープする
        let escaped = serde_json::to_string(&*text)
            .unwrap_or_else(|_| "\"\"".to_string());
        writeln!(self.file, "[{:.6},\"o\",{}]", elapsed, escaped)?;
        Ok(())
    }

    /// バッファをフラッシュする
    pub fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

type AsciicastWriterHandle = Arc<Mutex<Option<AsciicastWriter>>>;

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

/// 全クライアントへのブロードキャスト送信チャネル（sync 送信、Mutex 不要）
type SharedTx = Arc<broadcast::Sender<ServerToClient>>;

/// ペインの状態
pub struct Pane {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    /// 子プロセスの PID（Linux: /proc/{pid}/cwd から作業ディレクトリ取得に使用）
    #[allow(dead_code)]
    pid: Option<u32>,
    /// PTY 出力先チャネル（再アタッチ時に差し替え可能）
    #[allow(dead_code)]
    shared_tx: SharedTx,
    /// PTY マスタ（リサイズ用）
    master: Box<dyn MasterPty + Send>,
    /// PTY 書き込みハンドル（キー入力転送用）
    writer: Mutex<Box<dyn Write + Send>>,
    /// テキストログファイルライター（録音中のみ Some）
    log_writer: LogWriter,
    /// バイナリログファイルライター（binary_log=true 時のみ Some）
    binary_log_writer: LogWriter,
    /// asciicast v2 ライター（録音中のみ Some）
    asciicast_writer: AsciicastWriterHandle,
}

impl Pane {
    /// 新しいペインを生成してシェルを起動する
    pub fn spawn(
        cols: u16,
        rows: u16,
        initial_tx: broadcast::Sender<ServerToClient>,
        shell: &str,
    ) -> Result<Self> {
        Self::spawn_impl(new_pane_id(), cols, rows, initial_tx, shell, None)
    }

    /// 指定 ID でペインを生成する（BSP 分割時に ID を事前確定するために使用）
    pub fn spawn_with_id(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: broadcast::Sender<ServerToClient>,
        shell: &str,
    ) -> Result<Self> {
        Self::spawn_impl(id, cols, rows, initial_tx, shell, None)
    }

    /// 指定 ID・作業ディレクトリでペインを生成する（スナップショット復元時に使用）
    pub fn spawn_with_cwd(
        id: u32,
        cols: u16,
        rows: u16,
        initial_tx: broadcast::Sender<ServerToClient>,
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
        initial_tx: broadcast::Sender<ServerToClient>,
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

        // broadcast::Sender を Arc で共有する（Mutex 不要、sync 送信）
        let shared_tx: SharedTx = Arc::new(initial_tx);
        let shared_tx_clone = Arc::clone(&shared_tx);
        let pane_id = id;

        // ログライターを Arc<Mutex> で共有する
        let log_writer: LogWriter = Arc::new(Mutex::new(None));
        let log_writer_clone = Arc::clone(&log_writer);

        // バイナリログライターを Arc<Mutex> で共有する
        let binary_log_writer: LogWriter = Arc::new(Mutex::new(None));
        let binary_log_writer_clone = Arc::clone(&binary_log_writer);

        // asciicast ライターを Arc<Mutex> で共有する
        let asciicast_writer: AsciicastWriterHandle = Arc::new(Mutex::new(None));
        let asciicast_writer_clone = Arc::clone(&asciicast_writer);

        // PTY 読み取りスレッドを起動する
        tokio::task::spawn_blocking(move || {
            let mut parser = VtParser::new(cols, rows);
            let mut buf = [0u8; 4096];

            /// broadcast::Sender でメッセージを送信するヘルパー（sync、待機なし）
            fn send_msg(
                tx: &broadcast::Sender<ServerToClient>,
                msg: ServerToClient,
            ) {
                // 受信者がいない場合は無視（クライアント未接続時）
                let _ = tx.send(msg);
            }

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        debug!("ペイン {} の PTY が EOF になりました", pane_id);
                        break;
                    }
                    Ok(n) => {
                        parser.advance(&buf[..n]);

                        // 録音中であれば生バイト列をログファイルに書き込む
                        if let Ok(mut guard) = log_writer_clone.lock()
                            && let Some(w) = guard.as_mut()
                                && let Err(e) = w.write(&buf[..n]) {
                                    error!("ログ書き込みエラー: {}", e);
                                    *guard = None;
                                }

                        // バイナリログ: raw PTY bytes をそのまま保存する
                        if let Ok(mut guard) = binary_log_writer_clone.lock()
                            && let Some(w) = guard.as_mut()
                                && let Err(e) = w.write(&buf[..n]) {
                                    error!("バイナリログ書き込みエラー: {}", e);
                                    *guard = None;
                                }

                        // asciicast 録音中であれば書き込む
                        if let Ok(mut guard) = asciicast_writer_clone.lock()
                            && let Some(w) = guard.as_mut()
                                && let Err(e) = w.write_output(&buf[..n]) {
                                    error!("asciicast 書き込みエラー: {}", e);
                                    *guard = None;
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
                            send_msg(&shared_tx_clone, msg);
                        }

                        // BEL を受信していればクライアントに通知する
                        if parser.screen_mut().take_pending_bell() {
                            let msg = ServerToClient::Bell { pane_id };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // タイトル変更通知を送信する（OSC 0/1/2）
                        if let Some(title) = parser.screen_mut().take_pending_title() {
                            let msg = ServerToClient::TitleChanged { pane_id, title };
                            send_msg(&shared_tx_clone, msg);
                        }

                        // デスクトップ通知を送信する（OSC 9）
                        if let Some((title, body)) = parser.screen_mut().take_pending_notification() {
                            let msg = ServerToClient::DesktopNotification { pane_id, title, body };
                            send_msg(&shared_tx_clone, msg);
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
                            send_msg(&shared_tx_clone, msg);
                        }
                    }
                    Err(e) => {
                        error!("PTY 読み取りエラー: {}", e);
                        break;
                    }
                }
            }

            // Fix 2: PTY EOF 時にプロセスグループへ SIGHUP を送信してゾンビプロセスを防ぐ
            #[cfg(unix)]
            if let Some(pid_val) = pid
                && pid_val > 0 {
                    // SAFETY: kill() は有効な pid に対して安全。pgid は pid と同一（setsid 未使用）。
                    unsafe { libc::kill(pid_val as libc::pid_t, libc::SIGHUP) };
                    debug!("ペイン {}: PID {} に SIGHUP を送信しました", pane_id, pid_val);
                }

            // Fix 1: PTY EOF / シェル終了時に PaneClosed を送信する
            debug!("ペイン {} の PTY ループが終了しました。PaneClosed を送信します", pane_id);
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
        })
    }

    /// Full Refresh グリッドを生成する（アタッチ時用）
    pub fn make_full_refresh(&self) -> Grid {
        Grid::new(self.cols, self.rows)
    }

    /// PTY 出力チャネルを差し替える — broadcast では再アタッチ時の差し替えは不要（no-op）
    #[allow(dead_code)]
    pub fn update_tx(&self, _new_tx: broadcast::Sender<ServerToClient>) {
        // broadcast::Sender は共有されるため、クライアント再アタッチ時に差し替え不要
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
        self.start_recording_with_rotation(path, timestamp, strip_ansi, 0, 5)
    }

    /// PTY 出力のファイル録音をローテーション設定付きで開始する
    ///
    /// `max_size_mb` が 0 の場合はローテーションしない。`max_files` は保持ファイル数。
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
        let mut guard = self.log_writer.lock()
            .map_err(|e| anyhow::anyhow!("log_writer ロック取得に失敗しました: {}", e))?;
        *guard = Some(LogWriterInner::new(file, timestamp, strip_ansi, path.to_string(), max_bytes, max_files));
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

    /// LogConfig の設定（テンプレート・バイナリログ）を使って録音を開始する
    ///
    /// `base_path` はテンプレートを使わない場合のデフォルトパス。
    pub fn start_recording_with_config(
        &self,
        base_path: &str,
        session: &str,
        log_config: &nexterm_config::LogConfig,
    ) -> Result<()> {
        // テンプレートが設定されていれば展開する
        let resolved_path = if let Some(ref tmpl) = log_config.file_name_template {
            // テンプレートを使用してファイル名を生成する
            let filename = expand_log_filename_template(tmpl, session, self.id);
            if let Some(log_dir) = &log_config.log_dir {
                format!("{}/{}", log_dir.trim_end_matches('/'), filename)
            } else {
                filename
            }
        } else {
            base_path.to_string()
        };

        // 親ディレクトリを作成する
        if let Some(parent) = std::path::Path::new(&resolved_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        // テキストログを開始する
        self.start_recording_with_options(&resolved_path, log_config.timestamp, log_config.strip_ansi)?;

        // バイナリログが有効な場合は raw バイナリファイルも開始する
        if log_config.binary_log {
            let bin_path = format!("{}.bin", resolved_path.trim_end_matches(".log"));
            let bin_file = File::create(&bin_path)?;
            // バイナリログは timestamp/strip_ansi なしで raw bytes を保存する
            let mut guard = self.binary_log_writer.lock()
                .map_err(|e| anyhow::anyhow!("binary_log_writer ロック取得失敗: {}", e))?;
            *guard = Some(LogWriterInner::new(bin_file, false, false, bin_path.clone(), 0, 0));
            info!("ペイン {} のバイナリログを開始しました: {}", self.id, bin_path);
        }

        Ok(())
    }

    /// asciicast v2 形式での録画を開始する
    pub fn start_asciicast(&self, path: &str) -> Result<()> {
        let writer = AsciicastWriter::new(path, self.cols, self.rows)?;
        let mut guard = self.asciicast_writer.lock()
            .map_err(|e| anyhow::anyhow!("asciicast_writer ロック取得に失敗しました: {}", e))?;
        *guard = Some(writer);
        info!("ペイン {} の asciicast 録画を開始しました: {}", self.id, path);
        Ok(())
    }

    /// asciicast v2 形式での録画を停止する
    ///
    /// バッファをフラッシュしてからファイルを閉じる。
    pub fn stop_asciicast(&self) -> Result<()> {
        let mut guard = self.asciicast_writer.lock()
            .map_err(|e| anyhow::anyhow!("asciicast_writer ロック取得に失敗しました: {}", e))?;
        if let Some(mut w) = guard.take() {
            w.flush()?;
            info!("ペイン {} の asciicast 録画を停止しました", self.id);
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

    /// Windows 実装: PowerShell で子プロセスの CWD を取得する
    #[cfg(windows)]
    fn read_working_dir(&self) -> Option<std::path::PathBuf> {
        let pid = self.pid?;
        // PowerShell の (Get-Process).Path はバイナリパスなので Split-Path で親を取得する
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

    /// その他の OS: 作業ディレクトリ取得は非対応
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
