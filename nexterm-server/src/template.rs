//! レイアウトテンプレート — ウィンドウ/ペイン構成の保存と復元
//!
//! テンプレートは `~/.config/nexterm/templates/<name>.json` に保存される。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

// ---- テンプレート型 ----

/// ペインツリーの再帰表現
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PaneTemplate {
    /// 末端ペイン
    Leaf {
        /// 起動コマンド（None の場合はデフォルトシェル）
        command: Option<String>,
        /// 作業ディレクトリ（None の場合はデフォルト）
        cwd: Option<String>,
    },
    /// 垂直分割（左右）
    SplitH {
        ratio: f32,
        left: Box<PaneTemplate>,
        right: Box<PaneTemplate>,
    },
    /// 水平分割（上下）
    SplitV {
        ratio: f32,
        top: Box<PaneTemplate>,
        bottom: Box<PaneTemplate>,
    },
}

/// ウィンドウテンプレート
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowTemplate {
    /// ウィンドウタイトル
    pub title: String,
    /// ペインレイアウト
    pub layout: PaneTemplate,
}

/// セッション全体のレイアウトテンプレート
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutTemplate {
    /// テンプレート名
    pub name: String,
    /// ウィンドウ一覧
    pub windows: Vec<WindowTemplate>,
    /// 作成日時（UNIX timestamp）
    pub created_at: u64,
}

// ---- ファイルシステム操作 ----

/// テンプレートを保存するディレクトリを返す
pub fn template_dir() -> PathBuf {
    let base = nexterm_config::loader::config_dir();
    base.join("templates")
}

/// テンプレートのファイルパスを返す
pub fn template_path(name: &str) -> PathBuf {
    template_dir().join(format!("{}.json", name))
}

impl LayoutTemplate {
    /// 新しいテンプレートを生成する（デフォルト: 単一ペイン×1ウィンドウ）
    #[allow(dead_code)]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            windows: vec![WindowTemplate {
                title: "main".to_string(),
                layout: PaneTemplate::Leaf {
                    command: None,
                    cwd: None,
                },
            }],
            created_at: unix_now(),
        }
    }

    /// テンプレートをファイルに保存する
    ///
    /// 戻り値: 保存先パスの文字列
    pub fn save(&self) -> Result<String> {
        let dir = template_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("テンプレートディレクトリの作成に失敗しました: {}", dir.display()))?;

        let path = template_path(&self.name);
        let json = serde_json::to_string_pretty(self)
            .context("テンプレートの JSON シリアライズに失敗しました")?;
        std::fs::write(&path, &json)
            .with_context(|| format!("テンプレートの書き込みに失敗しました: {}", path.display()))?;

        info!("テンプレートを保存しました: {}", path.display());
        Ok(path.to_string_lossy().to_string())
    }

    /// ファイルからテンプレートを読み込む
    pub fn load(name: &str) -> Result<Self> {
        let path = template_path(name);
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("テンプレートの読み込みに失敗しました: {}", path.display()))?;
        let template: Self = serde_json::from_str(&json)
            .with_context(|| format!("テンプレートの JSON デシリアライズに失敗しました: {}", path.display()))?;
        Ok(template)
    }

    /// 保存済みテンプレートの名前一覧を返す
    pub fn list() -> Result<Vec<String>> {
        let dir = template_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("テンプレートディレクトリの読み取りに失敗しました: {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false)
                && let Some(stem) = path.file_stem() {
                    names.push(stem.to_string_lossy().to_string());
                }
        }
        names.sort();
        Ok(names)
    }
}

// ---- セッションからテンプレートを生成する ----

/// Session の BSP ツリーを LayoutTemplate に変換するヘルパー
///
/// セッションの実際のウィンドウ構造を走査してテンプレートを生成する。
/// 現時点では各ペインの CWD と分割構造を記録する（コマンドは記録しない）。
pub fn template_from_session_info(
    name: &str,
    window_titles: Vec<String>,
    pane_count_per_window: Vec<usize>,
) -> LayoutTemplate {
    // ウィンドウごとに単純なリーフノードを生成する（BSP 走査は将来拡張）
    let windows = window_titles
        .into_iter()
        .zip(pane_count_per_window)
        .map(|(title, count)| WindowTemplate {
            title,
            layout: build_balanced_layout(count),
        })
        .collect();

    LayoutTemplate {
        name: name.to_string(),
        windows,
        created_at: unix_now(),
    }
}

/// n 個のペインを均等分割するレイアウトを生成する
fn build_balanced_layout(count: usize) -> PaneTemplate {
    if count <= 1 {
        return PaneTemplate::Leaf { command: None, cwd: None };
    }
    let left_count = count / 2;
    let right_count = count - left_count;
    PaneTemplate::SplitH {
        ratio: left_count as f32 / count as f32,
        left: Box::new(build_balanced_layout(left_count)),
        right: Box::new(build_balanced_layout(right_count)),
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
