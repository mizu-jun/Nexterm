//! カラースキーム（組み込みパレット + カスタムパレット）

use serde::{Deserialize, Serialize};

/// カラースキームのパレット（前景・背景・ANSI 16色）
#[derive(Debug, Clone)]
pub struct SchemePalette {
    /// デフォルト前景色 [R, G, B]
    pub fg: [u8; 3],
    /// デフォルト背景色 [R, G, B]
    pub bg: [u8; 3],
    /// ANSI 16色パレット（0=black … 15=bright white）
    pub ansi: [[u8; 3]; 16],
}

/// 組み込みカラースキーム
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinScheme {
    /// ダークテーマ
    Dark,
    /// ライトテーマ
    Light,
    /// Tokyo Night テーマ
    TokyoNight,
    /// Solarized テーマ
    Solarized,
    /// Gruvbox テーマ
    Gruvbox,
    /// Catppuccin テーマ
    Catppuccin,
    /// Dracula テーマ
    Dracula,
    /// Nord テーマ
    Nord,
    #[serde(rename = "onedark")]
    /// One Dark テーマ
    OneDark,
}

impl BuiltinScheme {
    /// スキームの表示名を返す
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::TokyoNight => "Tokyo Night",
            Self::Solarized => "Solarized",
            Self::Gruvbox => "Gruvbox",
            Self::Catppuccin => "Catppuccin",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::OneDark => "One Dark",
        }
    }

    /// スキームの TOML 識別子（lowercase）を返す
    pub fn toml_name(&self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::TokyoNight => "tokyonight",
            Self::Solarized => "solarized",
            Self::Gruvbox => "gruvbox",
            Self::Catppuccin => "catppuccin",
            Self::Dracula => "dracula",
            Self::Nord => "nord",
            Self::OneDark => "onedark",
        }
    }

    /// すべての組み込みスキームをリストで返す（Sprint 5-4 / D4: テーマギャラリー）
    pub fn all() -> &'static [Self] {
        &[
            Self::Dark,
            Self::Light,
            Self::TokyoNight,
            Self::Solarized,
            Self::Gruvbox,
            Self::Catppuccin,
            Self::Dracula,
            Self::Nord,
            Self::OneDark,
        ]
    }

    /// TOML 識別子から組み込みスキームを取得する。
    ///
    /// 大文字小文字は問わない。未知の名前は `None` を返す（旧 `parse_builtin_scheme`
    /// は不明値を Dark にフォールバックしていたが、本メソッドは厳格チェック用）。
    pub fn from_toml_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "tokyonight" => Some(Self::TokyoNight),
            "solarized" => Some(Self::Solarized),
            "gruvbox" => Some(Self::Gruvbox),
            "catppuccin" => Some(Self::Catppuccin),
            "dracula" => Some(Self::Dracula),
            "nord" => Some(Self::Nord),
            "onedark" => Some(Self::OneDark),
            _ => None,
        }
    }

    /// スキームのカラーパレットを返す
    pub fn palette(&self) -> SchemePalette {
        match self {
            Self::Dark => SchemePalette {
                fg: [0xD8, 0xD8, 0xD8],
                bg: [0x0D, 0x0D, 0x0D],
                ansi: [
                    [0x00, 0x00, 0x00],
                    [0x80, 0x00, 0x00],
                    [0x00, 0x80, 0x00],
                    [0x80, 0x80, 0x00],
                    [0x00, 0x00, 0x80],
                    [0x80, 0x00, 0x80],
                    [0x00, 0x80, 0x80],
                    [0xC0, 0xC0, 0xC0],
                    [0x80, 0x80, 0x80],
                    [0xFF, 0x00, 0x00],
                    [0x00, 0xFF, 0x00],
                    [0xFF, 0xFF, 0x00],
                    [0x00, 0x00, 0xFF],
                    [0xFF, 0x00, 0xFF],
                    [0x00, 0xFF, 0xFF],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::Light => SchemePalette {
                fg: [0x2C, 0x2C, 0x2C],
                bg: [0xF2, 0xF2, 0xF2],
                ansi: [
                    [0x00, 0x00, 0x00],
                    [0xC0, 0x00, 0x00],
                    [0x00, 0x80, 0x00],
                    [0x80, 0x80, 0x00],
                    [0x00, 0x00, 0xC0],
                    [0xC0, 0x00, 0xC0],
                    [0x00, 0x80, 0x80],
                    [0xC0, 0xC0, 0xC0],
                    [0x60, 0x60, 0x60],
                    [0xFF, 0x40, 0x40],
                    [0x00, 0xC0, 0x00],
                    [0xC0, 0xC0, 0x00],
                    [0x40, 0x40, 0xFF],
                    [0xFF, 0x40, 0xFF],
                    [0x00, 0xC0, 0xC0],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::TokyoNight => SchemePalette {
                fg: [0xC0, 0xCA, 0xF5],
                bg: [0x1A, 0x1B, 0x26],
                ansi: [
                    [0x15, 0x16, 0x20],
                    [0xF7, 0x76, 0x8E],
                    [0x9E, 0xCE, 0x6A],
                    [0xE0, 0xAF, 0x68],
                    [0x7A, 0xA2, 0xF7],
                    [0xBB, 0x9A, 0xF7],
                    [0x7D, 0xCF, 0xFF],
                    [0xA9, 0xB1, 0xD6],
                    [0x41, 0x4B, 0x67],
                    [0xF7, 0x76, 0x8E],
                    [0x9E, 0xCE, 0x6A],
                    [0xE0, 0xAF, 0x68],
                    [0x7A, 0xA2, 0xF7],
                    [0xBB, 0x9A, 0xF7],
                    [0x7D, 0xCF, 0xFF],
                    [0xC0, 0xCA, 0xF5],
                ],
            },
            Self::Solarized => SchemePalette {
                fg: [0x83, 0x94, 0x96],
                bg: [0x00, 0x2B, 0x36],
                ansi: [
                    [0x07, 0x36, 0x42],
                    [0xDC, 0x32, 0x2F],
                    [0x85, 0x99, 0x00],
                    [0xB5, 0x89, 0x00],
                    [0x26, 0x8B, 0xD2],
                    [0xD3, 0x36, 0x82],
                    [0x2A, 0xA1, 0x98],
                    [0xEE, 0xE8, 0xD5],
                    [0x00, 0x2B, 0x36],
                    [0xCB, 0x4B, 0x16],
                    [0x58, 0x6E, 0x75],
                    [0x65, 0x7B, 0x83],
                    [0x83, 0x94, 0x96],
                    [0x6C, 0x71, 0xC4],
                    [0x93, 0xA1, 0xA1],
                    [0xFD, 0xF6, 0xE3],
                ],
            },
            Self::Gruvbox => SchemePalette {
                fg: [0xEB, 0xDB, 0xB2],
                bg: [0x28, 0x28, 0x28],
                ansi: [
                    [0x28, 0x28, 0x28],
                    [0xCC, 0x24, 0x1D],
                    [0x98, 0x97, 0x1A],
                    [0xD7, 0x99, 0x21],
                    [0x45, 0x85, 0x88],
                    [0xB1, 0x62, 0x86],
                    [0x68, 0x9D, 0x6A],
                    [0xA8, 0x99, 0x84],
                    [0x92, 0x83, 0x74],
                    [0xFB, 0x49, 0x34],
                    [0xB8, 0xBB, 0x26],
                    [0xFA, 0xBD, 0x2F],
                    [0x83, 0xA5, 0x98],
                    [0xD3, 0x86, 0x9B],
                    [0x8E, 0xC0, 0x7C],
                    [0xEB, 0xDB, 0xB2],
                ],
            },
            Self::Catppuccin => SchemePalette {
                // Catppuccin Mocha
                fg: [0xCD, 0xD6, 0xF4],
                bg: [0x1E, 0x1E, 0x2E],
                ansi: [
                    [0x45, 0x47, 0x5A],
                    [0xF3, 0x8B, 0xA8],
                    [0xA6, 0xE3, 0xA1],
                    [0xF9, 0xE2, 0xAF],
                    [0x89, 0xB4, 0xFA],
                    [0xF5, 0xC2, 0xE7],
                    [0x94, 0xE2, 0xD5],
                    [0xBA, 0xC2, 0xDE],
                    [0x58, 0x5B, 0x70],
                    [0xF3, 0x8B, 0xA8],
                    [0xA6, 0xE3, 0xA1],
                    [0xF9, 0xE2, 0xAF],
                    [0x89, 0xB4, 0xFA],
                    [0xF5, 0xC2, 0xE7],
                    [0x94, 0xE2, 0xD5],
                    [0xA6, 0xAD, 0xC8],
                ],
            },
            Self::Dracula => SchemePalette {
                fg: [0xF8, 0xF8, 0xF2],
                bg: [0x28, 0x2A, 0x36],
                ansi: [
                    [0x21, 0x22, 0x2C],
                    [0xFF, 0x55, 0x55],
                    [0x50, 0xFA, 0x7B],
                    [0xF1, 0xFA, 0x8C],
                    [0xBD, 0x93, 0xF9],
                    [0xFF, 0x79, 0xC6],
                    [0x8B, 0xE9, 0xFD],
                    [0xF8, 0xF8, 0xF2],
                    [0x6B, 0x72, 0x89],
                    [0xFF, 0x6E, 0x6E],
                    [0x69, 0xFF, 0x94],
                    [0xFF, 0xFF, 0xA5],
                    [0xD6, 0xAC, 0xFF],
                    [0xFF, 0x92, 0xDF],
                    [0xA4, 0xFF, 0xFF],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::Nord => SchemePalette {
                fg: [0xD8, 0xDE, 0xE9],
                bg: [0x2E, 0x34, 0x40],
                ansi: [
                    [0x3B, 0x42, 0x52],
                    [0xBF, 0x61, 0x6A],
                    [0xA3, 0xBE, 0x8C],
                    [0xEB, 0xCB, 0x8B],
                    [0x81, 0xA1, 0xC1],
                    [0xB4, 0x8E, 0xAD],
                    [0x88, 0xC0, 0xD0],
                    [0xE5, 0xE9, 0xF0],
                    [0x4C, 0x56, 0x6A],
                    [0xBF, 0x61, 0x6A],
                    [0xA3, 0xBE, 0x8C],
                    [0xEB, 0xCB, 0x8B],
                    [0x81, 0xA1, 0xC1],
                    [0xB4, 0x8E, 0xAD],
                    [0x8F, 0xBD, 0xBB],
                    [0xEC, 0xEF, 0xF4],
                ],
            },
            Self::OneDark => SchemePalette {
                fg: [0xAB, 0xB2, 0xBF],
                bg: [0x28, 0x2C, 0x34],
                ansi: [
                    [0x28, 0x2C, 0x34],
                    [0xE0, 0x6C, 0x75],
                    [0x98, 0xC3, 0x79],
                    [0xE5, 0xC0, 0x7B],
                    [0x61, 0xAF, 0xEF],
                    [0xC6, 0x78, 0xDD],
                    [0x56, 0xB6, 0xC2],
                    [0xAB, 0xB2, 0xBF],
                    [0x5C, 0x63, 0x70],
                    [0xE0, 0x6C, 0x75],
                    [0x98, 0xC3, 0x79],
                    [0xE5, 0xC0, 0x7B],
                    [0x61, 0xAF, 0xEF],
                    [0xC6, 0x78, 0xDD],
                    [0x56, 0xB6, 0xC2],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
        }
    }
}

/// カスタムカラーパレット（TOML で定義）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomPalette {
    /// 前景色 (#RRGGBB)
    pub foreground: String,
    /// 背景色 (#RRGGBB)
    pub background: String,
    /// カーソル色 (#RRGGBB)
    pub cursor: String,
    /// ANSI 16色 (#RRGGBB × 16: black, red, green, yellow, blue, magenta, cyan, white, bright×8)
    pub ansi: Vec<String>,
}

/// カラースキーム設定
///
/// TOML では以下の 3 形式を受け付ける（カスタム deserializer で対応）:
/// 1. `colors = "tokyonight"` — 文字列で組み込みスキーム指定
/// 2. `[colors] scheme = "tokyonight"` — テーブル形式（公式ドキュメント記載）
/// 3. `[colors] foreground = "#..." background = "#..." ...` — 完全カスタムパレット
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ColorScheme {
    /// 組み込みスキーム名
    Builtin(BuiltinScheme),
    /// カスタムパレット
    Custom(CustomPalette),
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::Builtin(BuiltinScheme::TokyoNight)
    }
}

impl<'de> Deserialize<'de> for ColorScheme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct ColorSchemeVisitor;

        impl<'de> Visitor<'de> for ColorSchemeVisitor {
            type Value = ColorScheme;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(
                    "string (組み込みスキーム名) または table ([colors] scheme = \"...\" / カスタムパレット)",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(ColorScheme::Builtin(parse_builtin_scheme(value)))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(ColorScheme::Builtin(parse_builtin_scheme(&value)))
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                // 一旦すべてのキーを HashMap に集める
                let mut entries: std::collections::HashMap<String, toml::Value> =
                    std::collections::HashMap::new();
                while let Some((key, value)) = map.next_entry::<String, toml::Value>()? {
                    entries.insert(key, value);
                }

                // パターン 2: [colors] scheme = "tokyonight"
                if let Some(scheme) = entries.get("scheme")
                    && let Some(name) = scheme.as_str()
                {
                    return Ok(ColorScheme::Builtin(parse_builtin_scheme(name)));
                }

                // パターン 3: フルカスタムパレット（foreground / background / cursor / ansi）
                let palette: CustomPalette = toml::Value::Table(entries.into_iter().collect())
                    .try_into()
                    .map_err(|e| {
                        de::Error::custom(format!("カスタムパレットのパースに失敗: {}", e))
                    })?;
                Ok(ColorScheme::Custom(palette))
            }
        }

        deserializer.deserialize_any(ColorSchemeVisitor)
    }
}

/// 組み込みスキーム名を `BuiltinScheme` にパースする。未知の値は Dark にフォールバック。
///
/// Sprint 5-4 / D4: 旧版では 5 種類しかパースできず、Catppuccin / Dracula / Nord /
/// OneDark を指定しても Dark にフォールバックしていた。`BuiltinScheme::from_toml_name`
/// に委譲して全 9 種類を扱えるように修正。
fn parse_builtin_scheme(s: &str) -> BuiltinScheme {
    BuiltinScheme::from_toml_name(s).unwrap_or(BuiltinScheme::Dark)
}
