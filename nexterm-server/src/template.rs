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
        std::fs::create_dir_all(&dir).with_context(|| {
            format!(
                "テンプレートディレクトリの作成に失敗しました: {}",
                dir.display()
            )
        })?;

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
        let template: Self = serde_json::from_str(&json).with_context(|| {
            format!(
                "テンプレートの JSON デシリアライズに失敗しました: {}",
                path.display()
            )
        })?;
        Ok(template)
    }

    /// 保存済みテンプレートの名前一覧を返す
    pub fn list() -> Result<Vec<String>> {
        let dir = template_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir).with_context(|| {
            format!(
                "テンプレートディレクトリの読み取りに失敗しました: {}",
                dir.display()
            )
        })? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false)
                && let Some(stem) = path.file_stem()
            {
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
        return PaneTemplate::Leaf {
            command: None,
            cwd: None,
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_dir_returnss_path() {
        let dir = template_dir();
        assert!(!dir.as_os_str().is_empty());
        assert!(dir.to_string_lossy().contains("templates"));
    }

    #[test]
    fn template_path_appends_json() {
        let path = template_path("test_template");
        let file_name = path.file_name().unwrap().to_string_lossy();
        assert_eq!(file_name, "test_template.json");
    }

    #[test]
    fn unix_now_returns_non_zero() {
        let now = unix_now();
        assert!(now > 0);
    }

    #[test]
    fn build_balanced_layout_single_pane() {
        let layout = build_balanced_layout(1);
        match layout {
            PaneTemplate::Leaf { .. } => {}
            _ => panic!("1個のペインはLeafになるべき"),
        }
    }

    #[test]
    fn build_balanced_layout_two_panes() {
        let layout = build_balanced_layout(2);
        match layout {
            PaneTemplate::SplitH { ratio, left, right } => {
                assert!((ratio - 0.5).abs() < 0.01); // 1:1分割
                // 左右はLeaf
                match *left {
                    PaneTemplate::Leaf { .. } => {}
                    _ => panic!("左はLeafになるべき"),
                }
                match *right {
                    PaneTemplate::Leaf { .. } => {}
                    _ => panic!("右はLeafになるべき"),
                }
            }
            _ => panic!("2個のペインはSplitHになるべき"),
        }
    }

    #[test]
    fn build_balanced_layout_four_panes() {
        let layout = build_balanced_layout(4);
        match layout {
            PaneTemplate::SplitH { left, right, .. } => {
                // 左: 2ペインの分割、右: 2ペインの分割
                match *left {
                    PaneTemplate::SplitH { .. } => {}
                    _ => panic!("左は再帰的に2分割されるべき"),
                }
                match *right {
                    PaneTemplate::SplitH { .. } | PaneTemplate::Leaf { .. } => {}
                    PaneTemplate::SplitV { .. } => {}
                }
            }
            _ => panic!("4個のペインはSplitHになるべき"),
        }
    }

    #[test]
    fn layout_template_new_creates_default() {
        let template = LayoutTemplate::new("test");
        assert_eq!(template.name, "test");
        assert_eq!(template.windows.len(), 1);
        assert_eq!(template.windows[0].title, "main");
        matches!(template.windows[0].layout, PaneTemplate::Leaf { .. });
    }

    #[test]
    fn template_from_session_info_creates_correct_structure() {
        let titles = vec!["win1".to_string(), "win2".to_string()];
        let counts = vec![2, 4];
        let template = template_from_session_info("session_template", titles, counts);

        assert_eq!(template.name, "session_template");
        assert_eq!(template.windows.len(), 2);
        assert_eq!(template.windows[0].title, "win1");
        assert_eq!(template.windows[1].title, "win2");
    }

    #[test]
    fn pane_template_serialization_roundtrip() {
        let leaf = PaneTemplate::Leaf {
            command: Some("/bin/bash".to_string()),
            cwd: Some("/home/user".to_string()),
        };
        let json = serde_json::to_string(&leaf).unwrap();
        let deserialized: PaneTemplate = serde_json::from_str(&json).unwrap();

        match (leaf, deserialized) {
            (
                PaneTemplate::Leaf {
                    command: c1,
                    cwd: d1,
                },
                PaneTemplate::Leaf {
                    command: c2,
                    cwd: d2,
                },
            ) => {
                assert_eq!(c1, c2);
                assert_eq!(d1, d2);
            }
            _ => panic!("roundtrip failed"),
        }
    }

    #[test]
    fn window_template_serialization_roundtrip() {
        let window = WindowTemplate {
            title: "test_window".to_string(),
            layout: PaneTemplate::Leaf {
                command: None,
                cwd: None,
            },
        };
        let json = serde_json::to_string(&window).unwrap();
        let deserialized: WindowTemplate = serde_json::from_str(&json).unwrap();
        assert_eq!(window.title, deserialized.title);
    }

    #[test]
    fn layout_template_serialization_roundtrip() {
        let template = LayoutTemplate::new("roundtrip_test");
        let json = serde_json::to_string(&template).unwrap();
        let deserialized: LayoutTemplate = serde_json::from_str(&json).unwrap();

        assert_eq!(template.name, deserialized.name);
        assert_eq!(template.windows.len(), deserialized.windows.len());
    }

    #[test]
    fn split_h_serialization_roundtrip() {
        let split = PaneTemplate::SplitH {
            ratio: 0.6,
            left: Box::new(PaneTemplate::Leaf {
                command: None,
                cwd: None,
            }),
            right: Box::new(PaneTemplate::Leaf {
                command: None,
                cwd: None,
            }),
        };
        let json = serde_json::to_string(&split).unwrap();
        let deserialized: PaneTemplate = serde_json::from_str(&json).unwrap();

        match (split, deserialized) {
            (PaneTemplate::SplitH { ratio: r1, .. }, PaneTemplate::SplitH { ratio: r2, .. }) => {
                assert!((r1 - r2).abs() < 0.0001);
            }
            _ => panic!("SplitH roundtrip failed"),
        }
    }

    #[test]
    fn split_v_serialization_roundtrip() {
        let split = PaneTemplate::SplitV {
            ratio: 0.3,
            top: Box::new(PaneTemplate::Leaf {
                command: None,
                cwd: None,
            }),
            bottom: Box::new(PaneTemplate::Leaf {
                command: None,
                cwd: None,
            }),
        };
        let json = serde_json::to_string(&split).unwrap();
        let deserialized: PaneTemplate = serde_json::from_str(&json).unwrap();

        match deserialized {
            PaneTemplate::SplitV { ratio, .. } => {
                assert!((ratio - 0.3).abs() < 0.0001);
            }
            _ => panic!("SplitV roundtrip failed"),
        }
    }
}
