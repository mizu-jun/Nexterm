//! Quake モード (Sprint 5-7 / Phase 2-2)
//!
//! グローバルホットキーで画面端からスライド表示するトグル機能。
//! Tilix / Guake / iTerm2 の "Hotkey Window" 相当。
//!
//! # 設計
//!
//! - **ホットキー登録**: `global-hotkey` クレートを使用し、Windows / macOS / X11 に対応。
//!   Wayland は本クレートが未対応のため、`nexterm-ctl quake toggle` から IPC 経由で
//!   `ServerToClient::QuakeToggleRequest` を受信してトグルする経路を併用する。
//! - **ホットキーイベント連携**: `global-hotkey` の `GlobalHotKeyEvent::receiver()` は
//!   crossbeam_channel ベース。winit イベントループへの転送は `lifecycle` 側の
//!   `about_to_wait` でポーリングする方式を採用（spawn 不要・winit 0.30 と相性が良い）。
//! - **ウィンドウ制御**: トグル状態を `ClientState.quake_visible` で保持し、表示時は
//!   モニタの作業領域から `edge` + `height_pct` / `width_pct` を計算してウィンドウ位置と
//!   サイズを設定する。装飾は隠す（borderless）が、戻すための旧状態を `QuakeState` に
//!   退避させる。
//!
//! # プラットフォーム別注意点
//!
//! - **Linux Wayland**: グローバルホットキー登録は失敗する（warn ログのみ）。
//!   ユーザーは compositor の `bindsym` 経由で `nexterm-ctl quake toggle` を実行する。
//! - **macOS**: `set_window_level(AlwaysOnTop)` は通常のアプリケーション層を超えるため
//!   フルスクリーンアプリより前面に来るとは限らない（OS 制約）。
//! - **Windows**: マルチモニタ環境ではプライマリモニタを基準に配置する。
//!   特定モニタ固定の要件は将来 `monitor_name` 設定で対応する。

use anyhow::{Context, Result};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};
use nexterm_config::{QuakeEdge, QuakeModeConfig};
use tracing::{debug, info, warn};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    window::Window,
};

/// 通常モード（非 Quake）時のウィンドウ位置・サイズ・装飾状態を保存する
#[derive(Debug, Clone)]
pub(crate) struct NormalWindowState {
    pub position: Option<PhysicalPosition<i32>>,
    pub size: PhysicalSize<u32>,
    pub decorations: bool,
}

/// Quake モード ランタイム状態
///
/// `GlobalHotKeyManager` は drop 時にホットキーを解除するため、ライフタイムの管理として
/// クライアントの上位構造体に長期保持する必要がある。
pub(crate) struct QuakeRuntime {
    /// global-hotkey クレートのマネージャ（drop でホットキー解除）
    _manager: Option<GlobalHotKeyManager>,
    /// 登録したホットキーの ID（イベントマッチ用）
    hotkey_id: Option<u32>,
    /// 表示状態（true=Quake 表示中）
    pub visible: bool,
    /// 通常モード時のウィンドウ状態（最初に表示する直前にスナップショット）
    pub saved: Option<NormalWindowState>,
}

impl QuakeRuntime {
    /// Quake モード設定に従って `GlobalHotKeyManager` を初期化する。
    ///
    /// `enabled=false` または hotkey パース失敗時はホットキーを登録せず、IPC 経由の
    /// トグルだけが利用可能な状態で返す（エラーにはしない）。
    pub fn new(cfg: &QuakeModeConfig) -> Self {
        if !cfg.enabled {
            debug!("Quake モードは無効化されています（[quake_mode] enabled=false）");
            return Self {
                _manager: None,
                hotkey_id: None,
                visible: false,
                saved: None,
            };
        }

        // global-hotkey マネージャの初期化（Wayland 等の未対応環境では失敗し得る）
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    "GlobalHotKeyManager の初期化に失敗しました（Wayland 等？）: {}。\
                     `nexterm-ctl quake toggle` 経由でのみ動作します",
                    e
                );
                return Self {
                    _manager: None,
                    hotkey_id: None,
                    visible: false,
                    saved: None,
                };
            }
        };

        let hotkey = match parse_hotkey(&cfg.hotkey) {
            Ok(h) => h,
            Err(e) => {
                warn!(
                    "Quake hotkey '{}' のパースに失敗しました: {}。\
                     `nexterm-ctl quake toggle` 経由でのみ動作します",
                    cfg.hotkey, e
                );
                return Self {
                    _manager: Some(manager),
                    hotkey_id: None,
                    visible: false,
                    saved: None,
                };
            }
        };

        let id = hotkey.id();
        if let Err(e) = manager.register(hotkey) {
            warn!(
                "Quake hotkey '{}' の登録に失敗しました: {}。\
                 別アプリで既に登録されている可能性があります",
                cfg.hotkey, e
            );
            return Self {
                _manager: Some(manager),
                hotkey_id: None,
                visible: false,
                saved: None,
            };
        }

        info!(
            "Quake モード有効。ホットキー '{}' を登録しました (id={})",
            cfg.hotkey, id
        );
        Self {
            _manager: Some(manager),
            hotkey_id: Some(id),
            visible: false,
            saved: None,
        }
    }

    /// ホットキーイベントが本ランタイムの登録 ID と一致するか判定する。
    ///
    /// `Pressed` 状態のみ true を返す（リリース時は無視）。
    pub fn matches(&self, event: &GlobalHotKeyEvent) -> bool {
        if event.state != global_hotkey::HotKeyState::Pressed {
            return false;
        }
        self.hotkey_id == Some(event.id)
    }

    /// 累積したホットキーイベントを排出して、Pressed が 1 回以上あったかを返す。
    ///
    /// global-hotkey はチャネルベースなので、winit の `about_to_wait` で本関数を呼んで
    /// 連続押下があった場合でも 1 トグルにまとめる。
    pub fn drain_pressed(&self) -> bool {
        let receiver = GlobalHotKeyEvent::receiver();
        let mut pressed = false;
        while let Ok(event) = receiver.try_recv() {
            if self.matches(&event) {
                pressed = true;
            }
        }
        pressed
    }
}

/// hotkey 文字列を `global_hotkey::hotkey::HotKey` に変換する。
///
/// 修飾子は `ctrl` / `alt` / `shift` / `super` (`meta` / `cmd` / `win` も同義) を
/// `+` 区切りで連結する。最後のトークンが主キー。例:
/// - `"ctrl+`"` → Ctrl + Backquote
/// - `"alt+space"` → Alt + Space
/// - `"super+shift+t"` → Super + Shift + T
pub fn parse_hotkey(s: &str) -> Result<HotKey> {
    let mut modifiers = Modifiers::empty();
    let mut main_code: Option<Code> = None;

    for token in s.split('+').map(|t| t.trim()).filter(|t| !t.is_empty()) {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "alt" | "option" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "super" | "meta" | "cmd" | "command" | "win" | "windows" => {
                modifiers |= Modifiers::SUPER;
            }
            other => {
                if main_code.is_some() {
                    anyhow::bail!("ホットキー文字列に主キーが複数指定されています: '{}'", s);
                }
                main_code =
                    Some(parse_code(other).with_context(|| {
                        format!("ホットキーの主キー '{}' を解釈できません", other)
                    })?);
            }
        }
    }

    let code = main_code
        .ok_or_else(|| anyhow::anyhow!("ホットキー文字列に主キーがありません: '{}'", s))?;
    Ok(HotKey::new(Some(modifiers), code))
}

/// 主キー文字列を `Code` に変換する（小文字前提）。
///
/// 単一文字（a-z, 0-9）と一般的な名前付きキーをサポートする。
/// 全 `Code` バリアントを網羅するわけではなく、Quake モードでよく使われるキーに絞る。
fn parse_code(name: &str) -> Result<Code> {
    let code = match name {
        // 数字
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        // アルファベット
        "a" => Code::KeyA,
        "b" => Code::KeyB,
        "c" => Code::KeyC,
        "d" => Code::KeyD,
        "e" => Code::KeyE,
        "f" => Code::KeyF,
        "g" => Code::KeyG,
        "h" => Code::KeyH,
        "i" => Code::KeyI,
        "j" => Code::KeyJ,
        "k" => Code::KeyK,
        "l" => Code::KeyL,
        "m" => Code::KeyM,
        "n" => Code::KeyN,
        "o" => Code::KeyO,
        "p" => Code::KeyP,
        "q" => Code::KeyQ,
        "r" => Code::KeyR,
        "s" => Code::KeyS,
        "t" => Code::KeyT,
        "u" => Code::KeyU,
        "v" => Code::KeyV,
        "w" => Code::KeyW,
        "x" => Code::KeyX,
        "y" => Code::KeyY,
        "z" => Code::KeyZ,
        // 特殊キー
        "space" => Code::Space,
        "tab" => Code::Tab,
        "enter" | "return" => Code::Enter,
        "esc" | "escape" => Code::Escape,
        "backspace" => Code::Backspace,
        "delete" | "del" => Code::Delete,
        "up" => Code::ArrowUp,
        "down" => Code::ArrowDown,
        "left" => Code::ArrowLeft,
        "right" => Code::ArrowRight,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" | "pgup" => Code::PageUp,
        "pagedown" | "pgdn" => Code::PageDown,
        // 記号
        "`" | "backquote" | "grave" => Code::Backquote,
        "-" | "minus" => Code::Minus,
        "=" | "equal" => Code::Equal,
        "[" | "leftbracket" => Code::BracketLeft,
        "]" | "rightbracket" => Code::BracketRight,
        "\\" | "backslash" => Code::Backslash,
        ";" | "semicolon" => Code::Semicolon,
        "'" | "quote" => Code::Quote,
        "," | "comma" => Code::Comma,
        "." | "period" => Code::Period,
        "/" | "slash" => Code::Slash,
        // F キー
        "f1" => Code::F1,
        "f2" => Code::F2,
        "f3" => Code::F3,
        "f4" => Code::F4,
        "f5" => Code::F5,
        "f6" => Code::F6,
        "f7" => Code::F7,
        "f8" => Code::F8,
        "f9" => Code::F9,
        "f10" => Code::F10,
        "f11" => Code::F11,
        "f12" => Code::F12,
        _ => anyhow::bail!("未知の主キー: '{}'", name),
    };
    Ok(code)
}

/// Quake モードでウィンドウを「表示」する。
///
/// 現在のウィンドウ位置・サイズ・装飾を `NormalWindowState` に退避し、
/// モニタ作業領域から `edge` + `height_pct` / `width_pct` でサイズを計算して
/// アンカー位置に配置する。装飾は隠す（borderless）。
pub fn show_window(window: &Window, cfg: &QuakeModeConfig) -> Option<NormalWindowState> {
    let saved = NormalWindowState {
        position: window.outer_position().ok(),
        size: window.outer_size(),
        decorations: true, // 通常モードは装飾あり前提
    };

    // 配置先モニタ（プライマリ優先）
    let monitor = window
        .current_monitor()
        .or_else(|| window.primary_monitor())
        .or_else(|| window.available_monitors().next());

    let Some(monitor) = monitor else {
        warn!("Quake モード: モニタ情報を取得できないため通常表示します");
        window.set_visible(true);
        window.focus_window();
        return Some(saved);
    };

    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let height_pct = cfg.clamped_height_pct() as u32;
    let width_pct = cfg.clamped_width_pct() as u32;

    let (target_pos, target_size) = match cfg.edge {
        QuakeEdge::Top => {
            let w = mon_size.width * width_pct / 100;
            let h = mon_size.height * height_pct / 100;
            // 横方向は中央寄せ
            let x = mon_pos.x + ((mon_size.width - w) / 2) as i32;
            let y = mon_pos.y;
            (PhysicalPosition::new(x, y), PhysicalSize::new(w, h))
        }
        QuakeEdge::Bottom => {
            let w = mon_size.width * width_pct / 100;
            let h = mon_size.height * height_pct / 100;
            let x = mon_pos.x + ((mon_size.width - w) / 2) as i32;
            let y = mon_pos.y + (mon_size.height - h) as i32;
            (PhysicalPosition::new(x, y), PhysicalSize::new(w, h))
        }
        QuakeEdge::Left => {
            let w = mon_size.width * width_pct / 100;
            let h = mon_size.height * height_pct / 100;
            let x = mon_pos.x;
            let y = mon_pos.y + ((mon_size.height - h) / 2) as i32;
            (PhysicalPosition::new(x, y), PhysicalSize::new(w, h))
        }
        QuakeEdge::Right => {
            let w = mon_size.width * width_pct / 100;
            let h = mon_size.height * height_pct / 100;
            let x = mon_pos.x + (mon_size.width - w) as i32;
            let y = mon_pos.y + ((mon_size.height - h) / 2) as i32;
            (PhysicalPosition::new(x, y), PhysicalSize::new(w, h))
        }
    };

    // 表示状態を整える
    window.set_decorations(false);
    window.set_outer_position(target_pos);
    // winit 0.30: request_inner_size は Option<PhysicalSize<u32>> を返すが本実装では結果不要
    let _ = window.request_inner_size(target_size);
    if cfg.always_on_top {
        window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
    }
    window.set_visible(true);
    window.focus_window();

    info!(
        "Quake モード表示: edge={:?} pos=({},{}) size={}x{}",
        cfg.edge, target_pos.x, target_pos.y, target_size.width, target_size.height
    );
    Some(saved)
}

/// Quake モードでウィンドウを「非表示」にする。
///
/// `saved` がある場合は通常モードの装飾・位置・サイズを復元する。
/// `minimize_on_hide=true` の場合は最小化、それ以外は `set_visible(false)`。
pub fn hide_window(window: &Window, cfg: &QuakeModeConfig, saved: Option<&NormalWindowState>) {
    if cfg.minimize_on_hide {
        window.set_minimized(true);
    } else {
        window.set_visible(false);
    }
    if cfg.always_on_top {
        window.set_window_level(winit::window::WindowLevel::Normal);
    }
    if let Some(state) = saved {
        window.set_decorations(state.decorations);
        if let Some(pos) = state.position {
            window.set_outer_position(pos);
        }
        let _ = window.request_inner_size(state.size);
    }
    info!("Quake モード非表示");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hotkey_単一修飾子() {
        let h = parse_hotkey("ctrl+`").unwrap();
        // global-hotkey の HotKey はフィールド public ではないので equality 確認のみ
        assert_eq!(h.mods, Modifiers::CONTROL);
        assert_eq!(h.key, Code::Backquote);
    }

    #[test]
    fn parse_hotkey_複数修飾子() {
        let h = parse_hotkey("ctrl+shift+t").unwrap();
        assert_eq!(h.mods, Modifiers::CONTROL | Modifiers::SHIFT);
        assert_eq!(h.key, Code::KeyT);
    }

    #[test]
    fn parse_hotkey_super_別名() {
        for alias in ["super+space", "meta+space", "cmd+space", "win+space"] {
            let h = parse_hotkey(alias).unwrap();
            assert_eq!(
                h.mods,
                Modifiers::SUPER,
                "alias='{}' で SUPER 認識されるべき",
                alias
            );
            assert_eq!(h.key, Code::Space);
        }
    }

    #[test]
    fn parse_hotkey_主キーなしはエラー() {
        assert!(parse_hotkey("ctrl+shift").is_err());
    }

    #[test]
    fn parse_hotkey_主キー複数はエラー() {
        assert!(parse_hotkey("ctrl+a+b").is_err());
    }

    #[test]
    fn parse_hotkey_未知のキーはエラー() {
        assert!(parse_hotkey("ctrl+xx").is_err());
    }

    #[test]
    fn parse_hotkey_空白を許容() {
        let h = parse_hotkey(" ctrl + a ").unwrap();
        assert_eq!(h.mods, Modifiers::CONTROL);
        assert_eq!(h.key, Code::KeyA);
    }

    #[test]
    fn parse_hotkey_記号類() {
        assert_eq!(parse_hotkey("ctrl+`").unwrap().key, Code::Backquote);
        assert_eq!(parse_hotkey("alt+space").unwrap().key, Code::Space);
        assert_eq!(parse_hotkey("ctrl+slash").unwrap().key, Code::Slash);
    }

    #[test]
    fn parse_hotkey_fキー() {
        assert_eq!(parse_hotkey("f1").unwrap().key, Code::F1);
        assert_eq!(parse_hotkey("alt+f12").unwrap().key, Code::F12);
    }

    #[test]
    fn quake_runtime_disabled_状態() {
        let cfg = QuakeModeConfig::default();
        let rt = QuakeRuntime::new(&cfg);
        assert!(rt.hotkey_id.is_none());
        assert!(!rt.visible);
    }
}
