//! Lua 評価ワーカー — 専用 OS スレッドで Lua を実行する
//!
//! # 背景
//!
//! `mlua::Lua` は `!Send + !Sync` のため tokio スレッドプールに渡せない。
//! winit イベントループ（メインスレッド）で同期的に Lua を評価すると、
//! 複雑なスクリプトが UI をブロックするリスクがある。
//!
//! # 設計
//!
//! - `std::thread::spawn` で専用の Lua ワーカースレッドを起動する
//! - ワーカーは `sync_channel(1)` でリクエストを受信して評価し、結果をキャッシュに書き込む
//! - `eval_widgets()` はキャッシュを即座に返す（ブロックしない）
//! - チャネルが満杯の場合（ワーカーが評価中）はリクエストを破棄し、前回キャッシュを返す

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mlua::prelude::*;
use tracing::warn;

/// ワーカースレッドへの評価リクエスト
struct LuaRequest {
    widgets: Vec<String>,
}

/// バックグラウンドスレッドで Lua を評価するワーカー
///
/// `Lua` インスタンスは専用スレッド上にのみ存在する（`!Send` の制約を満たす）。
/// メインスレッドは `eval_widgets()` でキャッシュを即座に取得できる。
pub struct LuaWorker {
    /// 最新評価結果のキャッシュ（ワーカースレッドが更新する）
    cache: Arc<Mutex<String>>,
    /// 評価リクエスト送信チャネル（容量 1: 満杯時は try_send が Err を返す）
    request_tx: std::sync::mpsc::SyncSender<LuaRequest>,
}

impl LuaWorker {
    /// ワーカースレッドを起動する
    ///
    /// `lua_script_path` が `Some` の場合、そのスクリプトをワーカー起動時に実行する。
    pub fn new(lua_script_path: Option<PathBuf>) -> Self {
        let cache = Arc::new(Mutex::new(String::new()));
        let cache_clone = Arc::clone(&cache);

        // 容量 1 のチャネル: ワーカーが評価中のとき try_send は即座に Err を返す
        let (tx, rx) = std::sync::mpsc::sync_channel::<LuaRequest>(1);

        std::thread::Builder::new()
            .name("nexterm-lua-worker".to_string())
            .spawn(move || {
                // Lua インスタンスをこのスレッド内で生成する（!Send のため move 不可）
                let lua = Lua::new();

                if let Some(path) = lua_script_path {
                    if path.exists() {
                        match std::fs::read_to_string(&path) {
                            Ok(script) => {
                                if let Err(e) = lua.load(&script).exec() {
                                    warn!("Lua ワーカー: スクリプト読み込みエラー: {}", e);
                                }
                            }
                            Err(e) => warn!("Lua ワーカー: ファイル読み込みエラー: {}", e),
                        }
                    }
                }

                // リクエストを順番に処理する（チャネルが閉じられたら終了）
                while let Ok(req) = rx.recv() {
                    let parts: Vec<String> = req
                        .widgets
                        .iter()
                        .map(|expr| {
                            lua.load(expr.as_str())
                                .eval::<String>()
                                .unwrap_or_default()
                        })
                        .collect();
                    let result = parts.join("  ");

                    if let Ok(mut guard) = cache_clone.lock() {
                        *guard = result;
                    }
                }
            })
            .expect("Lua ワーカースレッドの起動に失敗");

        Self {
            cache,
            request_tx: tx,
        }
    }

    /// ウィジェット式の評価をリクエストし、キャッシュ済み結果を返す
    ///
    /// - バックグラウンドスレッドへリクエストを送信してから即座に返す
    /// - ワーカーが評価中の場合、リクエストは破棄されて前回の結果を返す
    /// - 初回呼び出しは空文字列を返す（次フレームから結果が表示される）
    pub fn eval_widgets(&self, widgets: &[String]) -> String {
        // try_send: チャネル満杯なら破棄（ブロックしない）
        let _ = self.request_tx.try_send(LuaRequest {
            widgets: widgets.to_vec(),
        });
        self.cache.lock().map(|g| g.clone()).unwrap_or_default()
    }
}
