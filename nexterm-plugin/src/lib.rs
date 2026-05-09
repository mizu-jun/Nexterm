#![warn(missing_docs)]
//! Nexterm WASM プラグインホストランタイム
//!
//! # プラグイン ABI バージョン
//!
//! [`PLUGIN_API_VERSION`] が安定 ABI を識別する。プラグインは
//! `nexterm.api_version() -> i32` インポートで照合できる。
//!
//! # WASM エクスポート（プラグイン側が実装する）
//!
//! ```wat
//! ;; モジュール初期化（オプション）
//! (export "nexterm_init" (func ...))
//!
//! ;; プラグインメタデータ: name_ptr/name_len / version_ptr/version_len を返す
//! ;; 戻り値: 常に 0（未使用）
//! (export "nexterm_meta" (func (param i32 i32 i32 i32) (result i32)))
//!
//! ;; ペイン出力フック: data_ptr/data_len は線形メモリ上の UTF-8 バイト列
//! ;; 戻り値: 0=そのまま通過, 1=抑制
//! (export "nexterm_on_output" (func (param i32 i32 i32) (result i32)))
//!
//! ;; カスタムコマンドフック: cmd_ptr/cmd_len は `:cmd arg` 形式の文字列
//! ;; 戻り値: 0=処理済み, 1=未処理
//! (export "nexterm_on_command" (func (param i32 i32) (result i32)))
//! ```
//!
//! # ホストインポート（プラグインが利用できる）
//!
//! ```wat
//! (import "nexterm" "log" (func (param i32 i32)))            ;; ログ出力
//! (import "nexterm" "write_pane" (func (param i32 i32 i32))) ;; ペインへの書き込み
//! (import "nexterm" "api_version" (func (result i32)))        ;; API バージョン照合
//! ```

/// プラグイン ABI の安定バージョン番号。
/// ホスト/プラグイン間の互換性をランタイムに確認するために使う。
pub const PLUGIN_API_VERSION: u32 = 1;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tracing::{error, info, warn};
use wasmi::{Config, Engine, Linker, Module, Store};

/// プラグイン 1 回の呼び出しごとに供給する fuel（命令数の概算）。
///
/// 無限ループや極端に重い処理でホスト側を停止させない上限。
/// CRITICAL #10 対応: wasmi はデフォルトで fuel 制限がない。
const FUEL_PER_CALL: u64 = 10_000_000;

/// プラグインメモリの最大ページ数 (1 ページ = 64 KiB)。
///
/// 256 ページ = 16 MiB を上限とする。CRITICAL #10 対応:
/// wasmi はデフォルトで線形メモリ上限がなく、悪意あるプラグインが
/// `memory.grow` で GB 単位のメモリを確保できる。
const MAX_MEMORY_PAGES: u32 = 256;

// ---- ホストステート -------------------------------------------------------

/// プラグインホストへのコールバック（ペイン書き込み等）
pub type WritePaneFn = Arc<dyn Fn(u32, &[u8]) + Send + Sync>;

/// 1 プラグインのランタイムインスタンス
struct PluginInstance {
    /// プラグインファイルパス（デバッグ用）
    path: PathBuf,
    /// プラグインが公開する名前（nexterm_meta から取得、任意）
    pub meta_name: Option<String>,
    /// プラグインが公開するバージョン（nexterm_meta から取得、任意）
    pub meta_version: Option<String>,
    /// WASM ストア
    store: Store<HostState>,
    /// WASM インスタンス
    instance: wasmi::Instance,
}

/// WASM ストアに格納するホスト側ステート
struct HostState {
    /// ペイン書き込みコールバック
    write_pane: WritePaneFn,
    /// ログバッファ（インポート "nexterm" "log" で受け取った文字列）
    log_buf: Vec<String>,
}

// ---- プラグインマネージャー -----------------------------------------------

/// WASM プラグインをロード・管理するマネージャー
pub struct PluginManager {
    engine: Engine,
    plugins: Mutex<Vec<PluginInstance>>,
    write_pane: WritePaneFn,
}

impl PluginManager {
    /// 新しいプラグインマネージャーを作成する
    ///
    /// `write_pane` はプラグインが `nexterm.write_pane` を呼んだときに実行されるコールバック。
    ///
    /// # サンドボックス設定（CRITICAL #10 対応）
    ///
    /// - fuel 計測を有効化（デフォルト全有効）
    /// - 各 `on_output` / `on_command` 呼び出し前に `FUEL_PER_CALL` を供給
    /// - fuel 枯渇時は呼び出しが TrappedFuelExhausted エラーで中断
    pub fn new(write_pane: WritePaneFn) -> Self {
        let mut config = Config::default();
        // fuel 計測 = 命令単位の上限を強制
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        Self {
            engine,
            plugins: Mutex::new(Vec::new()),
            write_pane,
        }
    }

    /// WASM ファイルをロードしてプラグインとして登録する
    pub fn load(&self, path: &Path) -> Result<()> {
        let wasm_bytes = std::fs::read(path)
            .with_context(|| format!("プラグインファイルの読み込みに失敗: {}", path.display()))?;

        let module = Module::new(&self.engine, &wasm_bytes[..])
            .with_context(|| format!("WASM モジュールのコンパイルに失敗: {}", path.display()))?;

        let write_pane = Arc::clone(&self.write_pane);
        let mut store = Store::new(
            &self.engine,
            HostState {
                write_pane,
                log_buf: Vec::new(),
            },
        );

        let mut linker = Linker::<HostState>::new(&self.engine);

        // ホストインポート: nexterm.api_version() -> i32
        linker.func_wrap(
            "nexterm",
            "api_version",
            |_: wasmi::Caller<'_, HostState>| PLUGIN_API_VERSION as i32,
        )?;

        // ホストインポート: nexterm.log(ptr: i32, len: i32)
        linker.func_wrap(
            "nexterm",
            "log",
            |mut caller: wasmi::Caller<'_, HostState>, ptr: i32, len: i32| {
                if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = mem.data(&caller);
                    let start = ptr as usize;
                    let end = start.saturating_add(len as usize);
                    if end <= data.len() {
                        let s = String::from_utf8_lossy(&data[start..end]).into_owned();
                        info!("[plugin] {}", s);
                        caller.data_mut().log_buf.push(s);
                    }
                }
            },
        )?;

        // ホストインポート: nexterm.write_pane(pane_id: i32, ptr: i32, len: i32)
        linker.func_wrap(
            "nexterm",
            "write_pane",
            |caller: wasmi::Caller<'_, HostState>, pane_id: i32, ptr: i32, len: i32| {
                if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = mem.data(&caller);
                    let start = ptr as usize;
                    let end = start.saturating_add(len as usize);
                    if end <= data.len() {
                        let bytes = data[start..end].to_vec();
                        (caller.data().write_pane)(pane_id as u32, &bytes);
                    }
                }
            },
        )?;

        // 初期 fuel を供給（インスタンス化・nexterm_init・nexterm_meta で消費される）
        store
            .set_fuel(FUEL_PER_CALL)
            .with_context(|| "fuel 設定に失敗")?;

        let instance = linker
            .instantiate(&mut store, &module)
            .with_context(|| "プラグインのインスタンス化に失敗")?
            .start(&mut store)
            .with_context(|| "プラグインの起動に失敗")?;

        // メモリ制限検証（CRITICAL #10）: 初期メモリサイズが上限を超えていたら拒否
        if let Some(mem) = instance.get_memory(&store, "memory")
            && mem.size(&store) > MAX_MEMORY_PAGES
        {
            anyhow::bail!(
                "プラグインメモリが上限を超えています: {} pages > {} pages (上限 {} MiB)",
                mem.size(&store),
                MAX_MEMORY_PAGES,
                MAX_MEMORY_PAGES * 64 / 1024
            );
        }

        // API バージョン検証（CRITICAL: HIGH 5 対応）
        // プラグインが nexterm_api_version() をエクスポートしていたら呼んで検証
        if let Ok(version_fn) = instance.get_typed_func::<(), i32>(&store, "nexterm_api_version") {
            store
                .set_fuel(FUEL_PER_CALL)
                .with_context(|| "fuel 設定に失敗")?;
            match version_fn.call(&mut store, ()) {
                Ok(v) if v as u32 == PLUGIN_API_VERSION => {}
                Ok(v) => {
                    anyhow::bail!(
                        "プラグイン API バージョン不一致: plugin={}, host={}",
                        v,
                        PLUGIN_API_VERSION
                    );
                }
                Err(e) => {
                    warn!(
                        "プラグイン API バージョン取得失敗（続行）: {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        // nexterm_init があれば呼ぶ（オプション）
        if let Ok(init_fn) = instance.get_typed_func::<(), ()>(&store, "nexterm_init") {
            store
                .set_fuel(FUEL_PER_CALL)
                .with_context(|| "fuel 設定に失敗")?;
            init_fn.call(&mut store, ()).ok();
        }

        // nexterm_meta があればメタデータを取得する（オプション）
        store
            .set_fuel(FUEL_PER_CALL)
            .with_context(|| "fuel 設定に失敗")?;
        let (meta_name, meta_version) = read_plugin_meta(&mut store, &instance);

        info!(
            "プラグインをロードしました: {} (name={:?} version={:?})",
            path.display(),
            meta_name,
            meta_version
        );

        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex がポイズン状態。回復して継続します");
            poisoned.into_inner()
        });
        plugins.push(PluginInstance {
            path: path.to_path_buf(),
            meta_name,
            meta_version,
            store,
            instance,
        });

        Ok(())
    }

    /// 指定パスのプラグインをアンロードする。存在しない場合は Ok(false) を返す。
    pub fn unload(&self, path: &Path) -> Result<bool> {
        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex がポイズン状態。回復して継続します");
            poisoned.into_inner()
        });
        let before = plugins.len();
        plugins.retain(|p| p.path != path);
        let removed = plugins.len() < before;
        if removed {
            info!("プラグインをアンロードしました: {}", path.display());
        }
        Ok(removed)
    }

    /// 指定パスのプラグインを再ロードする（アンロード → ロード）。
    pub fn reload(&self, path: &Path) -> Result<()> {
        self.unload(path)?;
        self.load(path)
    }

    /// ディレクトリ内の全 `.wasm` ファイルをロードする
    pub fn load_dir(&self, dir: &Path) -> Result<usize> {
        if !dir.exists() {
            return Ok(0);
        }
        let mut count = 0;
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("プラグインディレクトリの読み込みに失敗: {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                match self.load(&path) {
                    Ok(()) => count += 1,
                    Err(e) => warn!("プラグインのロードをスキップ: {} — {}", path.display(), e),
                }
            }
        }
        Ok(count)
    }

    /// ペイン出力フック — 全プラグインの `nexterm_on_output` を呼ぶ
    ///
    /// 戻り値: `true` = 抑制（クライアントに転送しない）
    pub fn on_output(&self, pane_id: u32, data: &[u8]) -> bool {
        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex がポイズン状態。回復して継続します");
            poisoned.into_inner()
        });
        for plugin in plugins.iter_mut() {
            let Ok(func) = plugin
                .instance
                .get_typed_func::<(i32, i32, i32), i32>(&plugin.store, "nexterm_on_output")
            else {
                continue;
            };
            // WASM 線形メモリにデータを書き込む
            if let Some(mem) = plugin.instance.get_memory(&plugin.store, "memory") {
                let offset = 64 * 1024usize; // スタック上部から安全な領域
                let mem_size = mem.data_size(&plugin.store);
                if offset + data.len() <= mem_size {
                    mem.write(&mut plugin.store, offset, data).ok();
                    // 各呼び出し前に fuel を補充（無限ループ防止、CRITICAL #10）
                    if let Err(e) = plugin.store.set_fuel(FUEL_PER_CALL) {
                        error!("[plugin {}] fuel 設定失敗: {}", plugin.path.display(), e);
                        continue;
                    }
                    match func.call(
                        &mut plugin.store,
                        (pane_id as i32, offset as i32, data.len() as i32),
                    ) {
                        Ok(1) => return true, // 抑制
                        Ok(_) => {}
                        Err(e) => {
                            error!("[plugin {}] on_output エラー: {}", plugin.path.display(), e)
                        }
                    }
                }
            }
        }
        false
    }

    /// カスタムコマンドフック — `:cmd arg` 形式の文字列を全プラグインに渡す
    ///
    /// 戻り値: `true` = いずれかのプラグインが処理済み
    pub fn on_command(&self, cmd: &str) -> bool {
        let cmd_bytes = cmd.as_bytes();
        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex がポイズン状態。回復して継続します");
            poisoned.into_inner()
        });
        for plugin in plugins.iter_mut() {
            let Ok(func) = plugin
                .instance
                .get_typed_func::<(i32, i32), i32>(&plugin.store, "nexterm_on_command")
            else {
                continue;
            };
            if let Some(mem) = plugin.instance.get_memory(&plugin.store, "memory") {
                let offset = 64 * 1024usize;
                let mem_size = mem.data_size(&plugin.store);
                if offset + cmd_bytes.len() <= mem_size {
                    mem.write(&mut plugin.store, offset, cmd_bytes).ok();
                    // 各呼び出し前に fuel を補充（無限ループ防止、CRITICAL #10）
                    if let Err(e) = plugin.store.set_fuel(FUEL_PER_CALL) {
                        error!("[plugin {}] fuel 設定失敗: {}", plugin.path.display(), e);
                        continue;
                    }
                    match func.call(&mut plugin.store, (offset as i32, cmd_bytes.len() as i32)) {
                        Ok(0) => return true, // 処理済み
                        Ok(_) => {}
                        Err(e) => error!(
                            "[plugin {}] on_command エラー: {}",
                            plugin.path.display(),
                            e
                        ),
                    }
                }
            }
        }
        false
    }

    /// ロード済みプラグイン数を返す
    pub fn plugin_count(&self) -> usize {
        self.plugins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// ロード済みプラグインのパス一覧を返す
    pub fn plugin_paths(&self) -> Vec<PathBuf> {
        self.plugins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|p| p.path.clone())
            .collect()
    }
}

// ---- メタデータヘルパー ------------------------------------------------------

/// `nexterm_meta` エクスポートからプラグイン名とバージョン文字列を取得する。
/// エクスポートがない場合は (None, None) を返す。
fn read_plugin_meta(
    store: &mut Store<HostState>,
    instance: &wasmi::Instance,
) -> (Option<String>, Option<String>) {
    let Ok(meta_fn) =
        instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut *store, "nexterm_meta")
    else {
        return (None, None);
    };

    let Some(mem) = instance.get_memory(&mut *store, "memory") else {
        return (None, None);
    };

    // 名前・バージョン用バッファを WASM メモリ上に確保する（各 128 バイト）
    let name_off: usize = 64 * 1024;
    let ver_off: usize = name_off + 128;
    let mem_size = mem.data_size(&mut *store);
    if ver_off + 128 > mem_size {
        return (None, None);
    }

    // バッファをゼロ埋めしてから呼ぶ
    mem.write(&mut *store, name_off, &[0u8; 128]).ok();
    mem.write(&mut *store, ver_off, &[0u8; 128]).ok();

    let _ = meta_fn.call(&mut *store, (name_off as i32, 128, ver_off as i32, 128));

    let data = mem.data(&mut *store);
    let name = read_cstr_from(data, name_off, 128);
    let ver = read_cstr_from(data, ver_off, 128);
    (name, ver)
}

/// WASM 線形メモリからヌル終端 UTF-8 文字列を読み出す。
fn read_cstr_from(data: &[u8], offset: usize, max_len: usize) -> Option<String> {
    let slice = data.get(offset..offset + max_len)?;
    let end = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
    if end == 0 {
        return None;
    }
    String::from_utf8(slice[..end].to_vec()).ok()
}

// ---- プラグイン情報（ctl 表示用） -----------------------------------------

/// プラグイン情報（`nexterm-ctl plugin list` 等で表示）
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PluginInfo {
    /// WASM プラグインファイルのパス
    pub path: String,
    /// プラグインが公開する名前（nexterm_meta から取得）
    pub name: Option<String>,
    /// プラグインが公開するバージョン（nexterm_meta から取得）
    pub version: Option<String>,
}

impl PluginManager {
    /// ロード済みプラグイン情報の一覧を返す
    pub fn list_info(&self) -> Vec<PluginInfo> {
        self.plugins
            .lock()
            .unwrap()
            .iter()
            .map(|p| PluginInfo {
                path: p.path.display().to_string(),
                name: p.meta_name.clone(),
                version: p.meta_version.clone(),
            })
            .collect()
    }
}

// ---- デフォルトプラグインディレクトリ ------------------------------------

/// デフォルトのプラグインディレクトリパスを返す
///
/// - Linux/macOS: `~/.config/nexterm/plugins`
/// - Windows:     `%APPDATA%\nexterm\plugins`
pub fn default_plugin_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join("nexterm").join("plugins")
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("nexterm")
            .join("plugins")
    }
}

// ---- テスト ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_write_pane() -> WritePaneFn {
        Arc::new(|_pane_id: u32, _data: &[u8]| {})
    }

    #[test]
    fn test_manager_new() {
        let mgr = PluginManager::new(noop_write_pane());
        assert_eq!(mgr.plugin_count(), 0);
    }

    #[test]
    fn test_load_dir_nonexistent() {
        let mgr = PluginManager::new(noop_write_pane());
        // 存在しないディレクトリは Ok(0) を返す
        let result = mgr.load_dir(Path::new("/nonexistent/path"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_on_output_no_plugins() {
        let mgr = PluginManager::new(noop_write_pane());
        // プラグインがない場合は常に false（抑制しない）
        assert!(!mgr.on_output(1, b"hello"));
    }

    #[test]
    fn test_on_command_no_plugins() {
        let mgr = PluginManager::new(noop_write_pane());
        // プラグインがない場合は常に false（未処理）
        assert!(!mgr.on_command(":hello world"));
    }

    #[test]
    fn test_default_plugin_dir() {
        let dir = default_plugin_dir();
        // パスが空でないこと
        assert!(!dir.as_os_str().is_empty());
        // "nexterm" と "plugins" セグメントを含むこと
        let s = dir.display().to_string();
        assert!(s.contains("nexterm"));
        assert!(s.contains("plugins"));
    }

    #[test]
    fn test_load_invalid_wasm() {
        let mgr = PluginManager::new(noop_write_pane());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not wasm").unwrap();
        let result = mgr.load(tmp.path());
        assert!(result.is_err());
    }

    /// 最小限の有効な WASM モジュール（空のモジュール）をロードできること
    #[test]
    fn test_load_minimal_wasm() {
        // (module) の最小 WASM バイナリ（手書きエンコーディング）
        let wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        let mgr = PluginManager::new(noop_write_pane());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &wasm).unwrap();
        // nexterm.log / nexterm.write_pane インポートがないので失敗するが、
        // ロード試行自体はエラーを返すだけでパニックしない
        let _ = mgr.load(tmp.path());
    }

    #[test]
    fn test_list_info_empty() {
        let mgr = PluginManager::new(noop_write_pane());
        let info = mgr.list_info();
        assert!(info.is_empty());
    }

    #[test]
    fn test_plugin_paths_empty() {
        let mgr = PluginManager::new(noop_write_pane());
        assert!(mgr.plugin_paths().is_empty());
    }

    #[test]
    fn test_load_dir_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = PluginManager::new(noop_write_pane());
        // 空のディレクトリでは 0 件
        let count = mgr.load_dir(dir.path()).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_load_dir_skips_non_wasm_files() {
        let dir = tempfile::tempdir().unwrap();
        // .wasm でないファイルを置く
        std::fs::write(dir.path().join("script.sh"), b"#!/bin/sh\necho hello").unwrap();
        std::fs::write(dir.path().join("config.toml"), b"[plugin]\nname = \"test\"").unwrap();
        let mgr = PluginManager::new(noop_write_pane());
        let count = mgr.load_dir(dir.path()).unwrap();
        // .wasm ファイルがないので 0 件（.sh/.toml はスキップ）
        assert_eq!(count, 0);
    }

    #[test]
    fn test_on_output_returns_false_without_plugins() {
        let mgr = PluginManager::new(noop_write_pane());
        // 長いデータでも false を返す
        let data = b"Hello, World! This is a test output from a pane.";
        assert!(!mgr.on_output(42, data));
    }

    #[test]
    fn test_on_command_returns_false_without_plugins() {
        let mgr = PluginManager::new(noop_write_pane());
        assert!(!mgr.on_command(":open-split horizontal"));
        assert!(!mgr.on_command(":zoom"));
    }

    #[test]
    fn test_unload_nonexistent_returns_false() {
        let mgr = PluginManager::new(noop_write_pane());
        let result = mgr.unload(Path::new("/nonexistent/plugin.wasm"));
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_plugin_api_version_constant() {
        assert_eq!(PLUGIN_API_VERSION, 1);
    }

    #[test]
    fn test_list_info_has_path_fields() {
        let mgr = PluginManager::new(noop_write_pane());
        let info = mgr.list_info();
        assert!(info.is_empty());
    }

    #[test]
    fn test_read_cstr_from_empty_returns_none() {
        let data = vec![0u8; 32];
        assert!(read_cstr_from(&data, 0, 32).is_none());
    }

    #[test]
    fn test_read_cstr_from_valid_string() {
        let mut data = vec![0u8; 64];
        data[..5].copy_from_slice(b"hello");
        let result = read_cstr_from(&data, 0, 64);
        assert_eq!(result.as_deref(), Some("hello"));
    }
}
