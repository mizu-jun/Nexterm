//! Settings panel — Ctrl+, opens the floating UI (multi-category layout with left sidebar).

use anyhow::Result;
use nexterm_config::toml_path;

/// Phase 5-11-8 Step 8-3 (Sub-phase A): inline text-input state.
///
/// Holds the in-flight edit state for `TextInput` fields inside the settings
/// panel. Used to edit the SSH host name / host / username fields.
/// IME preedit text (Sub-phase B) is stored in the `preedit` field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInputState {
    /// Edit buffer.
    pub buffer: String,
    /// Cursor position (byte index inside `buffer`).
    /// Invariant: `buffer.is_char_boundary(cursor) == true`.
    pub cursor: usize,
    /// IME preedit text (used in Sub-phase B). `None` means no preedit in flight.
    pub preedit: Option<String>,
}

impl TextInputState {
    /// Build a `TextInputState` from an initial string; cursor goes to the end.
    pub fn new(initial: String) -> Self {
        let cursor = initial.len();
        Self {
            buffer: initial,
            cursor,
            preedit: None,
        }
    }

    /// Insert a single character at the cursor and advance past it.
    pub fn insert_char(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// Insert a string at the cursor and advance past it.
    /// Also used to commit multiple characters at once via the IME `Commit` path.
    pub fn insert_str(&mut self, s: &str) {
        self.buffer.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Delete the character immediately before the cursor (Backspace).
    /// Honours multibyte boundaries by doing a manual `floor_char_boundary`-style scan.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Find the character boundary immediately before the cursor.
        let mut prev = self.cursor - 1;
        while prev > 0 && !self.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        self.buffer.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    /// Delete the character immediately after the cursor (Delete).
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut next = self.cursor + 1;
        while next < self.buffer.len() && !self.buffer.is_char_boundary(next) {
            next += 1;
        }
        self.buffer.replace_range(self.cursor..next, "");
    }

    /// Move the cursor one character left.
    pub fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut prev = self.cursor - 1;
        while prev > 0 && !self.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        self.cursor = prev;
    }

    /// Move the cursor one character right.
    pub fn move_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut next = self.cursor + 1;
        while next < self.buffer.len() && !self.buffer.is_char_boundary(next) {
            next += 1;
        }
        self.cursor = next;
    }

    /// Move the cursor to the start of the buffer.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end of the buffer.
    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    /// Return the display string. With `preedit == None`, returns the buffer
    /// as-is; with `Some(pe)`, returns the string with the preedit inserted at
    /// the cursor.
    pub fn display_string(&self) -> String {
        match &self.preedit {
            None => self.buffer.clone(),
            Some(pe) => {
                let mut s = self.buffer.clone();
                s.insert_str(self.cursor, pe);
                s
            }
        }
    }

    /// Return the cursor position (in bytes) inside the display string.
    /// When a preedit is present, points to the end of the preedit (matches
    /// the visual cursor before IME commit).
    pub fn display_cursor(&self) -> usize {
        match &self.preedit {
            None => self.cursor,
            Some(pe) => self.cursor + pe.len(),
        }
    }
}

/// Slider variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderType {
    FontSize,
    WindowOpacity,
    /// Phase 5-11-6 #6: horizontal window padding (0–32 px).
    WindowPaddingX,
    /// Phase 5-11-6 #6: vertical window padding (0–32 px).
    WindowPaddingY,
}

/// State of an in-flight slider drag.
#[derive(Debug, Clone)]
pub struct SliderDrag {
    /// Which slider is being dragged.
    pub slider_type: SliderType,
    /// Slider track start X (pixels).
    pub track_x: f32,
    /// Slider track width (pixels).
    pub track_w: f32,
    /// Slider minimum value.
    #[allow(dead_code)]
    pub min_val: f32,
    /// Slider maximum value.
    #[allow(dead_code)]
    pub max_val: f32,
}

/// Sidebar category.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsCategory {
    Startup,
    Font,
    Theme,
    Window,
    Ssh,
    Keybindings,
    Profiles,
    /// Phase 2c-G: read-only view of the `[blocks]` config section. The
    /// values themselves are still edited through `config.toml`; making the
    /// toggles clickable lands in a follow-up.
    Blocks,
}

impl SettingsCategory {
    pub const ALL: &'static [SettingsCategory] = &[
        SettingsCategory::Startup,
        SettingsCategory::Font,
        SettingsCategory::Theme,
        SettingsCategory::Window,
        SettingsCategory::Ssh,
        SettingsCategory::Keybindings,
        SettingsCategory::Profiles,
        SettingsCategory::Blocks,
    ];

    pub fn label(&self) -> &str {
        match self {
            SettingsCategory::Startup => "Startup",
            SettingsCategory::Font => "Font",
            SettingsCategory::Theme => "Theme",
            SettingsCategory::Window => "Window",
            SettingsCategory::Ssh => "SSH",
            SettingsCategory::Keybindings => "Keybindings",
            SettingsCategory::Profiles => "Profiles",
            SettingsCategory::Blocks => "Blocks",
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            SettingsCategory::Startup => "▶",
            SettingsCategory::Font => "Aa",
            SettingsCategory::Theme => "◐",
            SettingsCategory::Window => "▢",
            SettingsCategory::Ssh => "⊞",
            SettingsCategory::Keybindings => "⌨",
            SettingsCategory::Profiles => "◉",
            SettingsCategory::Blocks => "▤",
        }
    }
}

/// Profile entry (editable inside the settings panel).
#[derive(Debug, Clone)]
pub struct ProfileEntry {
    pub name: String,
    pub icon: String,
    #[allow(dead_code)]
    pub shell_program: String,
    #[allow(dead_code)]
    pub working_dir: String,
}

impl Default for ProfileEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            icon: ">".to_string(),
            shell_program: String::new(),
            working_dir: String::new(),
        }
    }
}

/// Key binding entry (Phase 5-11-9 Sub-phase A: editable inside the settings panel).
///
/// A lightweight mirror of `nexterm-config::KeyBinding`. Sub-phase A populates
/// the list from `Config.keys` for display only; Sub-phase B/C/D add Record-mode
/// key capture, Action ComboBox cycling, and Add/Delete UI respectively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindingEntry {
    /// Key string (e.g. `"ctrl+shift+p"`, `"ctrl+b d"`). Matches the format
    /// accepted by `nexterm_client_gpu::key_map::config_key_matches_token`.
    pub key: String,
    /// Action name (e.g. `"CommandPalette"`). Must be one of the 27 actions
    /// dispatched by `execute_action` in `renderer::input_handler::action`.
    pub action: String,
}

impl KeyBindingEntry {
    /// Build the one-line label rendered / announced by the UI / SR.
    /// Example: `"ctrl+shift+p → CommandPalette"`.
    pub fn label(&self) -> String {
        let key = if self.key.is_empty() {
            "(unbound)"
        } else {
            self.key.as_str()
        };
        let action = if self.action.is_empty() {
            "(none)"
        } else {
            self.action.as_str()
        };
        format!("{} → {}", key, action)
    }
}

/// Phase 5-11-9 Sub-phase B: in-flight edit state for the key string of the
/// currently selected binding (`selected_key_index`).
///
/// Two modes:
///   - `Record`: the next non-modifier key press is captured via
///     `format_key_event` and committed to the binding. Useful for simple
///     combos like `ctrl+shift+p`.
///   - `Text(state)`: free-form text editing. Required for prefix bindings
///     (e.g. `"ctrl+b d"`) that cannot be expressed by a single physical
///     press. Tab toggles between modes; Enter commits Text mode; Esc cancels.
#[derive(Debug, Clone)]
pub enum KeyEditMode {
    /// Awaiting the next physical key press.
    Record,
    /// Free-form text edit (cursor + IME preedit aware).
    Text(TextInputState),
}

/// Allowed action names (Phase 5-11-9 Sub-phase A).
///
/// Mirror of the 27 `match` arms in `renderer::input_handler::action::execute_action`.
/// Used by Sub-phase C to populate the Action ComboBox.
/// Q2 decision: fixed list (no free-form input) to prevent silent typos.
pub const KEYBINDING_ACTIONS: &[&str] = &[
    "Quit",
    "SearchScrollback",
    "SplitVertical",
    "SplitHorizontal",
    "FocusNextPane",
    "FocusPrevPane",
    "ClosePane",
    "NewWindow",
    "Detach",
    "CommandPalette",
    "SetBroadcastOn",
    "SetBroadcastOff",
    "ToggleZoom",
    "QuickSelect",
    "SwapPaneNext",
    "SwapPanePrev",
    "BreakPane",
    "ShowSettings",
    "ShowHostManager",
    "ShowMacroPicker",
    "SftpUploadDialog",
    "SftpDownloadDialog",
    "ConnectSerialPrompt",
    "JumpPrevPrompt",
    "JumpNextPrompt",
    "DetachToNewWindow",
    "CloseOsWindow",
];

/// SSH host entry (Phase 5-11-8 Step 8-1: display-only inside the settings panel).
///
/// A lightweight subset of `nexterm-config::HostConfig` that keeps only the
/// fields needed for SR / settings-panel display. Step 8-2 / 8-3 will extend
/// the struct when edit functionality lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHostEntry {
    /// Display name (`HostConfig.name`).
    pub name: String,
    /// Hostname or IP address.
    pub host: String,
    /// SSH port.
    pub port: u16,
    /// Username.
    pub username: String,
    /// Authentication type (`"password"` / `"key"` / `"agent"`).
    pub auth_type: String,
}

impl SshHostEntry {
    /// Build the one-line label rendered / announced by the UI / SR.
    /// Example: `"myhost (alice@example.com:2222)"`.
    pub fn label(&self) -> String {
        let user_part = if self.username.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.username, self.host)
        };
        let endpoint = if self.port == 22 || self.port == 0 {
            user_part
        } else {
            format!("{}:{}", user_part, self.port)
        };
        if self.name.is_empty() {
            endpoint
        } else {
            format!("{} ({})", self.name, endpoint)
        }
    }
}

/// Settings-panel state.
pub struct SettingsPanel {
    pub is_open: bool,
    /// Open/close animation progress (0.0 = fully closed, 1.0 = fully open).
    /// Incremented every frame by the renderer.
    pub open_progress: f32,
    /// Slider currently being dragged with the mouse (`None` when no drag).
    pub drag_slider: Option<SliderDrag>,
    /// Currently selected category.
    pub category: SettingsCategory,
    /// Font size (slider value).
    pub font_size: f32,
    /// Selected color-scheme index.
    pub scheme_index: usize,
    /// Phase 3b (UI/UX v2): index of the colour-scheme dot the mouse is
    /// currently hovering inside the Theme category. `None` when not
    /// hovering. Drives the live preview — `render_frame` swaps in the
    /// hovered scheme transiently without touching `scheme_index` or
    /// the on-disk TOML, so moving the cursor away reverts cleanly and
    /// clicking commits via the existing `ThemeColor` hit handler.
    pub theme_hover_preview: Option<usize>,
    /// Window opacity.
    pub opacity: f32,
    /// Whether the panel has unsaved changes.
    pub dirty: bool,
    /// Font family name (editable).
    pub font_family: String,
    /// Whether the font-family input is focused.
    pub font_family_editing: bool,
    /// Profile list.
    pub profiles: Vec<ProfileEntry>,
    /// Selected profile index.
    pub selected_profile: usize,
    /// SSH host list (Phase 5-11-8 Step 8-1: display-only, generated from `config.hosts`).
    pub ssh_hosts: Vec<SshHostEntry>,
    /// Currently selected SSH host index (into `ssh_hosts`).
    pub selected_host_index: usize,
    /// Startup session name.
    #[allow(dead_code)]
    pub startup_session: String,
    /// Window ID whose tab name is being edited (`None` = no edit in flight).
    pub tab_rename_editing: Option<u32>,
    /// In-flight tab-rename text.
    pub tab_rename_text: String,
    /// Selected language index (position within `LANGUAGE_OPTIONS`).
    pub language_index: usize,
    /// Whether to check for updates at startup.
    pub auto_check_update: bool,
    /// Phase 2c-G: read-only mirror of the `[blocks]` section. Populated at
    /// construction time so the Blocks settings page can display the active
    /// values; interactive editing lands in a follow-up.
    pub blocks_enabled: bool,
    pub blocks_border_width_px: u8,
    pub blocks_show_exit_code_badge: bool,
    /// Cursor shape (Phase 5-11-6 #6). `block` / `beam` / `underline`.
    /// On save we write back to the top-level `cursor_style` key in the TOML.
    pub cursor_style: nexterm_config::CursorStyle,
    /// Horizontal window padding (pixels, 0–32).
    /// On save we write back to `[window].padding_x`.
    pub padding_x: u32,
    /// Vertical window padding (pixels, 0–32).
    pub padding_y: u32,
    /// GPU presentation mode (`fifo` / `mailbox` / `auto`).
    /// On save we write back to `[gpu].present_mode`.
    pub present_mode: nexterm_config::PresentModeConfig,
    /// Phase 5-11-6 #6: focused field index inside the Window category.
    /// 0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode.
    pub window_field_focus: u8,
    /// Phase 5-11-8 Step 8-2: focused field index inside the SSH category.
    /// 0=ListBox (host selection) / 1=name / 2=host / 3=port / 4=username / 5=auth_type.
    /// Range: 0..=5. Updated via AccessKit Focus or the arrow keys.
    pub ssh_field_focus: u8,
    /// Phase 5-11-8 Step 8-3 (Sub-phase A): in-flight SSH host field edit state.
    /// `Some(state)` = edit mode is on; `None` = off. Corresponds to
    /// `ssh_field_focus` values 1/2/4 (name/host/username). Enter starts the
    /// edit, Enter commits, Esc cancels. `port` / `auth_type` use separate UI
    /// (SpinButton / ComboBox) in Sub-phase C and do not flow through this option.
    pub ssh_field_editing: Option<TextInputState>,
    /// Phase 5-11-8 Step 8-3 (Sub-phase D): whether the SSH delete-confirmation
    /// dialog is open. When `true`, the `Role::AlertDialog` modal (NodeId 47) is
    /// shown; the user operates the Confirm (48) / Cancel (49) buttons. Esc
    /// acts as Cancel.
    pub ssh_delete_dialog_open: bool,
    /// Phase 5-11-8 Step 8-3 (Sub-phase D): which button has focus in the
    /// delete-confirmation dialog. `false` = Cancel (49, default; prevents
    /// accidental deletion); `true` = Confirm (48). Left/Right toggles; Enter
    /// executes.
    pub ssh_delete_dialog_confirm_focused: bool,
    /// Phase 5-11-9 Sub-phase A: key binding list (mirror of `Config.keys`).
    /// Sub-phase A loads this from the config on `new()`; Sub-phase B/C/D add
    /// edit operations and TOML write-back.
    pub keybindings: Vec<KeyBindingEntry>,
    /// Phase 5-11-9 Sub-phase A: currently selected key binding index (into `keybindings`).
    pub selected_key_index: usize,
    /// Phase 5-11-9 Sub-phase A: focused field index inside the Keybindings category.
    /// 0=ListBox (binding selection) / 1=key field / 2=action field.
    /// Sub-phase D extends this range to 0..=4 (3=Add, 4=Delete).
    pub key_field_focus: u8,
    /// Phase 5-11-9 Sub-phase B: in-flight key-string edit state.
    /// `Some(Record)` = waiting for the next physical key press to capture.
    /// `Some(Text(state))` = free-form text editing for prefix bindings.
    /// `None` = not editing.
    pub key_editing: Option<KeyEditMode>,
    /// Phase 5-11-9 Sub-phase D: delete-confirmation dialog open state.
    pub key_delete_dialog_open: bool,
    /// Phase 5-11-9 Sub-phase D: focus inside the delete-confirmation dialog.
    /// `false` = Cancel (default, accident guard) / `true` = Confirm.
    pub key_delete_dialog_confirm_focused: bool,
    /// Phase 3 (UI 4-tasks, 2026-06-12): cumulative drag-to-move offset
    /// applied to the centered panel position. `(0.0, 0.0)` means the panel
    /// renders at its default centered location. Persists for the lifetime of
    /// one open session — `close()` resets it back to centered.
    pub drag_offset: (f32, f32),
    /// Phase 3 (UI 4-tasks): the "grab anchor" for an in-flight title-bar
    /// drag. `Some((ax, ay))` while the user is holding the left mouse button
    /// after pressing inside the title bar; `None` otherwise. Stored as
    /// `cursor_at_press - drag_offset_at_press`, so the live update is just
    /// `drag_offset = cursor_now - anchor`. Cleared by `end_drag()` on button
    /// release and by `close()`.
    pub drag_anchor: Option<(f32, f32)>,
    /// Phase 4 (UI/UX v2): fuzzy search query for the category sidebar. Empty
    /// string disables filtering (default). Edited only while `search_focused`
    /// is true so panel-wide keyboard navigation keeps working.
    pub search_query: String,
    /// Phase 4 (UI/UX v2): whether the search input owns keyboard focus.
    /// Toggled by `/` (when no other edit mode is active) or by clicking the
    /// search box. Esc clears focus and the query.
    pub search_focused: bool,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        let config = nexterm_config::Config::default();
        Self::new(&config)
    }
}

impl SettingsPanel {
    pub fn new(config: &nexterm_config::Config) -> Self {
        let scheme_index = scheme_name_to_index(&config.colors);
        // Build `ProfileEntry` items from `config.profiles`.
        let profiles: Vec<ProfileEntry> = config
            .profiles
            .iter()
            .map(|p| ProfileEntry {
                name: p.name.clone(),
                icon: p.icon.clone(),
                shell_program: p
                    .shell
                    .as_ref()
                    .map(|s| s.program.clone())
                    .unwrap_or_default(),
                working_dir: p.working_dir.clone().unwrap_or_default(),
            })
            .collect();
        // Phase 5-11-8 Step 8-1: build `SshHostEntry` items from `config.hosts`.
        let ssh_hosts: Vec<SshHostEntry> = config
            .hosts
            .iter()
            .map(|h| SshHostEntry {
                name: h.name.clone(),
                host: h.host.clone(),
                port: h.port,
                username: h.username.clone(),
                auth_type: h.auth_type.clone(),
            })
            .collect();
        // Phase 5-11-9 Sub-phase A: build `KeyBindingEntry` items from `config.keys`.
        let keybindings: Vec<KeyBindingEntry> = config
            .keys
            .iter()
            .map(|k| KeyBindingEntry {
                key: k.key.clone(),
                action: k.action.clone(),
            })
            .collect();
        let language_index = LANGUAGE_OPTIONS
            .iter()
            .position(|(_, code)| *code == config.language.as_str())
            .unwrap_or(0);
        Self {
            is_open: false,
            open_progress: 0.0,
            drag_slider: None,
            category: SettingsCategory::Font,
            font_size: config.font.size,
            scheme_index,
            theme_hover_preview: None,
            opacity: config.window.background_opacity,
            dirty: false,
            font_family: config.font.family.clone(),
            font_family_editing: false,
            profiles,
            selected_profile: 0,
            ssh_hosts,
            selected_host_index: 0,
            ssh_field_focus: 0,
            ssh_field_editing: None,
            ssh_delete_dialog_open: false,
            ssh_delete_dialog_confirm_focused: false,
            keybindings,
            selected_key_index: 0,
            key_field_focus: 0,
            key_editing: None,
            key_delete_dialog_open: false,
            key_delete_dialog_confirm_focused: false,
            startup_session: "main".to_string(),
            tab_rename_editing: None,
            tab_rename_text: String::new(),
            language_index,
            auto_check_update: config.auto_check_update,
            blocks_enabled: config.blocks.enabled,
            blocks_border_width_px: config.blocks.border_width_px,
            blocks_show_exit_code_badge: config.blocks.show_exit_code_badge,
            cursor_style: config.cursor_style.clone(),
            // `padding_x` / `padding_y` are `u32` in the config but the UI
            // clamps them to 0..=32.
            padding_x: config.window.padding_x.min(32),
            padding_y: config.window.padding_y.min(32),
            present_mode: config.gpu.present_mode.clone(),
            window_field_focus: 0,
            // Phase 3 (UI 4-tasks): panel renders centered on first open.
            drag_offset: (0.0, 0.0),
            drag_anchor: None,
            // Phase 4 (UI/UX v2): start with no filter and search defocused.
            search_query: String::new(),
            search_focused: false,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        // Start the animation from 0 to replay the open transition.
        self.open_progress = 0.0;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.open_progress = 0.0;
        self.drag_slider = None;
        self.dirty = false;
        self.font_family_editing = false;
        self.tab_rename_editing = None;
        // Phase 3b (UI/UX v2): drop any in-flight theme preview so the
        // next panel open starts on the configured scheme.
        self.theme_hover_preview = None;
        // Phase 5-11-8 Step 8-3 (Sub-phase A): also leave SSH field-edit mode.
        self.ssh_field_editing = None;
        // Phase 5-11-8 Step 8-3 (Sub-phase D): also close the delete dialog.
        self.ssh_delete_dialog_open = false;
        self.ssh_delete_dialog_confirm_focused = false;
        // Phase 5-11-9 Sub-phase B: also leave key-field edit mode.
        self.key_editing = None;
        // Phase 5-11-9 Sub-phase D: also close the delete dialog.
        self.key_delete_dialog_open = false;
        self.key_delete_dialog_confirm_focused = false;
        // Phase 3 (UI 4-tasks): re-center the panel on the next open. Anchor
        // is also cleared so a stale drag from a pre-close press cannot
        // resume against the next opened panel.
        self.drag_offset = (0.0, 0.0);
        self.drag_anchor = None;
    }

    /// Phase 3 (UI 4-tasks): start a title-bar drag.
    ///
    /// Records the grab anchor so subsequent `update_drag` calls can compute
    /// the new offset as a simple subtraction. Called from
    /// `on_mouse_left_pressed` when the hit-test reports `TitleBar`.
    pub fn start_drag(&mut self, cursor_x: f32, cursor_y: f32) {
        self.drag_anchor = Some((cursor_x - self.drag_offset.0, cursor_y - self.drag_offset.1));
    }

    /// Phase 3 (UI 4-tasks): update the live drag offset.
    ///
    /// No-op when no drag is in flight, so it is safe to call on every cursor
    /// movement.
    pub fn update_drag(&mut self, cursor_x: f32, cursor_y: f32) {
        if let Some((ax, ay)) = self.drag_anchor {
            self.drag_offset = (cursor_x - ax, cursor_y - ay);
        }
    }

    /// Phase 3 (UI 4-tasks): end a title-bar drag.
    ///
    /// Called from `on_mouse_left_released` regardless of where the cursor
    /// ended up — the final `drag_offset` is whatever the latest
    /// `update_drag` recorded.
    pub fn end_drag(&mut self) {
        self.drag_anchor = None;
    }

    /// Phase 3 (UI 4-tasks): is a title-bar drag currently in flight?
    pub fn is_dragging(&self) -> bool {
        self.drag_anchor.is_some()
    }

    /// Set the font size from a slider X coordinate (used by mouse clicks/drags).
    pub fn set_font_size_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        // Font size range: 8.0..=32.0 (a 24-wide range, snapped to 0.5 steps).
        let raw = 8.0 + ratio * 24.0;
        self.font_size = (raw * 2.0).round() / 2.0;
        self.dirty = true;
    }

    /// Set the opacity from a slider X coordinate (used by mouse clicks/drags).
    pub fn set_opacity_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        // Opacity range: 0.1..=1.0 (snapped to 5% steps).
        let raw = 0.1 + ratio * 0.9;
        self.opacity = (raw * 20.0).round() / 20.0;
        self.dirty = true;
    }

    /// Phase 5-11-6 #6: set `padding_x` (0–32 px) from a slider X coordinate.
    pub fn set_padding_x_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        self.padding_x = (ratio * 32.0).round() as u32;
        self.dirty = true;
    }

    /// Phase 5-11-6 #6: set `padding_y` (0–32 px) from a slider X coordinate.
    pub fn set_padding_y_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        self.padding_y = (ratio * 32.0).round() as u32;
        self.dirty = true;
    }

    /// Ease-out cubic: smooth deceleration via `1 - (1-t)^3`.
    pub fn eased_progress(&self) -> f32 {
        let t = self.open_progress.clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }

    /// Move to the previous category in the sidebar.
    pub fn prev_category(&mut self) {
        let idx = Self::category_index(&self.category);
        let len = SettingsCategory::ALL.len();
        self.category = SettingsCategory::ALL[(idx + len - 1) % len].clone();
    }

    /// Move to the next category in the sidebar.
    pub fn next_category(&mut self) {
        let idx = Self::category_index(&self.category);
        self.category = SettingsCategory::ALL[(idx + 1) % SettingsCategory::ALL.len()].clone();
    }

    fn category_index(cat: &SettingsCategory) -> usize {
        SettingsCategory::ALL
            .iter()
            .position(|c| c == cat)
            .unwrap_or(0)
    }

    /// Backward-compat alias for setting the category by tab index (old API).
    #[allow(dead_code)]
    pub fn next_tab(&mut self) {
        self.next_category();
    }

    #[allow(dead_code)]
    pub fn prev_tab(&mut self) {
        self.prev_category();
    }

    /// Append a character to the font-family input field.
    pub fn push_font_family_char(&mut self, ch: char) {
        if self.font_family_editing {
            self.font_family.push(ch);
            self.dirty = true;
        }
    }

    /// Pop the trailing character from the font-family input field.
    pub fn pop_font_family_char(&mut self) {
        if self.font_family_editing {
            self.font_family.pop();
            self.dirty = true;
        }
    }

    pub fn increase_font_size(&mut self) {
        self.font_size = (self.font_size + 0.5).min(32.0);
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn decrease_font_size(&mut self) {
        self.font_size = (self.font_size - 0.5).max(8.0);
        self.dirty = true;
    }

    pub fn next_scheme(&mut self) {
        self.scheme_index = (self.scheme_index + 1) % 9;
        self.dirty = true;
    }

    pub fn prev_scheme(&mut self) {
        self.scheme_index = if self.scheme_index == 0 {
            8
        } else {
            self.scheme_index - 1
        };
        self.dirty = true;
    }

    pub fn increase_opacity(&mut self) {
        self.opacity = (self.opacity + 0.05).min(1.0);
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn decrease_opacity(&mut self) {
        self.opacity = (self.opacity - 0.05).max(0.1);
        self.dirty = true;
    }

    /// Used by SR via `Action::SetValue(NumericValue)`: clamp the f64 value to
    /// `8.0..=32.0`, snap to 0.5 steps, and store it as the font size.
    ///
    /// The mouse-drag path (`set_font_size_from_slider`) takes a pixel X
    /// coordinate instead of a direct value, but the rounding and clamp ranges
    /// are identical.
    pub fn set_font_size_value(&mut self, v: f64) {
        let raw = (v as f32).clamp(8.0, 32.0);
        self.font_size = (raw * 2.0).round() / 2.0;
        self.dirty = true;
    }

    /// Used by SR via `Action::SetValue(NumericValue)`: clamp the f64 value to
    /// `0.1..=1.0`, snap to 0.05 steps, and store it as the opacity.
    pub fn set_opacity_value(&mut self, v: f64) {
        let raw = (v as f32).clamp(0.1, 1.0);
        self.opacity = (raw * 20.0).round() / 20.0;
        self.dirty = true;
    }

    /// Used by SR via `Action::Click`: toggle the "check for updates at startup" box.
    pub fn toggle_auto_check_update(&mut self) {
        self.auto_check_update = !self.auto_check_update;
        self.dirty = true;
    }

    // ===== Phase 5-11-6 #6: cursor style =====
    //
    // Cycles through Block / Beam / Underline.
    // On save the TOML stores `cursor_style = "block" | "beam" | "underline"`.

    pub fn next_cursor_style(&mut self) {
        use nexterm_config::CursorStyle::*;
        self.cursor_style = match self.cursor_style {
            Block => Beam,
            Beam => Underline,
            Underline => Block,
        };
        self.dirty = true;
    }

    pub fn prev_cursor_style(&mut self) {
        use nexterm_config::CursorStyle::*;
        self.cursor_style = match self.cursor_style {
            Block => Underline,
            Beam => Block,
            Underline => Beam,
        };
        self.dirty = true;
    }

    /// Enumeration index (0=Block, 1=Beam, 2=Underline). Used for UI drawing
    /// and the AccessKit `Action::SetValue` path (currently only via tests).
    #[allow(dead_code)]
    pub fn cursor_style_index(&self) -> usize {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => 0,
            Beam => 1,
            Underline => 2,
        }
    }

    /// UI display label.
    pub fn cursor_style_label(&self) -> &'static str {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => "Block",
            Beam => "Beam",
            Underline => "Underline",
        }
    }

    /// Lowercase TOML key for write-back (matches `serde`'s `rename_all = "lowercase"`).
    pub fn cursor_style_toml_key(&self) -> &'static str {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => "block",
            Beam => "beam",
            Underline => "underline",
        }
    }

    // ===== Phase 5-11-6 #6: window padding =====
    //
    // 0–32 pixels; adjustable in 1-pixel steps. The SR
    // `Action::SetValue(NumericValue)` path rounds the f64 to u32 and clamps.

    pub fn set_padding_x_value(&mut self, v: f64) {
        self.padding_x = (v.round().clamp(0.0, 32.0)) as u32;
        self.dirty = true;
    }

    pub fn increase_padding_x(&mut self) {
        self.padding_x = (self.padding_x + 1).min(32);
        self.dirty = true;
    }

    pub fn decrease_padding_x(&mut self) {
        self.padding_x = self.padding_x.saturating_sub(1);
        self.dirty = true;
    }

    pub fn set_padding_y_value(&mut self, v: f64) {
        self.padding_y = (v.round().clamp(0.0, 32.0)) as u32;
        self.dirty = true;
    }

    pub fn increase_padding_y(&mut self) {
        self.padding_y = (self.padding_y + 1).min(32);
        self.dirty = true;
    }

    pub fn decrease_padding_y(&mut self) {
        self.padding_y = self.padding_y.saturating_sub(1);
        self.dirty = true;
    }

    // ===== Phase 5-11-6 #6: presentation mode =====
    //
    // Cycles through Fifo / Mailbox / Auto.
    // On save the TOML stores `[gpu].present_mode`.

    pub fn next_present_mode(&mut self) {
        use nexterm_config::PresentModeConfig::*;
        self.present_mode = match self.present_mode {
            Fifo => Mailbox,
            Mailbox => Auto,
            Auto => Fifo,
        };
        self.dirty = true;
    }

    pub fn prev_present_mode(&mut self) {
        use nexterm_config::PresentModeConfig::*;
        self.present_mode = match self.present_mode {
            Fifo => Auto,
            Mailbox => Fifo,
            Auto => Mailbox,
        };
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn present_mode_index(&self) -> usize {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => 0,
            Mailbox => 1,
            Auto => 2,
        }
    }

    pub fn present_mode_label(&self) -> &'static str {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => "Fifo (VSync / high compatibility)",
            Mailbox => "Mailbox (low latency / recommended)",
            Auto => "Auto (environment-dependent)",
        }
    }

    pub fn present_mode_toml_key(&self) -> &'static str {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => "fifo",
            Mailbox => "mailbox",
            Auto => "auto",
        }
    }

    // ===== Phase 5-11-6 #6: focused field inside the Window category =====
    //
    // 0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode.
    // Up/Down moves between fields; Left/Right changes the focused field's value.

    /// Total number of fields in the Window category.
    pub const WINDOW_FIELD_COUNT: u8 = 5;

    /// Move focus to the next field (stops at the last one).
    /// Returns `true` if focus moved; `false` if already on the last field
    /// (used by the category-navigation fallback).
    pub fn next_window_field(&mut self) -> bool {
        if self.window_field_focus + 1 < Self::WINDOW_FIELD_COUNT {
            self.window_field_focus += 1;
            true
        } else {
            false
        }
    }

    /// Move focus to the previous field (stops at the first one).
    pub fn prev_window_field(&mut self) -> bool {
        if self.window_field_focus > 0 {
            self.window_field_focus -= 1;
            true
        } else {
            false
        }
    }

    /// Increment the focused field's value (Right arrow, or the Up arrow's
    /// fallback inside the Window category).
    pub fn window_field_increase(&mut self) {
        match self.window_field_focus {
            0 => self.increase_opacity(),
            1 => self.next_cursor_style(),
            2 => self.increase_padding_x(),
            3 => self.increase_padding_y(),
            4 => self.next_present_mode(),
            _ => {}
        }
    }

    /// Decrement the focused field's value.
    pub fn window_field_decrease(&mut self) {
        match self.window_field_focus {
            0 => self.decrease_opacity(),
            1 => self.prev_cursor_style(),
            2 => self.decrease_padding_x(),
            3 => self.decrease_padding_y(),
            4 => self.prev_present_mode(),
            _ => {}
        }
    }

    /// Return the scheme name for the current `scheme_index`.
    pub fn scheme_name(&self) -> &str {
        const SCHEMES: [&str; 9] = [
            "dark",
            "light",
            "tokyonight",
            "solarized",
            "gruvbox",
            "catppuccin",
            "dracula",
            "nord",
            "onedark",
        ];
        SCHEMES[self.scheme_index % 9]
    }

    /// Return the currently selected language code.
    pub fn language_code(&self) -> &str {
        LANGUAGE_OPTIONS
            .get(self.language_index)
            .map(|(_, code)| *code)
            .unwrap_or("auto")
    }

    /// Switch to the next language.
    pub fn next_language(&mut self) {
        self.language_index = (self.language_index + 1) % LANGUAGE_OPTIONS.len();
        self.dirty = true;
    }

    /// Switch to the previous language.
    pub fn prev_language(&mut self) {
        let len = LANGUAGE_OPTIONS.len();
        self.language_index = (self.language_index + len - 1) % len;
        self.dirty = true;
    }

    // ===== Phase 5-11-8 Step 8-2: SSH host field editing =====
    //
    // Edits the 5 fields of the currently-selected host
    // (`ssh_hosts[selected_host_index]`). Supports both the AccessKit
    // `Action::SetValue` path (TextInput / SpinButton) and the
    // `Action::Click` path (ComboBox cycling). Every change sets `dirty = true`.

    /// Allowed auth_type values (matches the `HostConfig` serde spec).
    pub const SSH_AUTH_TYPES: &'static [&'static str] = &["password", "key", "agent"];

    /// Return a mutable reference to the currently-selected host (if any).
    fn selected_ssh_host_mut(&mut self) -> Option<&mut SshHostEntry> {
        self.ssh_hosts.get_mut(self.selected_host_index)
    }

    /// Update the `name` field (TextInput SetValue path).
    pub fn set_ssh_host_name(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.name = text;
            self.dirty = true;
        }
    }

    /// Update the `host` field (TextInput SetValue path).
    pub fn set_ssh_host_host(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.host = text;
            self.dirty = true;
        }
    }

    /// Update the `username` field (TextInput SetValue path).
    pub fn set_ssh_host_username(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.username = text;
            self.dirty = true;
        }
    }

    /// Update the `port` field (SpinButton SetValue path).
    /// Clamps f64 to u16 (1..=65535).
    pub fn set_ssh_host_port_value(&mut self, v: f64) {
        let clamped = v.round().clamp(1.0, 65535.0) as u16;
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = clamped;
            self.dirty = true;
        }
    }

    /// Increment `port` by 1 (SpinButton Increment path; saturates at 65535).
    /// `u16::saturating_add` saturates at 65535 automatically, so `.min()` is unnecessary.
    pub fn increase_ssh_host_port(&mut self) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = host.port.saturating_add(1);
            self.dirty = true;
        }
    }

    /// Decrement `port` by 1 (SpinButton Decrement path; saturates at 1).
    pub fn decrease_ssh_host_port(&mut self) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = host.port.saturating_sub(1).max(1);
            self.dirty = true;
        }
    }

    /// Advance `auth_type` to the next value (ComboBox Click / Increment path).
    /// Cycles through `SSH_AUTH_TYPES`. If the current value is unknown, resets
    /// to the first entry.
    pub fn next_ssh_auth_type(&mut self) {
        let types = Self::SSH_AUTH_TYPES;
        if let Some(host) = self.selected_ssh_host_mut() {
            let current = types.iter().position(|&t| t == host.auth_type).unwrap_or(0);
            host.auth_type = types[(current + 1) % types.len()].to_string();
            self.dirty = true;
        }
    }

    /// Move `auth_type` to the previous value (ComboBox Decrement path).
    pub fn prev_ssh_auth_type(&mut self) {
        let types = Self::SSH_AUTH_TYPES;
        if let Some(host) = self.selected_ssh_host_mut() {
            let current = types.iter().position(|&t| t == host.auth_type).unwrap_or(0);
            host.auth_type = types[(current + types.len() - 1) % types.len()].to_string();
            self.dirty = true;
        }
    }

    // ===== Phase 5-11-8 Step 8-3 (Sub-phase D): Add / Delete + delete-confirmation dialog =====
    //
    // - `add_ssh_host`: append a host with all-empty strings + port=22 +
    //   auth_type="password", move the selection to the new entry, and
    //   immediately enter edit mode on the name field (field_id=1).
    // - `open_ssh_delete_dialog`: open the delete-confirmation dialog. The
    //   default focus is on the Cancel button (prevents accidental deletion).
    // - `cancel_ssh_delete_dialog`: close the dialog without deleting.
    // - `confirm_ssh_delete_dialog`: delete the selected host and close the
    //   dialog. The post-deletion selection clamps to n (list shifts up; uses
    //   n-1 when the deleted index was the last entry).

    /// Append a new SSH host and start editing it (the Add button path).
    ///
    /// Default values: `name=""`, `host=""`, `port=22`, `username=""`,
    /// `auth_type="password"`. After appending, the selection moves to
    /// `selected_host_index = ssh_hosts.len() - 1`, `ssh_field_focus` becomes
    /// 1 (name), and `begin_ssh_field_edit()` is called so SR users can start
    /// typing the name immediately.
    pub fn add_ssh_host(&mut self) {
        let new_host = SshHostEntry {
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            auth_type: "password".to_string(),
        };
        self.ssh_hosts.push(new_host);
        self.selected_host_index = self.ssh_hosts.len() - 1;
        self.ssh_field_focus = 1;
        // Immediately enter edit mode on the name field.
        self.ssh_field_editing = Some(TextInputState::new(String::new()));
        self.dirty = true;
    }

    /// Open the delete-confirmation dialog (the Delete button path).
    ///
    /// No-op when the host list is empty (treated as disabled). The default
    /// focus is on the Cancel button — the standard UX guard against
    /// accidental deletions.
    pub fn open_ssh_delete_dialog(&mut self) {
        if self.ssh_hosts.is_empty() {
            return;
        }
        self.ssh_delete_dialog_open = true;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// Close the delete-confirmation dialog (the Cancel button or Esc path).
    /// Leaves the host unchanged.
    pub fn cancel_ssh_delete_dialog(&mut self) {
        self.ssh_delete_dialog_open = false;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// Confirm "delete" in the delete-confirmation dialog (Confirm button or Enter).
    ///
    /// Deletes the selected host and closes the dialog. Post-deletion selection
    /// clamps to n:
    /// - With `selected_host_index = n` before the delete and `ssh_hosts.len() = L`,
    ///   the new upper bound is `L - 1`; clamp to 0 otherwise.
    /// - When `n` was the last entry, the new selection is `n - 1`.
    /// - When the list becomes empty, reset `selected_host_index = 0` and
    ///   `ssh_field_focus = 0`.
    pub fn confirm_ssh_delete_dialog(&mut self) {
        if self.selected_host_index < self.ssh_hosts.len() {
            self.ssh_hosts.remove(self.selected_host_index);
            // n clamp: when the deleted index was the tail, fall back to n-1.
            if !self.ssh_hosts.is_empty() && self.selected_host_index >= self.ssh_hosts.len() {
                self.selected_host_index = self.ssh_hosts.len() - 1;
            }
            // When the list is empty, return focus to the ListBox.
            if self.ssh_hosts.is_empty() {
                self.selected_host_index = 0;
                self.ssh_field_focus = 0;
            }
            self.dirty = true;
        }
        self.ssh_delete_dialog_open = false;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// Toggle focus in the delete-confirmation dialog (Confirm ↔ Cancel)
    /// via Left/Right.
    pub fn toggle_ssh_delete_dialog_focus(&mut self) {
        self.ssh_delete_dialog_confirm_focused = !self.ssh_delete_dialog_confirm_focused;
    }

    // ===== Phase 5-11-8 Step 8-3 (Sub-phase A): SSH field inline editing =====
    //
    // When `ssh_field_focus` is 1 (name), 2 (host), or 4 (username), pressing
    // Enter starts edit mode and initialises the buffer with
    // `ssh_field_editing = Some(TextInputState::new(current))`. Enter again
    // writes back via `set_ssh_host_*`.

    /// Start TextInput edit mode for the current `ssh_field_focus` value.
    ///
    /// Returns `true` if edit mode actually started; `false` when the field
    /// is not a TextInput (port / auth_type / ListBox) or no host is selected.
    pub fn begin_ssh_field_edit(&mut self) -> bool {
        let initial = {
            let Some(host) = self.ssh_hosts.get(self.selected_host_index) else {
                return false;
            };
            match self.ssh_field_focus {
                1 => host.name.clone(),
                2 => host.host.clone(),
                4 => host.username.clone(),
                _ => return false,
            }
        };
        self.ssh_field_editing = Some(TextInputState::new(initial));
        true
    }

    /// Commit the in-flight buffer back to the host field and leave edit mode.
    /// Returns `true` when a write-back happened.
    pub fn commit_ssh_field_edit(&mut self) -> bool {
        let Some(state) = self.ssh_field_editing.take() else {
            return false;
        };
        let text = state.buffer;
        match self.ssh_field_focus {
            1 => self.set_ssh_host_name(text),
            2 => self.set_ssh_host_host(text),
            4 => self.set_ssh_host_username(text),
            _ => return false,
        }
        true
    }

    /// Discard the in-flight buffer and leave edit mode.
    /// Returns `true` if edit mode was active.
    pub fn cancel_ssh_field_edit(&mut self) -> bool {
        self.ssh_field_editing.take().is_some()
    }

    /// Insert one character at the cursor inside the in-flight buffer.
    /// No-op when not in edit mode.
    pub fn ssh_field_insert_char(&mut self, ch: char) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.insert_char(ch);
        }
    }

    /// Insert a string at the cursor inside the in-flight buffer (IME Commit path).
    pub fn ssh_field_insert_str(&mut self, s: &str) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.insert_str(s);
        }
    }

    /// Delete the character immediately before the cursor (Backspace).
    pub fn ssh_field_backspace(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.backspace();
        }
    }

    /// Delete the character immediately after the cursor (Delete).
    pub fn ssh_field_delete(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.delete_forward();
        }
    }

    /// Move the in-flight cursor one character left.
    pub fn ssh_field_move_left(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_left();
        }
    }

    /// Move the in-flight cursor one character right.
    pub fn ssh_field_move_right(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_right();
        }
    }

    /// Move the in-flight cursor to the start of the buffer.
    pub fn ssh_field_move_home(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_home();
        }
    }

    /// Move the in-flight cursor to the end of the buffer.
    pub fn ssh_field_move_end(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_end();
        }
    }

    // ===== Phase 5-11-9 Sub-phase B: key-binding key-field editing =====
    //
    // The action field (key_field_focus == 2) is handled separately by
    // Sub-phase C (ComboBox). Sub-phase B owns only the key field
    // (key_field_focus == 1).

    /// Start Record mode for the currently selected binding's key field.
    /// Returns `true` when edit mode actually started; `false` if no binding
    /// is selected.
    pub fn begin_key_record(&mut self) -> bool {
        if self.keybindings.is_empty() {
            return false;
        }
        if self.selected_key_index >= self.keybindings.len() {
            return false;
        }
        self.key_editing = Some(KeyEditMode::Record);
        true
    }

    /// Start Text edit mode initialised with the current key string.
    /// Used to enter Text mode directly without going through Record first.
    /// Currently exercised by tests; the in-UI entry path is Enter → Record → Tab.
    #[allow(dead_code)]
    pub fn begin_key_text_edit(&mut self) -> bool {
        if self.keybindings.is_empty() {
            return false;
        }
        let Some(kb) = self.keybindings.get(self.selected_key_index) else {
            return false;
        };
        self.key_editing = Some(KeyEditMode::Text(TextInputState::new(kb.key.clone())));
        true
    }

    /// Toggle between Record and Text mode. No-op if not editing.
    /// On Record → Text, the current binding's key value seeds the buffer.
    /// On Text → Record, the in-flight buffer is discarded.
    pub fn toggle_key_edit_mode(&mut self) {
        match self.key_editing.take() {
            Some(KeyEditMode::Record) => {
                let initial = self
                    .keybindings
                    .get(self.selected_key_index)
                    .map(|kb| kb.key.clone())
                    .unwrap_or_default();
                self.key_editing = Some(KeyEditMode::Text(TextInputState::new(initial)));
            }
            Some(KeyEditMode::Text(_)) => {
                self.key_editing = Some(KeyEditMode::Record);
            }
            None => {}
        }
    }

    /// In Record mode, set the binding's key to `formatted` (e.g. the result
    /// of `format_key_event`) and leave edit mode. No-op outside Record mode.
    /// Returns `true` when the binding was updated.
    pub fn capture_key_record(&mut self, formatted: String) -> bool {
        if !matches!(self.key_editing, Some(KeyEditMode::Record)) {
            return false;
        }
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            self.key_editing = None;
            return false;
        };
        kb.key = formatted;
        self.key_editing = None;
        self.dirty = true;
        true
    }

    /// Commit the in-flight Text buffer back to the selected binding's key.
    /// Returns `true` when a write-back happened. Record mode is a no-op
    /// here (Record commits on capture).
    pub fn commit_key_edit(&mut self) -> bool {
        let Some(KeyEditMode::Text(state)) = self.key_editing.take() else {
            return false;
        };
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        kb.key = state.buffer;
        self.dirty = true;
        true
    }

    /// Discard any in-flight buffer and leave edit mode.
    /// Returns `true` if edit mode was active.
    pub fn cancel_key_edit(&mut self) -> bool {
        self.key_editing.take().is_some()
    }

    /// Insert a single character into the in-flight Text buffer.
    /// No-op in Record mode or when not editing.
    pub fn key_field_insert_char(&mut self, ch: char) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.insert_char(ch);
        }
    }

    /// Insert a string into the in-flight Text buffer (IME Commit path).
    pub fn key_field_insert_str(&mut self, s: &str) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.insert_str(s);
        }
    }

    /// Backspace in the in-flight Text buffer.
    pub fn key_field_backspace(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.backspace();
        }
    }

    /// Forward-delete in the in-flight Text buffer.
    pub fn key_field_delete(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.delete_forward();
        }
    }

    /// Move cursor left by one character in the in-flight Text buffer.
    pub fn key_field_move_left(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.move_left();
        }
    }

    /// Move cursor right by one character in the in-flight Text buffer.
    pub fn key_field_move_right(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.move_right();
        }
    }

    /// Move cursor to the start of the in-flight Text buffer.
    pub fn key_field_move_home(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.move_home();
        }
    }

    /// Move cursor to the end of the in-flight Text buffer.
    pub fn key_field_move_end(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.move_end();
        }
    }

    // ===== Phase 5-11-9 Sub-phase C: Action ComboBox =====
    //
    // The action field (`key_field_focus == 2`) cycles through `KEYBINDING_ACTIONS`
    // via ← / → in the input handler. Unknown values (e.g. user-edited TOML
    // typos) are normalised to the first known action on the first cycle.

    /// Cycle the selected binding's `action` to the next entry in
    /// `KEYBINDING_ACTIONS`. Returns `true` when the value was updated.
    /// No-op when no binding is selected.
    pub fn next_key_action(&mut self) -> bool {
        let actions = KEYBINDING_ACTIONS;
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        let current = actions.iter().position(|&a| a == kb.action);
        let next_index = match current {
            Some(i) => (i + 1) % actions.len(),
            // Unknown action: snap to the first known entry rather than
            // staying silently invalid.
            None => 0,
        };
        kb.action = actions[next_index].to_string();
        self.dirty = true;
        true
    }

    /// Cycle the selected binding's `action` to the previous entry in
    /// `KEYBINDING_ACTIONS`. Returns `true` when the value was updated.
    /// Unknown values snap to the last known action.
    pub fn prev_key_action(&mut self) -> bool {
        let actions = KEYBINDING_ACTIONS;
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        let prev_index = match actions.iter().position(|&a| a == kb.action) {
            Some(i) => (i + actions.len() - 1) % actions.len(),
            // Unknown action: snap to the last known entry.
            None => actions.len() - 1,
        };
        kb.action = actions[prev_index].to_string();
        self.dirty = true;
        true
    }

    /// Returns `true` when the selected binding's action is in `KEYBINDING_ACTIONS`.
    /// Used by the renderer / SR layer to flag invalid entries.
    pub fn selected_key_action_is_valid(&self) -> bool {
        let Some(kb) = self.keybindings.get(self.selected_key_index) else {
            return false;
        };
        KEYBINDING_ACTIONS.contains(&kb.action.as_str())
    }

    // ===== Phase 5-11-9 Sub-phase D: Add / Delete + delete-confirmation dialog =====
    //
    // Mirrors the SSH host Sub-phase D (Phase 5-11-8 Step 8-3) pattern:
    // - `add_key_binding`: append a fresh entry, move the selection to it,
    //   focus the key field (`key_field_focus = 1`), and enter Record mode
    //   so SR users can press a key immediately.
    // - `open_key_delete_dialog`: open the confirmation dialog with Cancel
    //   focused by default (accident guard).
    // - `cancel_key_delete_dialog`: close without deleting.
    // - `confirm_key_delete_dialog`: delete + clamp the selection.
    // - `toggle_key_delete_dialog_focus`: swap Confirm ↔ Cancel.

    /// Append a fresh key binding with safe defaults and start Record-mode
    /// editing on the key field.
    pub fn add_key_binding(&mut self) {
        let new_binding = KeyBindingEntry {
            key: String::new(),
            action: KEYBINDING_ACTIONS[0].to_string(),
        };
        self.keybindings.push(new_binding);
        self.selected_key_index = self.keybindings.len() - 1;
        self.key_field_focus = 1;
        // Immediately enter Record mode — the next key press becomes the binding.
        self.key_editing = Some(KeyEditMode::Record);
        self.dirty = true;
    }

    /// Open the delete-confirmation dialog. No-op when the list is empty
    /// (treated as disabled). Default focus is Cancel.
    pub fn open_key_delete_dialog(&mut self) {
        if self.keybindings.is_empty() {
            return;
        }
        self.key_delete_dialog_open = true;
        self.key_delete_dialog_confirm_focused = false;
    }

    /// Close the delete-confirmation dialog without deleting.
    pub fn cancel_key_delete_dialog(&mut self) {
        self.key_delete_dialog_open = false;
        self.key_delete_dialog_confirm_focused = false;
    }

    /// Delete the selected binding and close the dialog.
    ///
    /// Selection clamp: if the deleted index was the tail, fall back to n-1.
    /// When the list becomes empty, reset focus to the ListBox (`key_field_focus = 0`).
    pub fn confirm_key_delete_dialog(&mut self) {
        if self.selected_key_index < self.keybindings.len() {
            self.keybindings.remove(self.selected_key_index);
            if !self.keybindings.is_empty() && self.selected_key_index >= self.keybindings.len() {
                self.selected_key_index = self.keybindings.len() - 1;
            }
            if self.keybindings.is_empty() {
                self.selected_key_index = 0;
                self.key_field_focus = 0;
            }
            self.dirty = true;
        }
        self.key_delete_dialog_open = false;
        self.key_delete_dialog_confirm_focused = false;
    }

    /// Toggle focus in the delete-confirmation dialog (Confirm ↔ Cancel).
    pub fn toggle_key_delete_dialog_focus(&mut self) {
        self.key_delete_dialog_confirm_focused = !self.key_delete_dialog_confirm_focused;
    }

    /// Convenience predicate: returns `true` when the key field is in Record mode.
    pub fn is_key_recording(&self) -> bool {
        matches!(self.key_editing, Some(KeyEditMode::Record))
    }

    /// Phase 5-11-9 Sub-phase E: directly overwrite the selected binding's key
    /// string. Used by the AccessKit `Action::SetValue` path so screen-reader
    /// users can write a key spelling like `"ctrl+b d"` without entering
    /// Record/Text mode. Cancels any in-flight edit mode. Returns `true` when
    /// the binding was updated.
    pub fn set_keybinding_key_direct(&mut self, value: String) -> bool {
        if self.keybindings.is_empty() {
            return false;
        }
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        kb.key = value;
        self.key_editing = None;
        self.dirty = true;
        true
    }

    /// Phase 5-11-9 Sub-phase E: directly overwrite the selected binding's
    /// action string. Used by the AccessKit `Action::SetValue` path on the
    /// Action ComboBox. The caller is expected to pass a string that appears in
    /// `KEYBINDING_ACTIONS`; values outside that list are accepted but flagged
    /// as a no-op by returning `false`.
    pub fn set_keybinding_action_direct(&mut self, value: &str) -> bool {
        if !KEYBINDING_ACTIONS.contains(&value) {
            return false;
        }
        if self.keybindings.is_empty() {
            return false;
        }
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        kb.action = value.to_string();
        self.dirty = true;
        true
    }

    /// Convenience predicate: returns `true` when the key field is in Text mode.
    pub fn is_key_text_editing(&self) -> bool {
        matches!(self.key_editing, Some(KeyEditMode::Text(_)))
    }

    /// Begin a tab-rename operation.
    pub fn begin_tab_rename(&mut self, window_id: u32, current_name: &str) {
        self.tab_rename_editing = Some(window_id);
        self.tab_rename_text = current_name.to_string();
    }

    /// Cancel an in-flight tab rename.
    pub fn cancel_tab_rename(&mut self) {
        self.tab_rename_editing = None;
        self.tab_rename_text.clear();
    }

    /// Append a character while editing the tab name.
    pub fn push_tab_rename_char(&mut self, ch: char) {
        if self.tab_rename_editing.is_some() {
            self.tab_rename_text.push(ch);
        }
    }

    /// Pop the trailing character while editing the tab name.
    pub fn pop_tab_rename_char(&mut self) {
        if self.tab_rename_editing.is_some() {
            self.tab_rename_text.pop();
        }
    }

    /// Save the current settings to `nexterm.toml`.
    pub fn save_to_toml(&self) -> Result<()> {
        let path = toml_path();

        // Read the existing TOML (start from an empty string if missing).
        let existing = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };

        let mut doc: toml_edit::DocumentMut = existing.parse().unwrap_or_default();

        // [font].family
        if !self.font_family.is_empty() {
            doc["font"]["family"] = toml_edit::value(self.font_family.as_str());
        }

        // [font].size
        doc["font"]["size"] = toml_edit::value(self.font_size as f64);

        // [colors].scheme
        doc["colors"]["scheme"] = toml_edit::value(self.scheme_name());

        // [window].background_opacity
        doc["window"]["background_opacity"] = toml_edit::value(self.opacity as f64);

        // [window].padding_x / padding_y (Phase 5-11-6 #6).
        doc["window"]["padding_x"] = toml_edit::value(self.padding_x as i64);
        doc["window"]["padding_y"] = toml_edit::value(self.padding_y as i64);

        // [gpu].present_mode (Phase 5-11-6 #6).
        doc["gpu"]["present_mode"] = toml_edit::value(self.present_mode_toml_key());

        // cursor_style (Phase 5-11-6 #6).
        doc["cursor_style"] = toml_edit::value(self.cursor_style_toml_key());

        // language
        doc["language"] = toml_edit::value(self.language_code());

        // auto_check_update
        doc["auto_check_update"] = toml_edit::value(self.auto_check_update);

        // [blocks].enabled / border_width_px / show_exit_code_badge (Phase 2c-G follow-up).
        doc["blocks"]["enabled"] = toml_edit::value(self.blocks_enabled);
        doc["blocks"]["border_width_px"] = toml_edit::value(self.blocks_border_width_px as i64);
        doc["blocks"]["show_exit_code_badge"] = toml_edit::value(self.blocks_show_exit_code_badge);

        // Phase 5-11-8 Step 8-2: in-place write-back to `[[hosts]]`.
        //
        // When the existing `ArrayOfTables` is present we update only the
        // managed fields per index, preserving unmanaged fields such as
        // `key_path` / `forward_local` / `proxy_jump`. When the array length
        // diverges from `self.ssh_hosts` (after Step 8-3 Add/Delete) we
        // adjust the tail diff only.
        write_ssh_hosts_back(&mut doc, &self.ssh_hosts);

        // Create the parent directory if necessary.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, doc.to_string())?;
        Ok(())
    }

    // ===== Phase 4 (UI/UX v2): category search =====

    /// Activate the search input. The next character event lands in
    /// `search_query` instead of being treated as a panel hotkey.
    pub fn focus_search(&mut self) {
        self.search_focused = true;
    }

    /// Deactivate the search input but keep the query (so the filter
    /// remains visible). Use `clear_search` to drop the query too.
    pub fn unfocus_search(&mut self) {
        self.search_focused = false;
    }

    /// Clear the query and defocus. Triggered by Esc while the search field
    /// is focused.
    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_focused = false;
    }

    /// Append a character to the query.
    pub fn push_search_char(&mut self, ch: char) {
        if !ch.is_control() {
            self.search_query.push(ch);
        }
    }

    /// Remove the last character from the query.
    pub fn pop_search_char(&mut self) {
        self.search_query.pop();
    }

    /// Whether the search query is currently filtering categories.
    pub fn is_search_filtering(&self) -> bool {
        !self.search_query.trim().is_empty()
    }

    /// Categories visible in the sidebar given the current query. Empty
    /// query returns every category in the canonical order. Pure function so
    /// tests can pin the ranking behaviour.
    pub fn filtered_categories(&self) -> Vec<SettingsCategory> {
        filter_categories(&self.search_query, SettingsCategory::ALL)
    }

    /// Number of fields in `cat` that match the current `search_query`.
    /// Returns `0` when the query is empty so the sidebar can suppress
    /// the badge cleanly. Phase 4b: drives the `(N)` hit-count after
    /// the category label.
    pub fn field_hit_count(&self, cat: &SettingsCategory) -> usize {
        field_hit_count(cat, &self.search_query)
    }
}

/// A single searchable field inside a category. Phase 4b extends the
/// Phase 4 category-level filter so a query like "font size" highlights
/// the Font category with a hit-count badge, instead of only matching
/// against a small curated keyword list.
///
/// `label` is the human-readable label as shown (or close to what is
/// shown) in the settings panel. `aliases` are extra search terms that
/// should also hit this field (e.g. "shell" for the `command` field
/// inside Profiles).
#[derive(Debug, Clone, Copy)]
pub struct FieldEntry {
    pub label: &'static str,
    pub aliases: &'static [&'static str],
}

/// Pure catalogue of the searchable fields per category. Used by the
/// Phase 4b field-level search; intentionally curated rather than
/// auto-derived from the renderer because the renderer is hand-written
/// per category and there is no single struct of record. The labels do
/// not need to match the renderer byte-for-byte; they exist so that
/// fuzzy queries against the field name or its aliases land on the
/// right category.
pub fn category_fields(cat: &SettingsCategory) -> &'static [FieldEntry] {
    match cat {
        SettingsCategory::Startup => &[
            FieldEntry {
                label: "Language",
                aliases: &["locale", "i18n", "translation"],
            },
            FieldEntry {
                label: "Check for updates on startup",
                aliases: &["update", "release", "auto-update"],
            },
            FieldEntry {
                label: "Restore previous session",
                aliases: &["session", "snapshot", "persist"],
            },
        ],
        SettingsCategory::Font => &[
            FieldEntry {
                label: "Family",
                aliases: &["font", "typeface"],
            },
            FieldEntry {
                label: "Size",
                aliases: &["font size", "pt", "px"],
            },
            FieldEntry {
                label: "Ligatures",
                aliases: &["ligature", "harfbuzz"],
            },
        ],
        SettingsCategory::Theme => &[
            FieldEntry {
                label: "Theme",
                aliases: &["color scheme", "palette", "colors"],
            },
            FieldEntry {
                label: "Light theme",
                aliases: &["light", "day"],
            },
            FieldEntry {
                label: "Dark theme",
                aliases: &["dark", "night"],
            },
            FieldEntry {
                label: "Follow system theme",
                aliases: &["os theme", "system", "auto"],
            },
        ],
        SettingsCategory::Window => &[
            FieldEntry {
                label: "Opacity",
                aliases: &["transparency", "alpha"],
            },
            FieldEntry {
                label: "Horizontal padding",
                aliases: &["padding x", "margin"],
            },
            FieldEntry {
                label: "Vertical padding",
                aliases: &["padding y", "margin"],
            },
            FieldEntry {
                label: "Cursor style",
                aliases: &["caret", "block", "beam", "underline"],
            },
            FieldEntry {
                label: "Present mode",
                aliases: &["vsync", "fifo", "mailbox"],
            },
            FieldEntry {
                label: "Acrylic blur",
                aliases: &["blur", "acrylic", "windows 11"],
            },
            FieldEntry {
                label: "Background image",
                aliases: &["wallpaper", "image"],
            },
        ],
        SettingsCategory::Ssh => &[
            FieldEntry {
                label: "SSH hosts",
                aliases: &["remote", "ssh", "host"],
            },
            FieldEntry {
                label: "Name",
                aliases: &["host name", "alias"],
            },
            FieldEntry {
                label: "Host",
                aliases: &["hostname", "address"],
            },
            FieldEntry {
                label: "Port",
                aliases: &["tcp", "ssh port"],
            },
            FieldEntry {
                label: "Username",
                aliases: &["user", "login"],
            },
            FieldEntry {
                label: "Auth type",
                aliases: &["authentication", "key", "password", "agent"],
            },
        ],
        SettingsCategory::Keybindings => &[
            FieldEntry {
                label: "Key bindings",
                aliases: &["shortcut", "hotkey", "binding"],
            },
            FieldEntry {
                label: "Action",
                aliases: &["command"],
            },
            FieldEntry {
                label: "Modifiers",
                aliases: &["ctrl", "shift", "alt", "cmd"],
            },
        ],
        SettingsCategory::Profiles => &[
            FieldEntry {
                label: "Profiles",
                aliases: &["shell", "session profile"],
            },
            FieldEntry {
                label: "Name",
                aliases: &["profile name"],
            },
            FieldEntry {
                label: "Command",
                aliases: &["shell", "executable", "bash", "powershell", "zsh"],
            },
            FieldEntry {
                label: "Working directory",
                aliases: &["cwd", "start dir"],
            },
            FieldEntry {
                label: "Environment",
                aliases: &["env", "variable"],
            },
        ],
        SettingsCategory::Blocks => &[
            FieldEntry {
                label: "Command blocks",
                aliases: &["warp", "osc133", "block"],
            },
            FieldEntry {
                label: "Enable blocks",
                aliases: &["toggle", "on", "off"],
            },
            FieldEntry {
                label: "Block divider style",
                aliases: &["divider", "separator"],
            },
        ],
    }
}

/// Score `query` against a single field (max of label-score and
/// best alias-score). Returns `0` when nothing matched.
fn score_field(
    matcher: &fuzzy_matcher::skim::SkimMatcherV2,
    field: &FieldEntry,
    query: &str,
) -> i64 {
    use fuzzy_matcher::FuzzyMatcher;
    let label_score = matcher.fuzzy_match(field.label, query).unwrap_or(0);
    let alias_score = field
        .aliases
        .iter()
        .filter_map(|a| matcher.fuzzy_match(a, query))
        .max()
        .unwrap_or(0);
    label_score.max(alias_score)
}

/// Best field score in a category for the given query. `0` when no
/// field matched. Used both by `filter_categories` (for ranking) and
/// `field_hit_count` (for the sidebar badge).
fn best_field_score(cat: &SettingsCategory, query: &str) -> i64 {
    use fuzzy_matcher::skim::SkimMatcherV2;
    let matcher = SkimMatcherV2::default();
    category_fields(cat)
        .iter()
        .map(|f| score_field(&matcher, f, query))
        .max()
        .unwrap_or(0)
}

/// Number of fields in `cat` that match `query` with a positive score.
/// Drives the `(N)` badge in the sidebar when filtering is active.
/// Pure function so tests can pin the count behaviour.
pub fn field_hit_count(cat: &SettingsCategory, query: &str) -> usize {
    let q = query.trim();
    if q.is_empty() {
        return 0;
    }
    use fuzzy_matcher::skim::SkimMatcherV2;
    let matcher = SkimMatcherV2::default();
    category_fields(cat)
        .iter()
        .filter(|f| score_field(&matcher, f, q) > 0)
        .count()
}

/// Pure helper that ranks categories for the given fuzzy query. Empty / blank
/// queries fall through to the canonical order so the sidebar reverts cleanly
/// when the user clears the search. Match score is the max across the
/// category label and the per-field score (label + aliases) from
/// `category_fields`; categories without any positive score are
/// dropped. Stable sort on `(-score, original_index)` keeps the canonical
/// order as a tiebreaker.
pub fn filter_categories(query: &str, all: &[SettingsCategory]) -> Vec<SettingsCategory> {
    let q = query.trim();
    if q.is_empty() {
        return all.to_vec();
    }
    use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, usize, SettingsCategory)> = all
        .iter()
        .enumerate()
        .filter_map(|(idx, cat)| {
            let label_score = matcher.fuzzy_match(cat.label(), q).unwrap_or(0);
            let field_score = best_field_score(cat, q);
            let best = label_score.max(field_score);
            if best > 0 {
                Some((best, idx, cat.clone()))
            } else {
                None
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, c)| c).collect()
}

/// Update the `[[hosts]]` array in place (Phase 5-11-8 Step 8-2).
///
/// Keeps the existing `ArrayOfTables` and overwrites only the 5 fields
/// managed by `SettingsPanel` (name / host / port / username / auth_type).
/// Unmanaged fields such as `key_path` / `forward_local` / `proxy_jump` /
/// `tags` are left untouched (so user-edited TOML values are not lost).
///
/// Length adjustments:
/// - `ssh_hosts.len() > arr.len()`: append a new Table at the tail (used by
///   Step 8-3 Add).
/// - `ssh_hosts.len() < arr.len()`: remove the trailing Table(s) (used by
///   Step 8-3 Delete).
/// - Equal: in-place updates only.
pub(crate) fn write_ssh_hosts_back(doc: &mut toml_edit::DocumentMut, hosts: &[SshHostEntry]) {
    // Get the existing hosts entry as `ArrayOfTables` (create one if absent).
    let entry = doc
        .entry("hosts")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));

    // If the existing item is not an `ArrayOfTables` (broken by manual
    // editing), rebuild it.
    if !entry.is_array_of_tables() {
        *entry = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    let Some(arr) = entry.as_array_of_tables_mut() else {
        return;
    };

    // Overwrite the 5 managed fields per index.
    for (i, host) in hosts.iter().enumerate() {
        if i < arr.len() {
            let t = arr.get_mut(i).expect("length was already checked");
            t.insert("name", toml_edit::value(host.name.as_str()));
            t.insert("host", toml_edit::value(host.host.as_str()));
            t.insert("port", toml_edit::value(host.port as i64));
            t.insert("username", toml_edit::value(host.username.as_str()));
            t.insert("auth_type", toml_edit::value(host.auth_type.as_str()));
        } else {
            // Append a new entry (used by Step 8-3 Add).
            let mut t = toml_edit::Table::new();
            t.insert("name", toml_edit::value(host.name.as_str()));
            t.insert("host", toml_edit::value(host.host.as_str()));
            t.insert("port", toml_edit::value(host.port as i64));
            t.insert("username", toml_edit::value(host.username.as_str()));
            t.insert("auth_type", toml_edit::value(host.auth_type.as_str()));
            arr.push(t);
        }
    }
    // Pop surplus entries from the tail (used by Step 8-3 Delete).
    while arr.len() > hosts.len() {
        arr.remove(arr.len() - 1);
    }
}

/// Language choices: (display name, language code).
///
/// The display names are intentionally written in each language's native script
/// so the picker shows them in their own form.
pub const LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("Auto (OS)", "auto"),
    ("English", "en"),
    ("日本語", "ja"),
    ("Français", "fr"),
    ("Deutsch", "de"),
    ("Español", "es"),
    ("Italiano", "it"),
    ("中文(简体)", "zh-CN"),
    ("한국어", "ko"),
];

/// Convert a color scheme into its index.
fn scheme_name_to_index(colors: &nexterm_config::ColorScheme) -> usize {
    use nexterm_config::{BuiltinScheme, ColorScheme};
    match colors {
        ColorScheme::Builtin(b) => match b {
            BuiltinScheme::Dark => 0,
            BuiltinScheme::Light => 1,
            BuiltinScheme::TokyoNight => 2,
            BuiltinScheme::Solarized => 3,
            BuiltinScheme::Gruvbox => 4,
            BuiltinScheme::Catppuccin => 5,
            BuiltinScheme::Dracula => 6,
            BuiltinScheme::Nord => 7,
            BuiltinScheme::OneDark => 8,
        },
        ColorScheme::Custom(_) => 0,
    }
}

/// Inverse of `scheme_name_to_index`: map a 0..=8 slot to a
/// `BuiltinScheme`. Used by Phase 3b live theme preview to derive a
/// `ColorScheme` value from a hovered dot index. Pure helper so it
/// can be unit-tested without instantiating a renderer.
///
/// Out-of-range inputs wrap modulo 9 so the caller doesn't need to
/// clamp ahead of time.
pub fn index_to_builtin_scheme(idx: usize) -> nexterm_config::BuiltinScheme {
    use nexterm_config::BuiltinScheme;
    match idx % 9 {
        0 => BuiltinScheme::Dark,
        1 => BuiltinScheme::Light,
        2 => BuiltinScheme::TokyoNight,
        3 => BuiltinScheme::Solarized,
        4 => BuiltinScheme::Gruvbox,
        5 => BuiltinScheme::Catppuccin,
        6 => BuiltinScheme::Dracula,
        7 => BuiltinScheme::Nord,
        _ => BuiltinScheme::OneDark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn default_state_from_config() {
        let config = Config::default();
        let panel = SettingsPanel::new(&config);
        assert!(!panel.is_open);
        assert_eq!(panel.category, SettingsCategory::Font);
        assert!(!panel.dirty);
        assert_eq!(panel.font_size, config.font.size);
        assert_eq!(panel.opacity, config.window.background_opacity);
    }

    #[test]
    fn font_size_clamped() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.font_size = 32.0;
        panel.increase_font_size();
        assert_eq!(
            panel.font_size, 32.0,
            "must not exceed the 32.0 upper bound"
        );

        panel.font_size = 8.0;
        panel.decrease_font_size();
        assert_eq!(
            panel.font_size, 8.0,
            "must not fall below the 8.0 lower bound"
        );

        panel.font_size = 14.0;
        panel.increase_font_size();
        assert!((panel.font_size - 14.5).abs() < f32::EPSILON);
        assert!(panel.dirty);
    }

    #[test]
    fn scheme_wraps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.scheme_index = 8;
        panel.next_scheme();
        assert_eq!(panel.scheme_index, 0, "the slot after index 8 wraps to 0");

        panel.scheme_index = 0;
        panel.prev_scheme();
        assert_eq!(panel.scheme_index, 8, "the slot before index 0 wraps to 8");
    }

    #[test]
    fn tab_rename_lifecycle() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert!(panel.tab_rename_editing.is_none());

        panel.begin_tab_rename(42, "main");
        assert_eq!(panel.tab_rename_editing, Some(42));
        assert_eq!(panel.tab_rename_text, "main");

        panel.push_tab_rename_char('!');
        assert_eq!(panel.tab_rename_text, "main!");

        panel.pop_tab_rename_char();
        assert_eq!(panel.tab_rename_text, "main");

        panel.cancel_tab_rename();
        assert!(panel.tab_rename_editing.is_none());
        assert!(panel.tab_rename_text.is_empty());
    }

    #[test]
    fn category_navigation() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.category = SettingsCategory::Font;
        panel.next_category();
        assert_eq!(panel.category, SettingsCategory::Theme);
        panel.prev_category();
        assert_eq!(panel.category, SettingsCategory::Font);
    }

    // ===== Phase 5-11-6 #6: cursor_style / padding / present_mode =====

    #[test]
    fn cursor_style_cycle_forward_and_back() {
        use nexterm_config::CursorStyle::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        // Default is Block.
        assert_eq!(panel.cursor_style, Block);
        assert_eq!(panel.cursor_style_index(), 0);
        assert_eq!(panel.cursor_style_toml_key(), "block");

        panel.next_cursor_style();
        assert_eq!(panel.cursor_style, Beam);
        assert_eq!(panel.cursor_style_index(), 1);
        assert_eq!(panel.cursor_style_toml_key(), "beam");

        panel.next_cursor_style();
        assert_eq!(panel.cursor_style, Underline);
        assert_eq!(panel.cursor_style_toml_key(), "underline");

        panel.next_cursor_style();
        assert_eq!(
            panel.cursor_style, Block,
            "the slot after Underline wraps to Block"
        );

        // Reverse direction.
        panel.prev_cursor_style();
        assert_eq!(
            panel.cursor_style, Underline,
            "the slot before Block is Underline"
        );
        panel.prev_cursor_style();
        assert_eq!(panel.cursor_style, Beam);
        panel.prev_cursor_style();
        assert_eq!(panel.cursor_style, Block);

        assert!(panel.dirty);
    }

    #[test]
    fn cursor_style_labels_are_human_readable() {
        use nexterm_config::CursorStyle::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.cursor_style = Block;
        assert!(panel.cursor_style_label().contains("Block"));
        panel.cursor_style = Beam;
        assert!(panel.cursor_style_label().contains("Beam"));
        panel.cursor_style = Underline;
        assert!(panel.cursor_style_label().contains("Underline"));
    }

    #[test]
    fn padding_x_increase_decrease_clamps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert_eq!(panel.padding_x, 0, "the default is 0");

        // Clamps at the upper bound 32.
        for _ in 0..40 {
            panel.increase_padding_x();
        }
        assert_eq!(panel.padding_x, 32);

        // Clamps at the lower bound 0.
        for _ in 0..40 {
            panel.decrease_padding_x();
        }
        assert_eq!(panel.padding_x, 0);

        assert!(panel.dirty);
    }

    #[test]
    fn padding_y_increase_decrease_clamps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        for _ in 0..50 {
            panel.increase_padding_y();
        }
        assert_eq!(panel.padding_y, 32, "upper bound");
        for _ in 0..50 {
            panel.decrease_padding_y();
        }
        assert_eq!(panel.padding_y, 0, "lower bound");
    }

    #[test]
    fn padding_set_value_clamps_and_rounds() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.set_padding_x_value(-5.0);
        assert_eq!(panel.padding_x, 0, "negative values clamp to 0");
        panel.set_padding_x_value(100.0);
        assert_eq!(
            panel.padding_x, 32,
            "values above the upper bound clamp to 32"
        );
        panel.set_padding_x_value(15.7);
        assert_eq!(panel.padding_x, 16, "values at or above .5 round up");
        panel.set_padding_x_value(15.3);
        assert_eq!(panel.padding_x, 15, "values below .5 round down");

        panel.set_padding_y_value(7.5);
        assert_eq!(
            panel.padding_y, 8,
            ".5 may round either bankers/half-up depending on the implementation"
        );
    }

    #[test]
    fn present_mode_cycle_forward_and_back() {
        use nexterm_config::PresentModeConfig::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        // The default is Mailbox (changed in Sprint 5-3 / C3).
        assert_eq!(panel.present_mode, Mailbox);
        assert_eq!(panel.present_mode_index(), 1);
        assert_eq!(panel.present_mode_toml_key(), "mailbox");

        panel.next_present_mode();
        assert_eq!(panel.present_mode, Auto);
        panel.next_present_mode();
        assert_eq!(panel.present_mode, Fifo);
        panel.next_present_mode();
        assert_eq!(panel.present_mode, Mailbox);

        // Reverse direction.
        panel.prev_present_mode();
        assert_eq!(panel.present_mode, Fifo);

        assert!(panel.dirty);
    }

    #[test]
    fn present_mode_labels_are_human_readable() {
        use nexterm_config::PresentModeConfig::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.present_mode = Fifo;
        assert!(panel.present_mode_label().contains("Fifo"));
        panel.present_mode = Mailbox;
        assert!(panel.present_mode_label().contains("Mailbox"));
        panel.present_mode = Auto;
        assert!(panel.present_mode_label().contains("Auto"));
    }

    #[test]
    fn new_reads_config_window_padding_and_present_mode() {
        let mut config = Config::default();
        config.window.padding_x = 12;
        config.window.padding_y = 4;
        config.gpu.present_mode = nexterm_config::PresentModeConfig::Fifo;
        config.cursor_style = nexterm_config::CursorStyle::Beam;

        let panel = SettingsPanel::new(&config);
        assert_eq!(panel.padding_x, 12);
        assert_eq!(panel.padding_y, 4);
        assert_eq!(panel.present_mode, nexterm_config::PresentModeConfig::Fifo);
        assert_eq!(panel.cursor_style, nexterm_config::CursorStyle::Beam);
    }

    #[test]
    fn new_clamps_oversized_padding_from_config() {
        let mut config = Config::default();
        config.window.padding_x = 1000;
        let panel = SettingsPanel::new(&config);
        assert_eq!(
            panel.padding_x, 32,
            "out-of-range config values are clamped to 32 in `new`"
        );
    }

    // ============================================================
    // Sprint 5-11-8 Step 8-3 Sub-phase E: TextInputState unit tests
    // ============================================================

    #[test]
    fn text_input_state_new_cursor_at_end() {
        let s = TextInputState::new("hello".to_string());
        assert_eq!(s.buffer, "hello");
        assert_eq!(s.cursor, 5);
        assert!(s.preedit.is_none());

        let empty = TextInputState::new(String::new());
        assert_eq!(empty.cursor, 0);
    }

    #[test]
    fn text_input_state_insert_char_advances_cursor_ascii() {
        let mut s = TextInputState::new(String::new());
        s.insert_char('a');
        s.insert_char('b');
        s.insert_char('c');
        assert_eq!(s.buffer, "abc");
        assert_eq!(s.cursor, 3);
    }

    #[test]
    fn text_input_state_insert_char_advances_cursor_cjk() {
        // One Japanese character = 3 bytes in UTF-8; the cursor must advance
        // in bytes too.
        let mut s = TextInputState::new(String::new());
        s.insert_char('あ');
        assert_eq!(s.buffer, "あ");
        assert_eq!(s.cursor, 3);
        s.insert_char('い');
        assert_eq!(s.buffer, "あい");
        assert_eq!(s.cursor, 6);
    }

    #[test]
    fn text_input_state_backspace_respects_utf8_boundary() {
        // Backspace on "あい" yields "あ" with the cursor at byte 3 (boundary).
        let mut s = TextInputState::new("あい".to_string());
        assert_eq!(s.cursor, 6);
        s.backspace();
        assert_eq!(s.buffer, "あ");
        assert_eq!(s.cursor, 3);
        s.backspace();
        assert_eq!(s.buffer, "");
        assert_eq!(s.cursor, 0);
        // Backspace on an empty buffer is a no-op.
        s.backspace();
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn text_input_state_move_left_right_clamps_and_respects_boundary() {
        let mut s = TextInputState::new("aあb".to_string());
        // Tail (5 = 1 + 3 + 1).
        assert_eq!(s.cursor, 5);
        s.move_left();
        assert_eq!(s.cursor, 4, "right before 'b'");
        s.move_left();
        assert_eq!(
            s.cursor, 1,
            "right before 'あ' (honours the UTF-8 boundary)"
        );
        s.move_left();
        assert_eq!(s.cursor, 0);
        // Moving further left at the head is a no-op.
        s.move_left();
        assert_eq!(s.cursor, 0);

        s.move_right();
        assert_eq!(s.cursor, 1);
        s.move_right();
        assert_eq!(s.cursor, 4, "steps past 'あ'");
        s.move_right();
        assert_eq!(s.cursor, 5);
        // Moving further right at the tail is a no-op.
        s.move_right();
        assert_eq!(s.cursor, 5);
    }

    #[test]
    fn text_input_state_display_string_with_preedit() {
        let mut s = TextInputState::new("ab".to_string());
        s.move_left(); // cursor to 1
        assert_eq!(s.cursor, 1);
        s.preedit = Some("X".to_string());

        // The display string inserts the preedit at the cursor.
        assert_eq!(s.display_string(), "aXb");
        assert_eq!(s.display_cursor(), 2, "points to the end of the preedit");

        // Clearing the preedit restores the original.
        s.preedit = None;
        assert_eq!(s.display_string(), "ab");
        assert_eq!(s.display_cursor(), 1);
    }

    // ============================================================
    // Sprint 5-11-8 Step 8-3 Sub-phase E: SSH field edit lifecycle
    // ============================================================

    fn panel_with_one_host() -> SettingsPanel {
        let mut panel = SettingsPanel::new(&Config::default());
        panel.ssh_hosts.push(SshHostEntry {
            name: "myhost".to_string(),
            host: "example.com".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth_type: "password".to_string(),
        });
        panel.selected_host_index = 0;
        panel
    }

    #[test]
    fn ssh_field_edit_begin_commit_lifecycle() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 1; // name

        assert!(panel.begin_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_some());
        let state = panel.ssh_field_editing.as_ref().unwrap();
        assert_eq!(state.buffer, "myhost");

        // Edit a character.
        panel.ssh_field_insert_char('!');
        assert_eq!(panel.ssh_field_editing.as_ref().unwrap().buffer, "myhost!");

        // Commit writes back to the host.
        assert!(panel.commit_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_none());
        assert_eq!(panel.ssh_hosts[0].name, "myhost!");
        assert!(panel.dirty);
    }

    #[test]
    fn ssh_field_edit_cancel_discards_changes() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 2; // host
        panel.begin_ssh_field_edit();
        panel.ssh_field_insert_char('X');

        assert!(panel.cancel_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_none());
        // The host is unchanged.
        assert_eq!(panel.ssh_hosts[0].host, "example.com");
    }

    #[test]
    fn ssh_field_edit_begin_returns_false_for_non_text_fields() {
        let mut panel = panel_with_one_host();
        // port (3) / auth_type (5) / ListBox (0) are not TextInputs, so begin
        // must return false.
        for focus in [0u8, 3, 5, 6, 7] {
            panel.ssh_field_focus = focus;
            assert!(
                !panel.begin_ssh_field_edit(),
                "focus={focus} is not a TextInput, so begin_ssh_field_edit should return false"
            );
            assert!(panel.ssh_field_editing.is_none());
        }
    }

    // ============================================================
    // Sprint 5-11-8 Step 8-3 Sub-phase E: Add / Delete + confirmation dialog
    // ============================================================

    #[test]
    fn add_ssh_host_appends_with_defaults_and_enters_edit_mode() {
        let mut panel = SettingsPanel::new(&Config::default());
        assert!(panel.ssh_hosts.is_empty());

        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 1);
        let new_host = &panel.ssh_hosts[0];
        assert_eq!(new_host.name, "");
        assert_eq!(new_host.host, "");
        assert_eq!(new_host.port, 22);
        assert_eq!(new_host.username, "");
        assert_eq!(new_host.auth_type, "password");

        assert_eq!(panel.selected_host_index, 0);
        assert_eq!(panel.ssh_field_focus, 1, "focus moves to the name field");
        assert!(
            panel.ssh_field_editing.is_some(),
            "name edit mode should start immediately"
        );
        assert_eq!(
            panel.ssh_field_editing.as_ref().unwrap().buffer,
            "",
            "the name of a new host is initialised to an empty string"
        );
        assert!(panel.dirty);
    }

    #[test]
    fn add_ssh_host_extends_existing_list() {
        let mut panel = panel_with_one_host();
        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(
            panel.selected_host_index, 1,
            "the newly added trailing host is selected"
        );
    }

    #[test]
    fn open_ssh_delete_dialog_noop_when_empty() {
        let mut panel = SettingsPanel::new(&Config::default());
        assert!(panel.ssh_hosts.is_empty());
        panel.open_ssh_delete_dialog();
        assert!(
            !panel.ssh_delete_dialog_open,
            "no dialog opens when the list is empty"
        );
    }

    #[test]
    fn open_ssh_delete_dialog_defaults_to_cancel_focus() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        assert!(panel.ssh_delete_dialog_open);
        assert!(
            !panel.ssh_delete_dialog_confirm_focused,
            "accidental-deletion guard: Cancel is the default focused button"
        );
    }

    #[test]
    fn cancel_ssh_delete_dialog_clears_state_and_keeps_host() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        panel.ssh_delete_dialog_confirm_focused = true;
        panel.cancel_ssh_delete_dialog();

        assert!(!panel.ssh_delete_dialog_open);
        assert!(!panel.ssh_delete_dialog_confirm_focused);
        assert_eq!(panel.ssh_hosts.len(), 1, "nothing is deleted");
    }

    #[test]
    fn confirm_ssh_delete_dialog_removes_at_end_clamps_to_prev() {
        let mut panel = panel_with_one_host();
        // Set up 2 hosts and delete the tail.
        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(panel.selected_host_index, 1);

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert_eq!(panel.ssh_hosts.len(), 1);
        assert_eq!(
            panel.selected_host_index, 0,
            "deleting the tail clamps the index to n-1=0"
        );
        assert!(!panel.ssh_delete_dialog_open);
        assert!(panel.dirty);
    }

    #[test]
    fn confirm_ssh_delete_dialog_middle_index_keeps_position() {
        let mut panel = panel_with_one_host();
        panel.add_ssh_host();
        panel.add_ssh_host(); // 3 hosts in total
        panel.selected_host_index = 1; // select the middle one

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(
            panel.selected_host_index, 1,
            "deleting the middle entry shifts the tail and leaves index=1 unchanged"
        );
    }

    #[test]
    fn confirm_ssh_delete_dialog_empty_after_resets_focus() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 3; // any non-zero value

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert!(panel.ssh_hosts.is_empty());
        assert_eq!(panel.selected_host_index, 0);
        assert_eq!(
            panel.ssh_field_focus, 0,
            "the focus returns to the ListBox once the list is empty"
        );
    }

    #[test]
    fn toggle_ssh_delete_dialog_focus_alternates() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        assert!(!panel.ssh_delete_dialog_confirm_focused);

        panel.toggle_ssh_delete_dialog_focus();
        assert!(panel.ssh_delete_dialog_confirm_focused);

        panel.toggle_ssh_delete_dialog_focus();
        assert!(!panel.ssh_delete_dialog_confirm_focused);
    }

    // ===== Phase 5-11-9 Sub-phase A: KeyBindingEntry + initial state =====

    #[test]
    fn keybinding_entry_label_normal() {
        let kb = KeyBindingEntry {
            key: "ctrl+shift+p".to_string(),
            action: "CommandPalette".to_string(),
        };
        assert_eq!(kb.label(), "ctrl+shift+p → CommandPalette");
    }

    #[test]
    fn keybinding_entry_label_empty_key_or_action() {
        let kb = KeyBindingEntry {
            key: String::new(),
            action: "Quit".to_string(),
        };
        assert_eq!(kb.label(), "(unbound) → Quit");
        let kb2 = KeyBindingEntry {
            key: "ctrl+b d".to_string(),
            action: String::new(),
        };
        assert_eq!(kb2.label(), "ctrl+b d → (none)");
    }

    #[test]
    fn keybindings_loaded_from_default_config() {
        let config = Config::default();
        let panel = SettingsPanel::new(&config);
        // The default config defines several key bindings; the panel must
        // mirror them 1:1 with matching length, key strings, and action names.
        assert_eq!(panel.keybindings.len(), config.keys.len());
        for (i, kb) in panel.keybindings.iter().enumerate() {
            assert_eq!(kb.key, config.keys[i].key);
            assert_eq!(kb.action, config.keys[i].action);
        }
        assert_eq!(panel.selected_key_index, 0);
        assert_eq!(panel.key_field_focus, 0);
    }

    #[test]
    fn keybindings_empty_when_config_keys_empty() {
        let mut config = Config::default();
        config.keys.clear();
        let panel = SettingsPanel::new(&config);
        assert!(panel.keybindings.is_empty());
        assert_eq!(panel.selected_key_index, 0);
        assert_eq!(panel.key_field_focus, 0);
    }

    #[test]
    fn keybinding_actions_contains_known_actions() {
        // Sanity check: a representative subset of actions exists in the table.
        for name in [
            "Quit",
            "CommandPalette",
            "SplitVertical",
            "DetachToNewWindow",
            "CloseOsWindow",
        ] {
            assert!(
                KEYBINDING_ACTIONS.contains(&name),
                "KEYBINDING_ACTIONS must include `{name}`"
            );
        }
        // No duplicates allowed.
        let mut sorted: Vec<&&str> = KEYBINDING_ACTIONS.iter().collect();
        sorted.sort();
        let dedup_len = {
            let mut s = sorted.clone();
            s.dedup();
            s.len()
        };
        assert_eq!(sorted.len(), dedup_len, "KEYBINDING_ACTIONS has duplicates");
    }

    // ===== Phase 5-11-9 Sub-phase B: KeyEditMode tests =====

    fn panel_with_one_binding() -> SettingsPanel {
        SettingsPanel {
            keybindings: vec![KeyBindingEntry {
                key: "ctrl+shift+p".to_string(),
                action: "CommandPalette".to_string(),
            }],
            selected_key_index: 0,
            ..Default::default()
        }
    }

    #[test]
    fn begin_key_record_starts_record_mode() {
        let mut panel = panel_with_one_binding();
        assert!(panel.begin_key_record());
        assert!(panel.is_key_recording());
        assert!(!panel.is_key_text_editing());
    }

    #[test]
    fn begin_key_record_noop_when_empty() {
        let mut panel = SettingsPanel::default();
        panel.keybindings.clear();
        assert!(!panel.begin_key_record());
        assert!(panel.key_editing.is_none());
    }

    #[test]
    fn capture_key_record_writes_back_and_exits() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_record();
        assert!(panel.capture_key_record("ctrl+q".to_string()));
        assert_eq!(panel.keybindings[0].key, "ctrl+q");
        assert!(panel.key_editing.is_none());
        assert!(panel.dirty);
    }

    #[test]
    fn capture_key_record_noop_when_not_recording() {
        let mut panel = panel_with_one_binding();
        // Without begin_key_record, capture must do nothing.
        assert!(!panel.capture_key_record("ctrl+q".to_string()));
        assert_eq!(panel.keybindings[0].key, "ctrl+shift+p");
    }

    #[test]
    fn toggle_key_edit_mode_record_to_text_preserves_value() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_record();
        panel.toggle_key_edit_mode();
        assert!(panel.is_key_text_editing());
        // Buffer is seeded with the current binding's key value.
        if let Some(KeyEditMode::Text(state)) = &panel.key_editing {
            assert_eq!(state.buffer, "ctrl+shift+p");
        } else {
            panic!("expected Text mode after toggle");
        }
    }

    #[test]
    fn toggle_key_edit_mode_text_to_record_discards_buffer() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_text_edit();
        // Type a character into the buffer.
        panel.key_field_insert_char('a');
        panel.toggle_key_edit_mode();
        assert!(panel.is_key_recording());
        // Original binding key untouched.
        assert_eq!(panel.keybindings[0].key, "ctrl+shift+p");
    }

    #[test]
    fn commit_key_edit_writes_text_buffer() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_text_edit();
        // Replace the buffer with a prefix binding.
        if let Some(KeyEditMode::Text(state)) = panel.key_editing.as_mut() {
            state.buffer = "ctrl+b d".to_string();
            state.cursor = state.buffer.len();
        }
        assert!(panel.commit_key_edit());
        assert_eq!(panel.keybindings[0].key, "ctrl+b d");
        assert!(panel.key_editing.is_none());
        assert!(panel.dirty);
    }

    #[test]
    fn cancel_key_edit_discards_buffer() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_text_edit();
        panel.key_field_insert_char('x');
        assert!(panel.cancel_key_edit());
        // Original binding survives, edit state cleared.
        assert_eq!(panel.keybindings[0].key, "ctrl+shift+p");
        assert!(panel.key_editing.is_none());
    }

    #[test]
    fn key_field_text_edit_methods_proxy_to_state() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_text_edit();
        panel.key_field_move_home();
        panel.key_field_insert_str("abc");
        if let Some(KeyEditMode::Text(state)) = &panel.key_editing {
            assert_eq!(state.buffer, "abcctrl+shift+p");
            assert_eq!(state.cursor, 3);
        }
        panel.key_field_move_end();
        panel.key_field_backspace();
        if let Some(KeyEditMode::Text(state)) = &panel.key_editing {
            // Last char ('p') was removed.
            assert_eq!(state.buffer, "abcctrl+shift+");
        }
    }

    #[test]
    fn close_panel_resets_key_editing() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_record();
        panel.close();
        assert!(panel.key_editing.is_none());
    }

    // ===== Phase 5-11-9 Sub-phase C: Action ComboBox tests =====

    #[test]
    fn next_key_action_cycles_forward_through_full_list() {
        let mut panel = panel_with_one_binding();
        // Seed with the first action so the cycle is deterministic.
        panel.keybindings[0].action = KEYBINDING_ACTIONS[0].to_string();
        panel.dirty = false;
        for i in 0..KEYBINDING_ACTIONS.len() {
            assert!(panel.next_key_action());
            let expected = KEYBINDING_ACTIONS[(i + 1) % KEYBINDING_ACTIONS.len()];
            assert_eq!(panel.keybindings[0].action, expected);
        }
        // After a full cycle we are back at index 0.
        assert_eq!(panel.keybindings[0].action, KEYBINDING_ACTIONS[0]);
        assert!(panel.dirty);
    }

    #[test]
    fn prev_key_action_cycles_backward_through_full_list() {
        let mut panel = panel_with_one_binding();
        panel.keybindings[0].action = KEYBINDING_ACTIONS[0].to_string();
        panel.dirty = false;
        // First step wraps to the last action.
        assert!(panel.prev_key_action());
        assert_eq!(
            panel.keybindings[0].action,
            KEYBINDING_ACTIONS[KEYBINDING_ACTIONS.len() - 1]
        );
        assert!(panel.dirty);
    }

    #[test]
    fn next_key_action_snaps_unknown_to_first() {
        let mut panel = panel_with_one_binding();
        panel.keybindings[0].action = "BogusAction".to_string();
        panel.dirty = false;
        assert!(panel.next_key_action());
        assert_eq!(panel.keybindings[0].action, KEYBINDING_ACTIONS[0]);
        assert!(panel.dirty);
    }

    #[test]
    fn prev_key_action_snaps_unknown_to_last() {
        let mut panel = panel_with_one_binding();
        panel.keybindings[0].action = "TypoHere".to_string();
        panel.dirty = false;
        assert!(panel.prev_key_action());
        assert_eq!(
            panel.keybindings[0].action,
            KEYBINDING_ACTIONS[KEYBINDING_ACTIONS.len() - 1]
        );
        assert!(panel.dirty);
    }

    #[test]
    fn key_action_cycles_noop_when_empty() {
        let mut panel = SettingsPanel::default();
        panel.keybindings.clear();
        assert!(!panel.next_key_action());
        assert!(!panel.prev_key_action());
        assert!(!panel.dirty);
    }

    #[test]
    fn selected_key_action_is_valid_detects_unknown() {
        let mut panel = panel_with_one_binding();
        assert!(panel.selected_key_action_is_valid());
        panel.keybindings[0].action = "BogusAction".to_string();
        assert!(!panel.selected_key_action_is_valid());
    }

    #[test]
    fn next_key_action_does_not_touch_key_field() {
        let mut panel = panel_with_one_binding();
        let key_before = panel.keybindings[0].key.clone();
        panel.next_key_action();
        // Only `action` should change. The key field is owned by Sub-phase B.
        assert_eq!(panel.keybindings[0].key, key_before);
    }

    // ===== Phase 5-11-9 Sub-phase D: Add / Delete + dialog tests =====

    #[test]
    fn add_key_binding_appends_with_defaults_and_enters_record_mode() {
        let mut panel = SettingsPanel::default();
        panel.keybindings.clear();
        panel.dirty = false;
        panel.add_key_binding();
        assert_eq!(panel.keybindings.len(), 1);
        assert_eq!(panel.keybindings[0].key, "");
        assert_eq!(panel.keybindings[0].action, KEYBINDING_ACTIONS[0]);
        assert_eq!(panel.selected_key_index, 0);
        assert_eq!(panel.key_field_focus, 1);
        assert!(panel.is_key_recording());
        assert!(panel.dirty);
    }

    #[test]
    fn add_key_binding_extends_existing_list() {
        let mut panel = panel_with_one_binding();
        panel.add_key_binding();
        assert_eq!(panel.keybindings.len(), 2);
        assert_eq!(panel.selected_key_index, 1);
        assert!(panel.is_key_recording());
    }

    #[test]
    fn open_key_delete_dialog_noop_when_empty() {
        let mut panel = SettingsPanel::default();
        panel.keybindings.clear();
        panel.open_key_delete_dialog();
        assert!(
            !panel.key_delete_dialog_open,
            "must not open dialog for empty list"
        );
    }

    #[test]
    fn open_key_delete_dialog_defaults_to_cancel_focus() {
        let mut panel = panel_with_one_binding();
        panel.open_key_delete_dialog();
        assert!(panel.key_delete_dialog_open);
        assert!(
            !panel.key_delete_dialog_confirm_focused,
            "default focus must be Cancel (accident guard)"
        );
    }

    #[test]
    fn cancel_key_delete_dialog_clears_state_and_keeps_binding() {
        let mut panel = panel_with_one_binding();
        panel.open_key_delete_dialog();
        panel.key_delete_dialog_confirm_focused = true;
        panel.cancel_key_delete_dialog();
        assert_eq!(panel.keybindings.len(), 1);
        assert!(!panel.key_delete_dialog_open);
        assert!(!panel.key_delete_dialog_confirm_focused);
    }

    #[test]
    fn confirm_key_delete_dialog_removes_at_end_clamps_to_prev() {
        let mut panel = panel_with_one_binding();
        panel.add_key_binding();
        // selected_key_index is now 1 (last). Delete it.
        panel.open_key_delete_dialog();
        panel.confirm_key_delete_dialog();
        assert_eq!(panel.keybindings.len(), 1);
        // Selection clamps to n-1 = 0.
        assert_eq!(panel.selected_key_index, 0);
        assert!(!panel.key_delete_dialog_open);
    }

    #[test]
    fn confirm_key_delete_dialog_in_middle_keeps_index() {
        let mut panel = panel_with_one_binding();
        panel.add_key_binding();
        panel.add_key_binding();
        // Three entries; select middle (index 1).
        panel.selected_key_index = 1;
        panel.open_key_delete_dialog();
        panel.confirm_key_delete_dialog();
        assert_eq!(panel.keybindings.len(), 2);
        // Middle delete shifts later entries up; selection stays at 1.
        assert_eq!(panel.selected_key_index, 1);
    }

    #[test]
    fn confirm_key_delete_dialog_emptying_resets_focus() {
        let mut panel = panel_with_one_binding();
        panel.key_field_focus = 4;
        panel.open_key_delete_dialog();
        panel.confirm_key_delete_dialog();
        assert!(panel.keybindings.is_empty());
        assert_eq!(panel.selected_key_index, 0);
        assert_eq!(
            panel.key_field_focus, 0,
            "empty list must restore ListBox focus"
        );
    }

    #[test]
    fn toggle_key_delete_dialog_focus_alternates() {
        let mut panel = panel_with_one_binding();
        panel.open_key_delete_dialog();
        assert!(!panel.key_delete_dialog_confirm_focused);
        panel.toggle_key_delete_dialog_focus();
        assert!(panel.key_delete_dialog_confirm_focused);
        panel.toggle_key_delete_dialog_focus();
        assert!(!panel.key_delete_dialog_confirm_focused);
    }

    #[test]
    fn close_panel_resets_key_delete_dialog() {
        let mut panel = panel_with_one_binding();
        panel.open_key_delete_dialog();
        panel.key_delete_dialog_confirm_focused = true;
        panel.close();
        assert!(!panel.key_delete_dialog_open);
        assert!(!panel.key_delete_dialog_confirm_focused);
    }
}

/// Phase 3 (UI 4-tasks, 2026-06-12): apply a drag-to-move offset to the panel's
/// centered base position, clamping the result so the title bar is always
/// reachable. Pure function — covered by the tests in `panel_drag_tests`.
///
/// `base_x` / `base_y` are the panel's default centered top-left in pixels.
/// `offset` is the cumulative drag delta from `SettingsPanel.drag_offset`.
/// `panel_w` / `panel_h` size the panel; `sw` / `sh` size the window;
/// `title_h` is the title-bar height (the portion that must stay onscreen
/// vertically so the user can always grab it back). The clamp keeps the panel
/// fully visible horizontally (`0 ..= sw - panel_w`) and ensures at least the
/// title bar height stays inside the window vertically (`0 ..= sh - title_h`).
#[allow(clippy::too_many_arguments)]
pub fn clamp_panel_position(
    base_x: f32,
    base_y: f32,
    panel_w: f32,
    _panel_h: f32,
    sw: f32,
    sh: f32,
    title_h: f32,
    offset: (f32, f32),
) -> (f32, f32) {
    let raw_x = base_x + offset.0;
    let raw_y = base_y + offset.1;
    let max_x = (sw - panel_w).max(0.0);
    let max_y = (sh - title_h).max(0.0);
    (raw_x.clamp(0.0, max_x), raw_y.clamp(0.0, max_y))
}

#[cfg(test)]
mod search_tests {
    //! Phase 4 (UI/UX v2): category-search fuzzy-filter tests.
    use super::*;

    /// Empty / blank queries must return every category in canonical order so
    /// the sidebar behaves identically to the pre-Phase-4 build.
    #[test]
    fn empty_query_returns_canonical_order() {
        let out = filter_categories("", SettingsCategory::ALL);
        assert_eq!(out, SettingsCategory::ALL.to_vec());
        let out = filter_categories("   ", SettingsCategory::ALL);
        assert_eq!(out, SettingsCategory::ALL.to_vec());
    }

    /// Exact-label matches must rank the target category first.
    #[test]
    fn exact_label_match_ranks_first() {
        let out = filter_categories("Theme", SettingsCategory::ALL);
        assert!(!out.is_empty());
        assert_eq!(out[0], SettingsCategory::Theme);
    }

    /// Keyword matches (color → Theme) prove the synonym table is consulted.
    #[test]
    fn keyword_match_finds_synonym() {
        let out = filter_categories("color", SettingsCategory::ALL);
        assert!(
            out.contains(&SettingsCategory::Theme),
            "color should match Theme via keyword, got {:?}",
            out
        );
        let out = filter_categories("shell", SettingsCategory::ALL);
        assert!(
            out.contains(&SettingsCategory::Profiles),
            "shell should match Profiles via keyword, got {:?}",
            out
        );
    }

    /// Queries with no fuzzy hit anywhere produce an empty result (so the
    /// sidebar collapses to "no matches" instead of silently showing
    /// everything).
    #[test]
    fn unmatched_query_returns_empty() {
        let out = filter_categories("xyzqq_nomatch", SettingsCategory::ALL);
        assert!(out.is_empty(), "expected empty, got {:?}", out);
    }

    /// Wiring sanity check: the struct method routes to the free helper and
    /// returns the same result.
    #[test]
    fn struct_method_matches_helper() {
        let panel = SettingsPanel {
            search_query: "block".to_string(),
            ..SettingsPanel::default()
        };
        let via_method = panel.filtered_categories();
        let via_helper = filter_categories("block", SettingsCategory::ALL);
        assert_eq!(via_method, via_helper);
    }

    /// Activation toggles must move `search_focused` without mutating the
    /// query (so the user can defocus to hit Tab then refocus later).
    #[test]
    fn focus_helpers_preserve_query() {
        let mut panel = SettingsPanel::default();
        panel.push_search_char('c');
        panel.push_search_char('o');
        panel.unfocus_search();
        assert!(!panel.search_focused);
        assert_eq!(panel.search_query, "co");
        panel.focus_search();
        assert!(panel.search_focused);
        assert_eq!(panel.search_query, "co");
        panel.clear_search();
        assert!(!panel.search_focused);
        assert!(panel.search_query.is_empty());
    }

    /// Control characters must not land in the query (regression guard for
    /// when the keyboard handler forwards Enter / Backspace as text).
    #[test]
    fn control_chars_are_skipped() {
        let mut panel = SettingsPanel::default();
        panel.push_search_char('\n');
        panel.push_search_char('\t');
        panel.push_search_char('a');
        assert_eq!(panel.search_query, "a");
    }

    // ---- Phase 4b: field-level search tests ----

    /// Every category must declare at least one field; otherwise the
    /// hit-count badge would never fire for that category.
    #[test]
    fn every_category_has_at_least_one_field() {
        for cat in SettingsCategory::ALL {
            let fields = category_fields(cat);
            assert!(
                !fields.is_empty(),
                "category {:?} has no searchable fields",
                cat
            );
        }
    }

    /// Searching for a field label (e.g. "Opacity") must hit the
    /// matching category through `filter_categories` even when the
    /// category label itself does not contain the query.
    #[test]
    fn field_label_match_finds_category() {
        let out = filter_categories("opacity", SettingsCategory::ALL);
        assert!(
            out.contains(&SettingsCategory::Window),
            "opacity should reach Window via field label, got {:?}",
            out
        );
    }

    /// Aliases declared on `FieldEntry` (e.g. "bash" for Profiles
    /// command) must also route the query to the right category.
    #[test]
    fn field_alias_match_finds_category() {
        let out = filter_categories("bash", SettingsCategory::ALL);
        assert!(
            out.contains(&SettingsCategory::Profiles),
            "bash should reach Profiles via the command field alias, got {:?}",
            out
        );
    }

    /// `field_hit_count` must return 0 for empty / blank queries so the
    /// sidebar can suppress the badge entirely when the user has not
    /// typed anything.
    #[test]
    fn field_hit_count_is_zero_for_empty_query() {
        for cat in SettingsCategory::ALL {
            assert_eq!(field_hit_count(cat, ""), 0);
            assert_eq!(field_hit_count(cat, "   "), 0);
        }
    }

    /// Hit count must be positive on the matching category and zero on
    /// an unrelated one, so the badge appears only where useful.
    #[test]
    fn field_hit_count_is_positive_for_matching_category() {
        let n = field_hit_count(&SettingsCategory::Window, "opacity");
        assert!(n >= 1, "expected ≥1 hit on Window for 'opacity', got {}", n);
        assert_eq!(
            field_hit_count(&SettingsCategory::Ssh, "opacity"),
            0,
            "SSH should not match 'opacity'"
        );
    }

    /// Hit count must rise when the query is broad enough to match
    /// multiple fields inside the same category (regression guard so we
    /// do not collapse to bool semantics).
    #[test]
    fn field_hit_count_aggregates_multiple_fields() {
        // "padding" appears in two field labels (Horizontal padding /
        // Vertical padding) inside Window.
        let n = field_hit_count(&SettingsCategory::Window, "padding");
        assert!(
            n >= 2,
            "expected ≥2 hits on Window for 'padding', got {}",
            n
        );
    }

    /// `SettingsPanel::field_hit_count` must agree with the free helper
    /// (wiring sanity check, mirrors `struct_method_matches_helper`).
    #[test]
    fn field_hit_count_struct_method_matches_helper() {
        let panel = SettingsPanel {
            search_query: "padding".to_string(),
            ..SettingsPanel::default()
        };
        let via_method = panel.field_hit_count(&SettingsCategory::Window);
        let via_helper = field_hit_count(&SettingsCategory::Window, "padding");
        assert_eq!(via_method, via_helper);
    }
}

#[cfg(test)]
mod theme_preview_tests {
    //! Phase 3b (UI/UX v2): live theme preview helpers.
    use super::*;
    use nexterm_config::BuiltinScheme;

    /// `index_to_builtin_scheme` must round-trip with the existing
    /// `scheme_name_to_index` inverse for every slot 0..=8 so the live
    /// preview cannot select a scheme that the commit path then drops.
    #[test]
    fn index_to_scheme_round_trips_with_name_to_index() {
        for idx in 0..9 {
            let scheme = index_to_builtin_scheme(idx);
            let back = scheme_name_to_index(&nexterm_config::ColorScheme::Builtin(scheme));
            assert_eq!(back, idx, "round-trip mismatch at idx={}", idx);
        }
    }

    /// Out-of-range inputs must wrap modulo 9 rather than panic — the
    /// renderer passes the field value verbatim and we don't want
    /// stray hover state to crash the frame.
    #[test]
    fn index_to_scheme_wraps_out_of_range() {
        assert_eq!(index_to_builtin_scheme(9), BuiltinScheme::Dark);
        assert_eq!(index_to_builtin_scheme(17), BuiltinScheme::OneDark);
        assert_eq!(
            index_to_builtin_scheme(usize::MAX),
            index_to_builtin_scheme(usize::MAX % 9)
        );
    }

    /// A fresh panel must start with no hover preview so the first
    /// open frame uses the configured scheme, not a stale value left
    /// over from a previous session.
    #[test]
    fn fresh_panel_has_no_hover_preview() {
        let panel = SettingsPanel::default();
        assert_eq!(panel.theme_hover_preview, None);
    }

    /// `close()` must drop any in-flight hover preview so the next
    /// open starts on the configured scheme even when the user
    /// dismissed the panel mid-hover.
    #[test]
    fn close_clears_hover_preview() {
        let mut panel = SettingsPanel {
            is_open: true,
            theme_hover_preview: Some(3),
            ..SettingsPanel::default()
        };
        panel.close();
        assert_eq!(panel.theme_hover_preview, None);
        assert!(!panel.is_open);
    }

    /// Commit (setting `scheme_index` from a click handler) must NOT
    /// rely on `theme_hover_preview` being kept in sync — the commit
    /// path moves the value into `scheme_index`, and 3b's renderer
    /// then falls back to the configured scheme on the next frame.
    /// This guards the click handler's "clear after commit" semantics.
    #[test]
    fn scheme_index_is_independent_of_preview() {
        let panel = SettingsPanel {
            theme_hover_preview: Some(5),
            scheme_index: 2,
            ..SettingsPanel::default()
        };
        assert_eq!(panel.scheme_index, 2);
        assert_eq!(panel.theme_hover_preview, Some(5));
    }
}

#[cfg(test)]
mod panel_drag_tests {
    use super::*;

    /// Default-size panel with a zero offset must render at its base center.
    #[test]
    fn zero_offset_returns_base_position() {
        let (x, y) =
            clamp_panel_position(100.0, 80.0, 800.0, 600.0, 1280.0, 800.0, 30.0, (0.0, 0.0));
        assert_eq!(x, 100.0);
        assert_eq!(y, 80.0);
    }

    /// A small positive offset moves the panel without hitting any clamp.
    #[test]
    fn small_offset_applies_directly() {
        let (x, y) =
            clamp_panel_position(100.0, 80.0, 800.0, 600.0, 1280.0, 800.0, 30.0, (50.0, 25.0));
        assert_eq!(x, 150.0);
        assert_eq!(y, 105.0);
    }

    /// A huge positive offset must clamp at the right/bottom such that the
    /// panel is still fully visible horizontally and the title bar still fits.
    #[test]
    fn large_positive_offset_clamps_to_right_and_bottom() {
        let (x, y) = clamp_panel_position(
            100.0,
            80.0,
            800.0,
            600.0,
            1280.0,
            800.0,
            30.0,
            (9999.0, 9999.0),
        );
        // sw - panel_w = 1280 - 800 = 480
        assert_eq!(x, 480.0);
        // sh - title_h = 800 - 30 = 770
        assert_eq!(y, 770.0);
    }

    /// A large negative offset cannot push the panel past x=0 / y=0.
    #[test]
    fn large_negative_offset_clamps_to_origin() {
        let (x, y) = clamp_panel_position(
            100.0,
            80.0,
            800.0,
            600.0,
            1280.0,
            800.0,
            30.0,
            (-9999.0, -9999.0),
        );
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
    }

    /// When the panel is wider than the window, the clamp range collapses to
    /// `0..=0` and the panel pins to the left edge (never disappears off-screen).
    #[test]
    fn panel_wider_than_window_pins_to_left() {
        let (x, _) =
            clamp_panel_position(0.0, 80.0, 1500.0, 600.0, 1280.0, 800.0, 30.0, (200.0, 0.0));
        assert_eq!(x, 0.0);
    }

    /// `start_drag` → `update_drag` → `end_drag` records the expected offset
    /// and clears the anchor on release.
    #[test]
    fn start_update_end_drag_records_offset() {
        let config = nexterm_config::Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert_eq!(panel.drag_offset, (0.0, 0.0));
        assert!(!panel.is_dragging());

        // Press at (200, 100).
        panel.start_drag(200.0, 100.0);
        assert!(panel.is_dragging());

        // Move the cursor to (260, 130) → offset must be (60, 30).
        panel.update_drag(260.0, 130.0);
        assert_eq!(panel.drag_offset, (60.0, 30.0));

        // Release: offset persists but drag is no longer in flight.
        panel.end_drag();
        assert!(!panel.is_dragging());
        assert_eq!(panel.drag_offset, (60.0, 30.0));
    }

    /// Calling `update_drag` without a prior `start_drag` is a no-op.
    /// (Defensive check for the mouse-move path which fires unconditionally.)
    #[test]
    fn update_drag_without_start_is_noop() {
        let config = nexterm_config::Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.update_drag(500.0, 500.0);
        assert_eq!(panel.drag_offset, (0.0, 0.0));
    }

    /// A second drag starting from an already-offset panel must compound the
    /// previous offset rather than reset it.
    #[test]
    fn second_drag_compounds_previous_offset() {
        let config = nexterm_config::Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.start_drag(100.0, 100.0);
        panel.update_drag(150.0, 120.0);
        panel.end_drag();
        assert_eq!(panel.drag_offset, (50.0, 20.0));

        // Second drag from (300, 300) to (320, 310) → +20, +10.
        panel.start_drag(300.0, 300.0);
        panel.update_drag(320.0, 310.0);
        panel.end_drag();
        assert_eq!(panel.drag_offset, (70.0, 30.0));
    }

    /// `close()` must zero the offset (next open returns to centered).
    #[test]
    fn close_resets_drag_offset() {
        let config = nexterm_config::Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.start_drag(0.0, 0.0);
        panel.update_drag(123.0, 45.0);
        panel.close();
        assert_eq!(panel.drag_offset, (0.0, 0.0));
        assert!(!panel.is_dragging());
    }
}
