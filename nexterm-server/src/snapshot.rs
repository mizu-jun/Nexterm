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

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// スナップショットのスキーマバージョン
///
/// フォーマットを変更した場合はインクリメントする。
/// バージョン不一致のスナップショットは復元をスキップする。
pub const SNAPSHOT_VERSION: u32 = 1;

/// サーバー全体のスナップショット（保存の最上位単位）
#[derive(Debug, Serialize, Deserialize)]
pub struct ServerSnapshot {
    /// スキーマバージョン
    pub version: u32,
    /// 保存時点の全セッション
    pub sessions: Vec<SessionSnapshot>,
    /// 保存時の Unix タイムスタンプ（秒）
    pub saved_at: u64,
}

/// セッションのスナップショット
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub name: String,
    pub shell: String,
    pub cols: u16,
    pub rows: u16,
    pub windows: Vec<WindowSnapshot>,
    pub focused_window_id: u32,
}

/// ウィンドウのスナップショット
#[derive(Debug, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub id: u32,
    pub name: String,
    pub focused_pane_id: u32,
    /// BSP 分割ツリー
    pub layout: SplitNodeSnapshot,
}

/// BSP 分割ツリーのスナップショット
#[derive(Debug, Serialize, Deserialize)]
pub enum SplitNodeSnapshot {
    Pane {
        pane_id: u32,
        /// 作業ディレクトリ（Linux のみ取得可能）
        cwd: Option<PathBuf>,
    },
    Split {
        dir: SplitDirSnapshot,
        /// 左/上の占有割合（0.0〜1.0）
        ratio: f32,
        left: Box<SplitNodeSnapshot>,
        right: Box<SplitNodeSnapshot>,
    },
}

/// 分割方向のスナップショット
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitDirSnapshot {
    Vertical,
    Horizontal,
}
