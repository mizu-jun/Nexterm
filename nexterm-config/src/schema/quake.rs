//! Quake モード設定（Sprint 5-7 / Phase 2-2）
//!
//! Quake モードとは、グローバルホットキーで画面上端（または下端・左端・右端）
//! からターミナルウィンドウをスライド表示・非表示できるトグル機能。
//! Tilix・Guake・iTerm2 の "Hotkey Window" 機能に相当する。
//!
//! 設定は `config.toml` の `[quake_mode]` セクションで指定する:
//!
//! ```toml
//! [quake_mode]
//! enabled = true
//! hotkey = "ctrl+`"
//! edge = "top"
//! height_pct = 45
//! width_pct = 100
//! animation_ms = 150
//! ```
//!
//! プラットフォーム制約:
//! - Linux Wayland では Wayland プロトコルの仕様上、グローバルホットキーは
//!   compositor 経由でのみ実現可能。本実装は `global-hotkey` クレートを用いる
//!   ため Windows / macOS / Linux X11 では動作するが、Wayland では動かない。
//!   Wayland ユーザーは `nexterm-ctl quake toggle` を compositor の `bindsym`
//!   等から呼び出すワークアラウンドを利用してほしい（README 参照）。

use serde::{Deserialize, Serialize};

/// Quake ウィンドウのアンカー位置
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum QuakeEdge {
    /// 画面上端（デフォルト）
    #[default]
    Top,
    /// 画面下端
    Bottom,
    /// 画面左端
    Left,
    /// 画面右端
    Right,
}

/// Quake モード設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuakeModeConfig {
    /// Quake モードを有効にするか。
    /// `false` の場合、ホットキー登録もウィンドウ装飾切替も行わない。
    pub enabled: bool,
    /// グローバルホットキー文字列（例: `"ctrl+`"`, `"alt+space"`）。
    /// 修飾子は `ctrl` / `alt` / `shift` / `super`（または `meta` / `cmd` / `win`）を
    /// `+` 区切りで連結する。最後のトークンが主キー。詳細は `global-hotkey` クレート参照。
    pub hotkey: String,
    /// アンカー位置（top/bottom/left/right）。
    pub edge: QuakeEdge,
    /// 画面の何 % の高さを使うか（top/bottom 時。1〜100）。
    pub height_pct: u8,
    /// 画面の何 % の幅を使うか（left/right 時、または top/bottom でも横幅指定したい場合。1〜100）。
    pub width_pct: u8,
    /// スライドアニメーション時間（ミリ秒）。0 で即時表示。
    pub animation_ms: u32,
    /// 表示時に常時最前面化（topmost）するか。
    pub always_on_top: bool,
    /// 非表示時にウィンドウを最小化するか（`false` の場合は `set_visible(false)` のみ）。
    /// macOS では `Hide` 相当となり Dock からも消えるため UX 上は `false` 推奨。
    pub minimize_on_hide: bool,
}

fn default_hotkey() -> String {
    // Ctrl + バッククォート（チルダキー）。Guake / Tilix のデフォルトに合わせる。
    "ctrl+`".to_string()
}

fn default_height_pct() -> u8 {
    45
}

fn default_width_pct() -> u8 {
    100
}

fn default_animation_ms() -> u32 {
    150
}

impl Default for QuakeModeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: default_hotkey(),
            edge: QuakeEdge::default(),
            height_pct: default_height_pct(),
            width_pct: default_width_pct(),
            animation_ms: default_animation_ms(),
            always_on_top: true,
            minimize_on_hide: false,
        }
    }
}

impl QuakeModeConfig {
    /// `height_pct` / `width_pct` を 1〜100 にクランプして返す。
    pub fn clamped_height_pct(&self) -> u8 {
        self.height_pct.clamp(1, 100)
    }

    /// `width_pct` のクランプ済みコピー。
    pub fn clamped_width_pct(&self) -> u8 {
        self.width_pct.clamp(1, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quake_デフォルトは無効() {
        let cfg = QuakeModeConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.hotkey, "ctrl+`");
        assert_eq!(cfg.edge, QuakeEdge::Top);
        assert_eq!(cfg.height_pct, 45);
        assert_eq!(cfg.width_pct, 100);
        assert_eq!(cfg.animation_ms, 150);
        assert!(cfg.always_on_top);
        assert!(!cfg.minimize_on_hide);
    }

    #[test]
    fn quake_clamped_pct_が範囲内に収まる() {
        let cfg_low = QuakeModeConfig {
            height_pct: 0,
            width_pct: 0,
            ..QuakeModeConfig::default()
        };
        assert_eq!(cfg_low.clamped_height_pct(), 1);
        assert_eq!(cfg_low.clamped_width_pct(), 1);

        let cfg_high = QuakeModeConfig {
            height_pct: 200,
            width_pct: 200,
            ..QuakeModeConfig::default()
        };
        assert_eq!(cfg_high.clamped_height_pct(), 100);
        assert_eq!(cfg_high.clamped_width_pct(), 100);
    }

    #[test]
    fn quake_toml往復() {
        let cfg = QuakeModeConfig {
            enabled: true,
            hotkey: "alt+space".to_string(),
            edge: QuakeEdge::Bottom,
            height_pct: 60,
            width_pct: 80,
            animation_ms: 200,
            always_on_top: false,
            minimize_on_hide: true,
        };
        let s = toml::to_string(&cfg).unwrap();
        let parsed: QuakeModeConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn quake_部分指定の_toml_でもデフォルト値で埋まる() {
        let toml_str = r#"
enabled = true
hotkey = "ctrl+space"
"#;
        let parsed: QuakeModeConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.hotkey, "ctrl+space");
        // 他はデフォルト
        assert_eq!(parsed.edge, QuakeEdge::Top);
        assert_eq!(parsed.height_pct, 45);
    }
}
