//! Lua フックランナー — 設定ファイルの Lua 関数をイベントフックとして呼び出す
//!
//! # 設計
//!
//! `LuaHookRunner` は `LuaWorker` と同様に専用スレッドで動作する。
//! フックイベントは `fire_hook()` で非同期に送信し、fire-and-forget で実行される。
//!
//! # Lua フックの書き方
//!
//! ```lua
//! -- ~/.config/nexterm/nexterm.lua
//! hooks.on_pane_open = function(session, pane_id)
//!   -- 例: 新しいペインの情報をログに記録する
//!   io.write("pane opened: " .. tostring(pane_id) .. " in " .. session .. "\n")
//! end
//!
//! hooks.on_session_start = function(session)
//!   io.write("session started: " .. session .. "\n")
//! end
//! ```

use std::path::PathBuf;
use std::sync::mpsc;

use mlua::prelude::*;
use tracing::{error, warn};

/// フックイベントの種別
#[derive(Debug)]
pub enum HookEvent {
    PaneOpen { session: String, pane_id: u32 },
    PaneClose { session: String, pane_id: u32 },
    SessionStart { session: String },
    Attach { session: String },
    Detach { session: String },
    /// マクロ実行リクエスト（応答チャネル付き）
    RunMacro {
        lua_fn: String,
        session: String,
        pane_id: u32,
        reply_tx: mpsc::SyncSender<Option<String>>,
    },
}

/// Lua フックランナー
///
/// 専用スレッドで Lua を実行し、フックイベントを処理する。
pub struct LuaHookRunner {
    /// イベント送信チャネル（None = Lua スクリプトが存在しない）
    event_tx: Option<mpsc::SyncSender<HookEvent>>,
}

impl LuaHookRunner {
    /// Lua スクリプトを読み込んでランナーを起動する
    ///
    /// スクリプトが存在しない場合は no-op のランナーを返す。
    pub fn new(lua_script_path: Option<PathBuf>) -> Self {
        let script_path = match lua_script_path {
            Some(p) if p.exists() => p,
            _ => return Self { event_tx: None },
        };

        let script = match std::fs::read_to_string(&script_path) {
            Ok(s) => s,
            Err(e) => {
                warn!("LuaHookRunner: スクリプト読み込みエラー: {}", e);
                return Self { event_tx: None };
            }
        };

        let (tx, rx) = mpsc::sync_channel::<HookEvent>(64);

        std::thread::Builder::new()
            .name("nexterm-lua-hooks".to_string())
            .spawn(move || {
                let lua = Lua::new();

                // `hooks` テーブルを事前に作成する（Lua でフック関数を登録できるようにする）
                if let Err(e) = lua.load("hooks = {}").exec() {
                    warn!("LuaHookRunner: hooks テーブルの初期化に失敗しました: {}", e);
                    return;
                }

                // ユーザースクリプトを実行する
                if let Err(e) = lua.load(&script).exec() {
                    warn!("LuaHookRunner: スクリプト実行エラー: {}", e);
                }

                // イベントループ
                while let Ok(event) = rx.recv() {
                    if let Err(e) = call_hook(&lua, &event) {
                        error!("LuaHookRunner: フック実行エラー ({:?}): {}", event, e);
                    }
                }
            })
            .expect("LuaHookRunner スレッドの起動に失敗");

        Self { event_tx: Some(tx) }
    }

    /// フックイベントを非同期で送信する（fire-and-forget）
    ///
    /// チャネルが満杯の場合はイベントを破棄してログを出力する。
    pub fn fire(&self, event: HookEvent) {
        if let Some(tx) = &self.event_tx {
            if tx.try_send(event).is_err() {
                warn!("LuaHookRunner: イベントキューが満杯です。フックをスキップします");
            }
        }
    }

    /// Lua フックランナーが有効か（スクリプトが存在するか）
    pub fn is_enabled(&self) -> bool {
        self.event_tx.is_some()
    }

    /// Lua マクロを同期実行して返り値（PTY 送信テキスト）を返す
    ///
    /// マクロは `function(session, pane_id) -> string` のシグネチャを持つ。
    /// タイムアウト（500ms）または Lua が無効な場合は `None` を返す。
    pub fn call_macro(&self, lua_fn: &str, session: &str, pane_id: u32) -> Option<String> {
        let tx = self.event_tx.as_ref()?;

        // 応答チャネル（容量1 = 呼び出し側は結果を1回受信する）
        let (reply_tx, reply_rx) = mpsc::sync_channel::<Option<String>>(1);

        tx.try_send(HookEvent::RunMacro {
            lua_fn: lua_fn.to_string(),
            session: session.to_string(),
            pane_id,
            reply_tx,
        })
        .ok()?;

        // 500ms 以内に結果を待つ
        reply_rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .ok()
            .flatten()
    }
}

/// Lua フック関数を呼び出す
///
/// `hooks.<event_name>` が関数として定義されている場合のみ呼び出す。
fn call_hook(lua: &Lua, event: &HookEvent) -> LuaResult<()> {
    let hooks: LuaTable = match lua.globals().get::<LuaTable>("hooks") {
        Ok(t) => t,
        Err(_) => return Ok(()), // hooks テーブルが存在しない
    };

    match event {
        HookEvent::PaneOpen { session, pane_id } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_pane_open") {
                func.call::<()>((session.as_str(), *pane_id))?;
            }
        }
        HookEvent::PaneClose { session, pane_id } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_pane_close") {
                func.call::<()>((session.as_str(), *pane_id))?;
            }
        }
        HookEvent::SessionStart { session } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_session_start") {
                func.call::<()>(session.as_str())?;
            }
        }
        HookEvent::Attach { session } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_attach") {
                func.call::<()>(session.as_str())?;
            }
        }
        HookEvent::Detach { session } => {
            if let Ok(func) = hooks.get::<LuaFunction>("on_detach") {
                func.call::<()>(session.as_str())?;
            }
        }
        HookEvent::RunMacro { lua_fn, session, pane_id, reply_tx } => {
            // グローバル関数名で探す（macros テーブル経由とグローバル直接呼び出し両対応）
            let result: Option<String> = lua
                .globals()
                .get::<LuaFunction>(lua_fn.as_str())
                .ok()
                .and_then(|func| {
                    func.call::<LuaValue>((session.as_str(), *pane_id)).ok()
                })
                .and_then(|val| match val {
                    LuaValue::String(s) => s.to_str().ok().map(|s| s.to_string()),
                    _ => None,
                });
            // 応答を送る（受信側がタイムアウトしていても無視）
            let _ = reply_tx.try_send(result);
        }
    }

    Ok(())
}
