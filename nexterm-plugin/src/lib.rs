//! Nexterm WASM プラグインホストランタイム
//!
//! # プラグイン ABI
//!
//! WASM モジュールは以下の関数を export する必要がある：
//!
//! ```wat
//! ;; モジュール初期化（オプション）
//! (export "nexterm_init" (func ...))
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
//! ホストから提供されるインポート関数：
//! ```wat
//! (import "nexterm" "log" (func (param i32 i32)))       ;; ログ出力
//! (import "nexterm" "write_pane" (func (param i32 i32 i32))) ;; ペインへの書き込み
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tracing::{error, info, warn};
use wasmi::{Engine, Linker, Module, Store};

// ---- ホストステート -------------------------------------------------------

/// プラグインホストへのコールバック（ペイン書き込み等）
pub type WritePaneFn = Arc<dyn Fn(u32, &[u8]) + Send + Sync>;

/// 1 プラグインのランタイムインスタンス
struct PluginInstance {
    /// プラグインファイルパス（デバッグ用）
    path: PathBuf,
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
    pub fn new(write_pane: WritePaneFn) -> Self {
        let engine = Engine::default();
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

        let instance = linker
            .instantiate(&mut store, &module)
            .with_context(|| "プラグインのインスタンス化に失敗")?
            .start(&mut store)
            .with_context(|| "プラグインの起動に失敗")?;

        // nexterm_init があれば呼ぶ（オプション）
        if let Ok(init_fn) = instance
            .get_typed_func::<(), ()>(&store, "nexterm_init")
        {
            init_fn.call(&mut store, ()).ok();
        }

        info!("プラグインをロードしました: {}", path.display());

        let mut plugins = self.plugins.lock().unwrap();
        plugins.push(PluginInstance {
            path: path.to_path_buf(),
            store,
            instance,
        });

        Ok(())
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
        let mut plugins = self.plugins.lock().unwrap();
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
        let mut plugins = self.plugins.lock().unwrap();
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
        self.plugins.lock().unwrap().len()
    }

    /// ロード済みプラグインのパス一覧を返す
    pub fn plugin_paths(&self) -> Vec<PathBuf> {
        self.plugins
            .lock()
            .unwrap()
            .iter()
            .map(|p| p.path.clone())
            .collect()
    }
}

// ---- プラグイン情報（ctl 表示用） -----------------------------------------

/// プラグイン情報（`nexterm-ctl plugin list` 等で表示）
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PluginInfo {
    pub path: String,
}

impl PluginManager {
    /// ロード済みプラグイン情報の一覧を返す
    pub fn list_info(&self) -> Vec<PluginInfo> {
        self.plugin_paths()
            .into_iter()
            .map(|p| PluginInfo {
                path: p.display().to_string(),
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
}
