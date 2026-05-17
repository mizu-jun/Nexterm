//! アニメーション設定（Sprint 5-7 / Phase 3-2）
//!
//! タブ切替・ペイン追加・カーソルブリンク等の UI アニメーション全体を制御する。
//! `enabled = false` または `intensity = "off"` で完全に無効化でき、
//! アクセシビリティ（reduced motion）への配慮が可能。

use serde::{Deserialize, Serialize};

/// アニメーション強度（duration をスケールする係数を返す）。
///
/// 系統:
/// - `Off`     — 全て即時反映（0 ms）
/// - `Subtle`  — 控えめ（duration × 0.5）
/// - `Normal`  — 標準（duration × 1.0、デフォルト）
/// - `Energetic` — 強調（duration × 1.5）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AnimationIntensity {
    /// 無効（duration = 0）
    Off,
    /// 控えめ（× 0.5）
    Subtle,
    /// 標準（× 1.0）
    #[default]
    Normal,
    /// 強調（× 1.5）
    Energetic,
}

impl AnimationIntensity {
    /// 基準 duration（ミリ秒）に乗算する係数を返す。
    pub fn multiplier(&self) -> f32 {
        match self {
            AnimationIntensity::Off => 0.0,
            AnimationIntensity::Subtle => 0.5,
            AnimationIntensity::Normal => 1.0,
            AnimationIntensity::Energetic => 1.5,
        }
    }
}

/// アニメーション全体設定。
///
/// ```toml
/// [animations]
/// enabled = true
/// intensity = "normal"  # off / subtle / normal / energetic
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationsConfig {
    /// アニメーション全体の有効/無効。`false` の場合 intensity に関わらず全て即時反映
    #[serde(default = "default_animations_enabled")]
    pub enabled: bool,
    /// アニメーション強度（off / subtle / normal / energetic）
    #[serde(default)]
    pub intensity: AnimationIntensity,
}

fn default_animations_enabled() -> bool {
    // デフォルト有効。ユーザーが reduced motion を希望する場合は `enabled = false` で停止
    true
}

impl Default for AnimationsConfig {
    fn default() -> Self {
        Self {
            enabled: default_animations_enabled(),
            intensity: AnimationIntensity::default(),
        }
    }
}

impl AnimationsConfig {
    /// 有効な係数を返す（`enabled = false` または `Off` で 0）。
    pub fn effective_multiplier(&self) -> f32 {
        if self.enabled {
            self.intensity.multiplier()
        } else {
            0.0
        }
    }

    /// 基準 duration（ミリ秒）にスケールを掛けた実効 duration を返す。
    /// 0 を返すと「アニメーションなし（即時反映）」を意味する。
    pub fn scaled_duration_ms(&self, base_ms: u32) -> u32 {
        let mult = self.effective_multiplier();
        if mult <= 0.0 {
            return 0;
        }
        (base_ms as f32 * mult).round() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn デフォルトは有効_normal() {
        let cfg = AnimationsConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.intensity, AnimationIntensity::Normal);
        assert!((cfg.effective_multiplier() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn intensity_乗算係数の検証() {
        assert!((AnimationIntensity::Off.multiplier() - 0.0).abs() < f32::EPSILON);
        assert!((AnimationIntensity::Subtle.multiplier() - 0.5).abs() < f32::EPSILON);
        assert!((AnimationIntensity::Normal.multiplier() - 1.0).abs() < f32::EPSILON);
        assert!((AnimationIntensity::Energetic.multiplier() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn enabled_false_で全てが_0() {
        let cfg = AnimationsConfig {
            enabled: false,
            intensity: AnimationIntensity::Energetic,
        };
        assert_eq!(cfg.effective_multiplier(), 0.0);
        assert_eq!(cfg.scaled_duration_ms(200), 0);
    }

    #[test]
    fn off_で全てが_0() {
        let cfg = AnimationsConfig {
            enabled: true,
            intensity: AnimationIntensity::Off,
        };
        assert_eq!(cfg.effective_multiplier(), 0.0);
        assert_eq!(cfg.scaled_duration_ms(200), 0);
    }

    #[test]
    fn scaled_duration_ms_は係数を反映する() {
        let cfg = AnimationsConfig {
            enabled: true,
            intensity: AnimationIntensity::Subtle,
        };
        assert_eq!(cfg.scaled_duration_ms(200), 100); // 200 × 0.5
        let cfg = AnimationsConfig {
            enabled: true,
            intensity: AnimationIntensity::Energetic,
        };
        assert_eq!(cfg.scaled_duration_ms(200), 300); // 200 × 1.5
    }

    #[test]
    fn tomlでパースできる() {
        let toml_str = r#"
[animations]
enabled = true
intensity = "subtle"
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        assert!(parsed.animations.enabled);
        assert_eq!(parsed.animations.intensity, AnimationIntensity::Subtle);
    }

    #[test]
    fn デフォルト構造体のtoml往復() {
        let cfg = AnimationsConfig::default();
        let s = toml::to_string(&cfg).unwrap();
        let parsed: AnimationsConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, parsed);
    }
}
