//! セッションスナップショット型定義
//!
//! サーバー再起動をまたいでセッション状態を保存・復元するための
//! シリアライズ可能なデータ構造を定義する。
//!
//! # 保存対象
//!
//! - セッション名・シェル・端末サイズ
//! - ウィンドウ名・フォーカス状態
//! - BSP 分割ツリー（ペイン ID・分割方向・比率）
//! - 各ペインの作業ディレクトリ（Linux のみ `/proc/{pid}/cwd` より取得）
//!
//! # 復元の制約
//!
//! PTY プロセス自体は復元不可のため、復元時は保存されたシェルと
//! 作業ディレクトリで新規 PTY プロセスを起動する。
//! スクロールバック内容は保存されない（将来課題）。
//!
//! # スキーマバージョン履歴
//!
//! - v1: 初期バージョン（`shell_args` が後から追加、`#[serde(default)]` で互換）
//! - v2: `session_title` フィールドを追加。v1 スナップショットは自動マイグレーション可能
//! - v3: Sprint 5-7 / Phase 2-1 — `SessionSnapshot.workspace_name` を追加し、
//!   セッションをワークスペースにグルーピングする。v2 以前は `default` ワークスペース
//!   に所属するものとして自動マイグレーション可能

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// スナップショットのスキーマバージョン
///
/// フォーマットを変更した場合はインクリメントする。
/// 旧バージョンのスナップショットは `persist::load_snapshot` でマイグレーションを試みる。
pub const SNAPSHOT_VERSION: u32 = 3;

/// 旧バージョン（v1）との互換読み込みに使う最低サポートバージョン
///
/// v2.0.0 リリース時に `2` へ bump 予定。
/// 詳細は ADR-0007 (`docs/adr/0007-snapshot-v1-deprecation.md`) を参照。
pub const SNAPSHOT_VERSION_MIN: u32 = 1;

/// デフォルトワークスペース名。新規セッションや旧スナップショットの復元時に使用する。
pub const DEFAULT_WORKSPACE: &str = "default";

fn default_workspace() -> String {
    DEFAULT_WORKSPACE.to_string()
}

/// サーバー全体のスナップショット（保存の最上位単位）
#[derive(Debug, Serialize, Deserialize)]
pub struct ServerSnapshot {
    /// スキーマバージョン
    pub version: u32,
    /// 保存時点の全セッション
    pub sessions: Vec<SessionSnapshot>,
    /// 保存時の Unix タイムスタンプ（秒）
    pub saved_at: u64,
    /// 保存時にアクティブだったワークスペース名（v3 追加。省略時は `default`）
    #[serde(default = "default_workspace")]
    pub current_workspace: String,
}

/// セッションのスナップショット
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// セッション名
    pub name: String,
    /// 起動シェルコマンド
    pub shell: String,
    /// シェル起動引数（例: ["-NoLogo"] for PowerShell）
    #[serde(default)]
    pub shell_args: Vec<String>,
    /// 端末列数
    pub cols: u16,
    /// 端末行数
    pub rows: u16,
    /// ウィンドウ一覧
    pub windows: Vec<WindowSnapshot>,
    /// フォーカスしているウィンドウ ID
    pub focused_window_id: u32,
    /// セッションの表示タイトル（v2 追加。省略時はセッション名を使用）
    #[serde(default)]
    pub session_title: Option<String>,
    /// 所属ワークスペース名（v3 追加。省略時は `default`）
    #[serde(default = "default_workspace")]
    pub workspace_name: String,
}

/// ウィンドウのスナップショット
#[derive(Debug, Serialize, Deserialize)]
pub struct WindowSnapshot {
    /// ウィンドウ ID
    pub id: u32,
    /// ウィンドウ名
    pub name: String,
    /// フォーカスしているペイン ID
    pub focused_pane_id: u32,
    /// BSP 分割ツリー
    pub layout: SplitNodeSnapshot,
}

/// BSP 分割ツリーのスナップショット
#[derive(Debug, Serialize, Deserialize)]
pub enum SplitNodeSnapshot {
    /// 単一ペイン
    Pane {
        /// ペイン ID
        pane_id: u32,
        /// 作業ディレクトリ（Linux のみ取得可能）
        cwd: Option<PathBuf>,
    },
    /// 分割ノード
    Split {
        /// 分割方向
        dir: SplitDirSnapshot,
        /// 左/上の占有割合（0.0〜1.0）
        ratio: f32,
        /// 左/上の子ノード
        left: Box<SplitNodeSnapshot>,
        /// 右/下の子ノード
        right: Box<SplitNodeSnapshot>,
    },
}

/// 分割方向のスナップショット
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitDirSnapshot {
    /// 垂直分割（左右）
    Vertical,
    /// 水平分割（上下）
    Horizontal,
}
