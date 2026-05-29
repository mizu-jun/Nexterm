//! Quake mode (Sprint 5-7 / Phase 2-2).
//!
//! A global-hotkey toggle that slides a window in from one screen edge.
//! Equivalent to the "hotkey window" feature in Tilix / Guake / iTerm2.
//!
//! # Design
//!
//! - **Hotkey registration**: uses the `global-hotkey` crate, which supports
//!   Windows / macOS / X11. Wayland is not supported by that crate, so we
//!   complement it with an IPC path: `nexterm-ctl quake toggle` triggers a
//!   `ServerToClient::QuakeToggleRequest` that this module reacts to.
//! - **Hotkey event delivery**: `global-hotkey`'s `GlobalHotKeyEvent::receiver()`
//!   is crossbeam-channel based. Forwarding into the winit event loop is done
//!   by polling in `lifecycle::about_to_wait` (no `spawn` needed, plays nicely
//!   with winit 0.30).
//! - **Window control**: the toggle state lives in `ClientState.quake_visible`.
//!   When showing, the window position and size are computed from the monitor
//!   work area + `edge` + `height_pct` / `width_pct`. Decorations are hidden
//!   (borderless); the previous state is saved in `QuakeState` so we can restore it.
//!
//! # Platform-specific notes
//!
//! - **Linux Wayland**: global hotkey registration fails (warn log only). Users
//!   are expected to bind `nexterm-ctl quake toggle` through their compositor
//!   (e.g. Sway's `bindsym`).
//! - **macOS**: `set_window_level(AlwaysOnTop)` may still not appear above
//!   fullscreen apps (OS restriction).
//! - **Windows**: multi-monitor setups anchor to the primary monitor. Targeting
//!   a specific monitor is a future enhancement via a `monitor_name` option.

use anyhow::{Context, Result};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};
use nexterm_config::{QuakeEdge, QuakeModeConfig};
use tracing::{debug, info, warn};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    window::{Window, WindowId},
};

/// Saved window position / size / decoration state for the non-Quake (normal) mode.
#[derive(Debug, Clone)]
pub(crate) struct NormalWindowState {
    pub position: Option<PhysicalPosition<i32>>,
    pub size: PhysicalSize<u32>,
    pub decorations: bool,
}

/// Quake-mode runtime state.
///
/// `GlobalHotKeyManager` unregisters the hotkey on drop, so the client must keep
/// it alive for the lifetime of the runtime.
pub(crate) struct QuakeRuntime {
    /// Manager from the `global-hotkey` crate (unregisters on drop).
    _manager: Option<GlobalHotKeyManager>,
    /// ID of the registered hotkey (used to match incoming events).
    hotkey_id: Option<u32>,
    /// Visible state (true = Quake window is showing).
    pub visible: bool,
    /// Saved normal-mode window state (captured the first time we go visible).
    pub saved: Option<NormalWindowState>,
    /// Target OS Window for Quake mode (Sprint 5-8 Phase 4-1 Step 1.5).
    ///
    /// With multi-OS-window support (Phase 4-2 onwards) we keep Quake mode
    /// **pinned to a single main window**. `on_resumed` sets this to
    /// `Some(window_id)` once the primary window is initialized; from then on
    /// `handle_quake_tick` only operates on that WindowId.
    ///
    /// Currently `None` (`handle_quake_tick` falls back to `self.window`).
    /// Phase 4-2 onwards references `windows[target_window_id]` directly.
    #[allow(dead_code)]
    pub target_window_id: Option<WindowId>,
}

impl QuakeRuntime {
    /// Initialise a `GlobalHotKeyManager` per the Quake-mode config.
    ///
    /// Returns a runtime with no hotkey registered (but still functional via the
    /// IPC toggle path) when `enabled = false` or the hotkey fails to parse.
    /// These cases do not produce an error.
    pub fn new(cfg: &QuakeModeConfig) -> Self {
        if !cfg.enabled {
            debug!("Quake mode is disabled ([quake_mode] enabled=false)");
            return Self {
                _manager: None,
                hotkey_id: None,
                visible: false,
                saved: None,
                target_window_id: None,
            };
        }

        // Initialise the global-hotkey manager (may fail on Wayland and similar).
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    "failed to initialise GlobalHotKeyManager (Wayland?): {}. \
                     Quake mode is only available through `nexterm-ctl quake toggle`",
                    e
                );
                return Self {
                    _manager: None,
                    hotkey_id: None,
                    visible: false,
                    saved: None,
                    target_window_id: None,
                };
            }
        };

        let hotkey = match parse_hotkey(&cfg.hotkey) {
            Ok(h) => h,
            Err(e) => {
                warn!(
                    "failed to parse Quake hotkey '{}': {}. \
                     Quake mode is only available through `nexterm-ctl quake toggle`",
                    cfg.hotkey, e
                );
                return Self {
                    _manager: Some(manager),
                    hotkey_id: None,
                    visible: false,
                    saved: None,
                    target_window_id: None,
                };
            }
        };

        let id = hotkey.id();
        if let Err(e) = manager.register(hotkey) {
            warn!(
                "failed to register Quake hotkey '{}': {}. \
                 Another application may already own it",
                cfg.hotkey, e
            );
            return Self {
                _manager: Some(manager),
                hotkey_id: None,
                visible: false,
                saved: None,
                target_window_id: None,
            };
        }

        info!(
            "Quake mode enabled; registered hotkey '{}' (id={})",
            cfg.hotkey, id
        );
        Self {
            _manager: Some(manager),
            hotkey_id: Some(id),
            visible: false,
            saved: None,
            target_window_id: None,
        }
    }

    /// Decide whether a hotkey event matches this runtime's registered ID.
    ///
    /// Only returns true for `Pressed` events (releases are ignored).
    pub fn matches(&self, event: &GlobalHotKeyEvent) -> bool {
        if event.state != global_hotkey::HotKeyState::Pressed {
            return false;
        }
        self.hotkey_id == Some(event.id)
    }

    /// Drain accumulated hotkey events and return whether at least one `Pressed`
    /// event was seen.
    ///
    /// `global-hotkey` is channel-based, so calling this from winit's
    /// `about_to_wait` collapses repeated presses into a single toggle.
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

/// Convert a hotkey string into a `global_hotkey::hotkey::HotKey`.
///
/// Modifiers (`ctrl` / `alt` / `shift` / `super` — `meta` / `cmd` / `win` are
/// synonyms) are joined with `+`. The final token is the main key. Examples:
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
                    anyhow::bail!("hotkey string specifies multiple main keys: '{}'", s);
                }
                main_code = Some(
                    parse_code(other)
                        .with_context(|| format!("cannot interpret main hotkey '{}'", other))?,
                );
            }
        }
    }

    let code =
        main_code.ok_or_else(|| anyhow::anyhow!("hotkey string has no main key: '{}'", s))?;
    Ok(HotKey::new(Some(modifiers), code))
}

/// Convert a main-key string into a `Code` (lowercase assumed).
///
/// Covers single characters (a–z, 0–9) and the common named keys; does not
/// enumerate every `Code` variant — we focus on the keys commonly used for
/// Quake-mode bindings.
fn parse_code(name: &str) -> Result<Code> {
    let code = match name {
        // Digits.
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
        // Letters.
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
        // Special keys.
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
        // Symbols.
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
        // Function keys.
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
        _ => anyhow::bail!("unknown main key: '{}'", name),
    };
    Ok(code)
}

/// Show the window in Quake mode.
///
/// Saves the current window position / size / decorations into
/// `NormalWindowState`, then computes the target size from the monitor's work
/// area combined with `edge` + `height_pct` / `width_pct` and snaps the window
/// to that anchor. Decorations are hidden (borderless).
pub fn show_window(window: &Window, cfg: &QuakeModeConfig) -> Option<NormalWindowState> {
    let saved = NormalWindowState {
        position: window.outer_position().ok(),
        size: window.outer_size(),
        decorations: true, // normal mode assumes decorations are on
    };

    // Target monitor (prefer the primary).
    let monitor = window
        .current_monitor()
        .or_else(|| window.primary_monitor())
        .or_else(|| window.available_monitors().next());

    let Some(monitor) = monitor else {
        warn!("Quake mode: monitor info unavailable, falling back to normal display");
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
            // Center horizontally.
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

    // Apply the visible state.
    window.set_decorations(false);
    window.set_outer_position(target_pos);
    // winit 0.30: `request_inner_size` returns `Option<PhysicalSize<u32>>`; we
    // do not need the result here.
    let _ = window.request_inner_size(target_size);
    if cfg.always_on_top {
        window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
    }
    window.set_visible(true);
    window.focus_window();

    info!(
        "Quake mode shown: edge={:?} pos=({},{}) size={}x{}",
        cfg.edge, target_pos.x, target_pos.y, target_size.width, target_size.height
    );
    Some(saved)
}

/// Hide the Quake-mode window.
///
/// When `saved` is provided, restores the normal-mode decorations, position,
/// and size. With `minimize_on_hide = true` the window is minimised; otherwise
/// `set_visible(false)` is used.
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
    info!("Quake mode hidden");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hotkey_single_modifier() {
        let h = parse_hotkey("ctrl+`").unwrap();
        // global-hotkey's `HotKey` fields are not public; only equality is checked here.
        assert_eq!(h.mods, Modifiers::CONTROL);
        assert_eq!(h.key, Code::Backquote);
    }

    #[test]
    fn parse_hotkey_multiple_modifiers() {
        let h = parse_hotkey("ctrl+shift+t").unwrap();
        assert_eq!(h.mods, Modifiers::CONTROL | Modifiers::SHIFT);
        assert_eq!(h.key, Code::KeyT);
    }

    #[test]
    fn parse_hotkey_super_aliases() {
        for alias in ["super+space", "meta+space", "cmd+space", "win+space"] {
            let h = parse_hotkey(alias).unwrap();
            assert_eq!(
                h.mods,
                Modifiers::SUPER,
                "alias='{}' should resolve to SUPER",
                alias
            );
            assert_eq!(h.key, Code::Space);
        }
    }

    #[test]
    fn parse_hotkey_without_main_key_errors() {
        assert!(parse_hotkey("ctrl+shift").is_err());
    }

    #[test]
    fn parse_hotkey_with_multiple_main_keys_errors() {
        assert!(parse_hotkey("ctrl+a+b").is_err());
    }

    #[test]
    fn parse_hotkey_with_unknown_key_errors() {
        assert!(parse_hotkey("ctrl+xx").is_err());
    }

    #[test]
    fn parse_hotkey_allows_whitespace() {
        let h = parse_hotkey(" ctrl + a ").unwrap();
        assert_eq!(h.mods, Modifiers::CONTROL);
        assert_eq!(h.key, Code::KeyA);
    }

    #[test]
    fn parse_hotkey_symbol_keys() {
        assert_eq!(parse_hotkey("ctrl+`").unwrap().key, Code::Backquote);
        assert_eq!(parse_hotkey("alt+space").unwrap().key, Code::Space);
        assert_eq!(parse_hotkey("ctrl+slash").unwrap().key, Code::Slash);
    }

    #[test]
    fn parse_hotkey_function_keys() {
        assert_eq!(parse_hotkey("f1").unwrap().key, Code::F1);
        assert_eq!(parse_hotkey("alt+f12").unwrap().key, Code::F12);
    }

    #[test]
    fn quake_runtime_disabled_state() {
        let cfg = QuakeModeConfig::default();
        let rt = QuakeRuntime::new(&cfg);
        assert!(rt.hotkey_id.is_none());
        assert!(!rt.visible);
    }
}
