//! SSH ホスト設定とイベントフック

use serde::{Deserialize, Serialize};

/// SSH ホスト設定
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct HostConfig {
    /// 表示名
    pub name: String,
    /// ホスト名または IP アドレス
    pub host: String,
    /// SSH ポート（デフォルト: 22）
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// ユーザー名
    pub username: String,
    /// 認証方式: "password", "key", "agent"
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    /// 秘密鍵ファイルパス（auth_type = "key" の場合）
    pub key_path: Option<String>,
    /// ローカルポートフォワーディング設定（例: "8080:localhost:80"）
    #[serde(default)]
    pub forward_local: Vec<String>,
    /// リモートポートフォワーディング設定（例: "9090:localhost:9090"）
    #[serde(default)]
    pub forward_remote: Vec<String>,
    /// ProxyJump ホスト名（hosts に登録されたエントリ名）
    pub proxy_jump: Option<String>,
    /// X11 フォワーディングを有効にするか（ssh -X 相当）
    #[serde(default)]
    pub x11_forward: bool,
    /// 信頼された X11 フォワーディング（ssh -Y 相当）
    #[serde(default)]
    pub x11_trusted: bool,
    /// グループ名（ホストをカテゴリ分けするための任意文字列）
    #[serde(default)]
    pub group: String,
    /// タグ一覧（複数のラベルで絞り込みに使用する）
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_auth_type() -> String {
    "key".to_string()
}

/// ターミナルフック設定 — イベント発生時に実行するシェルコマンドまたは Lua 関数
///
/// シェルコマンドフック: 文字列で指定（`sh -c` で実行）
///   `$NEXTERM_PANE_ID` / `$NEXTERM_SESSION` 環境変数が利用可能
///
/// Lua 関数フック: `lua_on_*` フィールドに Lua 関数名を指定
///   設定ファイル内で `function on_pane_open(session, pane_id) ... end` のように定義する
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// 新しいペインが開かれたときに実行するシェルコマンド
    pub on_pane_open: Option<String>,
    /// ペインが閉じられたときに実行するシェルコマンド
    pub on_pane_close: Option<String>,
    /// 新しいセッションが開始されたときに実行するシェルコマンド
    pub on_session_start: Option<String>,
    /// セッションにクライアントがアタッチしたときに実行するシェルコマンド
    pub on_attach: Option<String>,
    /// クライアントがセッションからデタッチしたときに実行するシェルコマンド
    pub on_detach: Option<String>,
    /// ペインが開かれたときに呼び出す Lua 関数名（例: "on_pane_open"）
    pub lua_on_pane_open: Option<String>,
    /// ペインが閉じられたときに呼び出す Lua 関数名
    pub lua_on_pane_close: Option<String>,
    /// セッション開始時に呼び出す Lua 関数名
    pub lua_on_session_start: Option<String>,
    /// アタッチ時に呼び出す Lua 関数名
    pub lua_on_attach: Option<String>,
    /// デタッチ時に呼び出す Lua 関数名
    pub lua_on_detach: Option<String>,
}
