#![warn(missing_docs)]
//! Nexterm WASM プラグインホストランタイム
//!
//! # プラグイン ABI バージョン
//!
//! [`PLUGIN_API_VERSION`] が安定 ABI を識別する。プラグインは
//! `nexterm.api_version() -> i32` インポートで照合できる。
//!
//! ## v2（現行・推奨）
//!
//! - 入力データ（ペイン出力 / コマンド文字列）は **サニタイズ済み**で渡される。
//!   ESC・C0 制御文字・OSC/CSI/DCS/APC シーケンスは除去される。
//!   タブ・改行・印字可能 ASCII / UTF-8 マルチバイトのみ通過する。
//! - `nexterm.write_pane(pane_id, ...)` は呼び出しスコープごとに**許可された
//!   pane_id 集合**でフィルタされる。`nexterm_on_output(pane_id, ...)` 中は
//!   その `pane_id` のみ書き込み可、`nexterm_on_command(...)` 中はどの
//!   pane にも書き込めない。
//!
//! ## v1（後方互換のため継続サポート）
//!
//! `nexterm_api_version` エクスポートを公開していない、または `1` を返す
//! プラグインは v1 として扱われる:
//!
//! - 入力データはサニタイズされず生バイト列が渡される（旧挙動）
//! - `write_pane` は任意の pane_id に書き込み可（旧挙動）
//! - ロード時に **deprecation 警告**を 1 回だけログに出す
//!
//! v1 は将来的に削除予定（具体的な削除タイミングは未定）。新規プラグインは
//! v2 で実装すること。
//!
//! # WASM エクスポート（プラグイン側が実装する）
//!
//! ```wat
//! ;; モジュール初期化（オプション）
//! (export "nexterm_init" (func ...))
//!
//! ;; API バージョン宣言（v2 では必須）
//! (export "nexterm_api_version" (func (result i32)))
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

/// プラグイン ABI の現行バージョン番号（最新仕様）。
/// プラグインが `nexterm_api_version` エクスポートで宣言する値はこの値と
/// 一致するか、`MIN_SUPPORTED_API_VERSION` 以上であれば受け入れる。
pub const PLUGIN_API_VERSION: u32 = 2;

/// 後方互換でロードを許可する最小 API バージョン。
///
/// v1 プラグインはサニタイズなし・pane_id 検証なしで動作する旧挙動を維持する。
/// ロード時に deprecation 警告がログに出る。
pub const MIN_SUPPORTED_API_VERSION: u32 = 1;

use std::collections::HashSet;
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

// ---- 入力サニタイズ -------------------------------------------------------

/// v2 プラグインに渡す前に入力バイト列から制御文字・エスケープシーケンスを除去する。
///
/// 通過するバイト:
/// - `0x09`（TAB）、`0x0A`（LF）、`0x0D`（CR）
/// - `0x20..=0x7E`（印字可能 ASCII）
/// - `0x80..=0xFF`（UTF-8 マルチバイト先頭・継続バイト）
///
/// 除去される:
/// - その他の C0 制御文字（`0x00..=0x08`, `0x0B..=0x0C`, `0x0E..=0x1F`）
/// - `0x7F`（DEL）
/// - ESC (`0x1B`) で始まる CSI / OSC / DCS / APC シーケンス全体
///   （ESC と続くシーケンス終端まで一括破棄）
///
/// ## v2 でサニタイズする理由
///
/// v1 ではプラグインがクリップボード書き換え（OSC 52）やハイパーリンク
/// （OSC 8）等の機密シーケンスを直接観測できた。v2 ではホスト側で先に
/// 除去することで、プラグインに渡る情報をプレーンテキストに限定する。
pub fn sanitize_for_plugin(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        match b {
            0x1B => {
                // ESC: 続くシーケンスをスキップ
                i += 1;
                if i >= input.len() {
                    break;
                }
                match input[i] {
                    b'[' => {
                        // CSI: パラメータ + 中間バイト + 終端バイト (0x40..=0x7E)
                        i += 1;
                        while i < input.len() && !(0x40..=0x7E).contains(&input[i]) {
                            i += 1;
                        }
                        if i < input.len() {
                            i += 1; // 終端バイトを消費
                        }
                    }
                    b']' | b'P' | b'_' | b'^' => {
                        // OSC / DCS / APC / PM: ST (ESC \) または BEL (0x07) で終了
                        i += 1;
                        while i < input.len() {
                            if input[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if input[i] == 0x1B && i + 1 < input.len() && input[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        // 2 バイトエスケープ（ESC c, ESC =, ESC > 等）: 1 バイト消費
                        i += 1;
                    }
                }
            }
            0x09 | 0x0A | 0x0D => {
                // TAB / LF / CR は通過
                out.push(b);
                i += 1;
            }
            0x00..=0x1F | 0x7F => {
                // その他の C0 制御 + DEL は除去
                i += 1;
            }
            _ => {
                // 印字可能 ASCII / UTF-8 マルチバイト
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

// ---- ホストステート -------------------------------------------------------

/// プラグインホストへのコールバック（ペイン書き込み等）
pub type WritePaneFn = Arc<dyn Fn(u32, &[u8]) + Send + Sync>;

/// 1 プラグインのランタイムインスタンス
struct PluginInstance {
    /// プラグインファイルパス（デバッグ用）
    path: PathBuf,
    /// プラグインが宣言した API バージョン（`nexterm_api_version` 取得値、
    /// 取得不可の場合は `1`）
    api_version: u32,
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
    /// 現在実行中のフック呼び出しで `write_pane` が許可されている pane_id 集合。
    /// v2 プラグインのみ参照。空集合 = どのペインにも書き込めない。
    allowed_panes: HashSet<u32>,
    /// プラグインの API バージョン（v1 では allowed_panes を無視するために必要）
    api_version: u32,
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
    /// v2 プラグインの場合、許可された pane_id でしか呼び出されない。
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
                allowed_panes: HashSet::new(),
                // 暫定値。`nexterm_api_version` 取得後に確定する。
                api_version: MIN_SUPPORTED_API_VERSION,
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
        //
        // v2 プラグイン: pane_id が `allowed_panes` に含まれない場合は
        //   呼び出しを無視し、warn ログを出す（拒否を明示するため）。
        // v1 プラグイン: 旧挙動どおり常に許可する。
        linker.func_wrap(
            "nexterm",
            "write_pane",
            |caller: wasmi::Caller<'_, HostState>, pane_id: i32, ptr: i32, len: i32| {
                let pane_u = pane_id as u32;
                let allowed = {
                    let state = caller.data();
                    state.api_version < 2 || state.allowed_panes.contains(&pane_u)
                };
                if !allowed {
                    warn!(
                        "[plugin] write_pane 拒否: pane_id={} は許可リスト外（API v2）",
                        pane_u
                    );
                    return;
                }
                if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                    let data = mem.data(&caller);
                    let start = ptr as usize;
                    let end = start.saturating_add(len as usize);
                    if end <= data.len() {
                        let bytes = data[start..end].to_vec();
                        (caller.data().write_pane)(pane_u, &bytes);
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

        // API バージョン検出 + 互換性チェック
        // - エクスポートあり → 値を採用、`PLUGIN_API_VERSION` を上回ったら拒否
        // - エクスポートあり、呼び出し失敗 → ロード継続（v1 扱い）
        // - エクスポートなし → v1 扱い
        let mut api_version = MIN_SUPPORTED_API_VERSION;
        if let Ok(version_fn) = instance.get_typed_func::<(), i32>(&store, "nexterm_api_version") {
            store
                .set_fuel(FUEL_PER_CALL)
                .with_context(|| "fuel 設定に失敗")?;
            match version_fn.call(&mut store, ()) {
                Ok(v) => {
                    let v_u = v as u32;
                    if v_u > PLUGIN_API_VERSION {
                        anyhow::bail!(
                            "プラグイン API バージョンがホストより新しい: plugin={}, host={}",
                            v_u,
                            PLUGIN_API_VERSION
                        );
                    }
                    if v_u < MIN_SUPPORTED_API_VERSION {
                        anyhow::bail!(
                            "プラグイン API バージョンが古すぎます: plugin={}, min={}",
                            v_u,
                            MIN_SUPPORTED_API_VERSION
                        );
                    }
                    api_version = v_u;
                }
                Err(e) => {
                    warn!(
                        "プラグイン API バージョン取得失敗（v1 として続行）: {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        // HostState の api_version を最終確定値で書き換える
        store.data_mut().api_version = api_version;

        // v1 プラグインには deprecation 警告を 1 回だけ出す
        if api_version < PLUGIN_API_VERSION {
            warn!(
                "プラグインが API v{} で動作中（現行 v{}）: {} — \
                 サニタイズ・PaneId 検証なしの旧挙動で動作します。\
                 将来のバージョンで v1 サポートは削除予定です。",
                api_version,
                PLUGIN_API_VERSION,
                path.display()
            );
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
            "プラグインをロードしました: {} (api=v{} name={:?} version={:?})",
            path.display(),
            api_version,
            meta_name,
            meta_version
        );

        let mut plugins = self.plugins.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("plugins mutex がポイズン状態。回復して継続します");
            poisoned.into_inner()
        });
        plugins.push(PluginInstance {
            path: path.to_path_buf(),
            api_version,
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
    ///
    /// v2 プラグイン: `data` をサニタイズしてから渡し、`pane_id` のみ
    ///   `write_pane` 経由で書き込み可能にする。
    /// v1 プラグイン: 生バイト列を渡し、書き込み制限なし（旧挙動）。
    pub fn on_output(&self, pane_id: u32, data: &[u8]) -> bool {
        // v2 用にサニタイズ済みデータを 1 度だけ計算（複数プラグインで再利用）
        let sanitized = sanitize_for_plugin(data);
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
            let payload: &[u8] = if plugin.api_version >= 2 {
                &sanitized
            } else {
                data
            };
            // WASM 線形メモリにデータを書き込む
            if let Some(mem) = plugin.instance.get_memory(&plugin.store, "memory") {
                let offset = 64 * 1024usize; // スタック上部から安全な領域
                let mem_size = mem.data_size(&plugin.store);
                if offset + payload.len() <= mem_size {
                    mem.write(&mut plugin.store, offset, payload).ok();
                    // 各呼び出し前に fuel を補充（無限ループ防止、CRITICAL #10）
                    if let Err(e) = plugin.store.set_fuel(FUEL_PER_CALL) {
                        error!("[plugin {}] fuel 設定失敗: {}", plugin.path.display(), e);
                        continue;
                    }
                    // v2: 許可 pane を {pane_id} に設定（呼び出しスコープのみ有効）
                    {
                        let state = plugin.store.data_mut();
                        state.allowed_panes.clear();
                        if state.api_version >= 2 {
                            state.allowed_panes.insert(pane_id);
                        }
                    }
                    let result = func.call(
                        &mut plugin.store,
                        (pane_id as i32, offset as i32, payload.len() as i32),
                    );
                    // 後片付け: 許可リストを必ず空に戻す
                    plugin.store.data_mut().allowed_panes.clear();
                    match result {
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
    ///
    /// v2 プラグイン: 文字列をサニタイズしてから渡し、`write_pane` は
    ///   どのペインにも書き込めない（許可リスト空）。
    /// v1 プラグイン: 生文字列を渡し、書き込み制限なし（旧挙動）。
    pub fn on_command(&self, cmd: &str) -> bool {
        let cmd_bytes = cmd.as_bytes();
        let sanitized = sanitize_for_plugin(cmd_bytes);
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
            let payload: &[u8] = if plugin.api_version >= 2 {
                &sanitized
            } else {
                cmd_bytes
            };
            if let Some(mem) = plugin.instance.get_memory(&plugin.store, "memory") {
                let offset = 64 * 1024usize;
                let mem_size = mem.data_size(&plugin.store);
                if offset + payload.len() <= mem_size {
                    mem.write(&mut plugin.store, offset, payload).ok();
                    // 各呼び出し前に fuel を補充（無限ループ防止、CRITICAL #10）
                    if let Err(e) = plugin.store.set_fuel(FUEL_PER_CALL) {
                        error!("[plugin {}] fuel 設定失敗: {}", plugin.path.display(), e);
                        continue;
                    }
                    // v2: コマンドフックではどの pane にも書き込ませない
                    plugin.store.data_mut().allowed_panes.clear();
                    let result =
                        func.call(&mut plugin.store, (offset as i32, payload.len() as i32));
                    plugin.store.data_mut().allowed_panes.clear();
                    match result {
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
    /// プラグインが宣言した API バージョン
    pub api_version: u32,
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
                api_version: p.api_version,
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
        assert_eq!(PLUGIN_API_VERSION, 2);
        assert_eq!(MIN_SUPPORTED_API_VERSION, 1);
        // const assert で互換性条件を保証する
        const _: () = assert!(MIN_SUPPORTED_API_VERSION <= PLUGIN_API_VERSION);
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

    // ---- サニタイズのテスト（Sprint 4-2） ----

    #[test]
    fn sanitize_passes_printable_ascii() {
        let input = b"Hello, World! 0123";
        assert_eq!(sanitize_for_plugin(input), input);
    }

    #[test]
    fn sanitize_passes_utf8_multibyte() {
        let input = "こんにちは世界".as_bytes();
        assert_eq!(sanitize_for_plugin(input), input);
    }

    #[test]
    fn sanitize_keeps_tab_lf_cr() {
        let input = b"a\tb\nc\rd";
        assert_eq!(sanitize_for_plugin(input), input);
    }

    #[test]
    fn sanitize_strips_other_c0_controls() {
        let input = b"a\x00b\x07c\x08d\x0Be\x7Ff";
        // NUL, BEL, BS, VT, DEL は除去
        assert_eq!(sanitize_for_plugin(input), b"abcdef");
    }

    #[test]
    fn sanitize_strips_csi_sequence() {
        // ESC [ 31 m  → 赤色 SGR
        let input = b"red:\x1b[31mfoo\x1b[0mend";
        assert_eq!(sanitize_for_plugin(input), b"red:fooend");
    }

    #[test]
    fn sanitize_strips_osc_with_bel_terminator() {
        // OSC 0; title BEL
        let input = b"x\x1b]0;mytitle\x07y";
        assert_eq!(sanitize_for_plugin(input), b"xy");
    }

    #[test]
    fn sanitize_strips_osc_with_st_terminator() {
        // OSC 52 ; c ; <base64> ESC \
        let input = b"a\x1b]52;c;SGVsbG8=\x1b\\b";
        assert_eq!(sanitize_for_plugin(input), b"ab");
    }

    #[test]
    fn sanitize_strips_dcs_and_apc() {
        // DCS / APC それぞれ ESC \ で終わる
        let input = b"a\x1bP123\x1b\\b\x1b_apc\x1b\\c";
        assert_eq!(sanitize_for_plugin(input), b"abc");
    }

    #[test]
    fn sanitize_strips_two_byte_escape() {
        // ESC = (DECKPAM)
        let input = b"x\x1b=y";
        assert_eq!(sanitize_for_plugin(input), b"xy");
    }

    #[test]
    fn sanitize_handles_truncated_csi() {
        // ESC [ で終わる場合（パラメータ未終了）はそれ以降を破棄
        let input = b"safe\x1b[31";
        let out = sanitize_for_plugin(input);
        assert_eq!(out, b"safe");
    }

    #[test]
    fn sanitize_handles_lone_esc_at_end() {
        let input = b"safe\x1b";
        let out = sanitize_for_plugin(input);
        assert_eq!(out, b"safe");
    }

    #[test]
    fn sanitize_does_not_panic_on_arbitrary_bytes() {
        // 全 256 バイトを含む入力でパニックしないこと
        let input: Vec<u8> = (0..=255u8).collect();
        let _ = sanitize_for_plugin(&input);
    }
}
