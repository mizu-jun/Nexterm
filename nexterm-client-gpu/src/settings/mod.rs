//! Settings panel — Ctrl+, opens the floating UI (multi-category layout
//! with left sidebar).
//!
//! `SettingsPanel` itself, plus lifecycle (open/close) and category/tab
//! navigation and window-tab renaming live here. Title-bar drag lives in
//! `drag.rs`. Per-category state and behavior are split into sibling
//! modules (see below); this file re-exports their public types so
//! external call sites can keep using `crate::settings_panel::X`
//! unchanged (see the compatibility shim at `crate::settings_panel`).
//!
//! Split out of the former single-file `settings_panel.rs` (Phase B6
//! mechanical refactor). Pure code motion: no behavior changes.

mod category;
mod drag;
mod font;
mod hover;
mod keybindings;
mod keybindings_edit;
mod profiles;
mod reset;
mod row_filter;
mod save;
mod scroll;
mod security;
mod ssh;
mod startup;
mod text_input;
mod theme;
mod window;
mod window_extra;

pub use category::SettingsCategory;
pub use drag::clamp_panel_position;
pub use hover::HoverDwell;
pub use keybindings::{KEYBINDING_ACTIONS, KeyBindingEntry, KeyEditMode};
pub use profiles::ProfileEntry;
pub use row_filter::slot_of;
pub use scroll::ScrollState;
pub use ssh::SshHostEntry;
pub(crate) use ssh::write_ssh_hosts_back;
pub use startup::LANGUAGE_OPTIONS;
pub use text_input::TextInputState;
pub use theme::index_to_builtin_scheme;
use theme::scheme_name_to_index;

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
    /// UI/UX v3 P1b: the widget the pointer is resting on, with the dwell
    /// start time. Drives tooltips; `None` when the pointer is not over a
    /// widget of a migrated category.
    pub hover_widget: Option<HoverDwell>,
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
    /// Phase B1: vertical scroll state for the content area. Reset on
    /// category switch and panel close; `content_h_px` / `viewport_h_px`
    /// are recomputed by the renderer every frame.
    pub scroll: ScrollState,
    /// Phase 2c-G: read-only mirror of the `[blocks]` section. Populated at
    /// construction time so the Blocks settings page can display the active
    /// values; interactive editing lands in a follow-up.
    pub blocks_enabled: bool,
    pub blocks_border_width_px: u8,
    pub blocks_show_exit_code_badge: bool,
    /// `[security]` consent policies, edited via the Security category.
    /// On save each writes back to `[security].<key>` (allow / deny / prompt).
    pub sec_external_url: nexterm_config::ConsentPolicy,
    pub sec_osc52_clipboard: nexterm_config::ConsentPolicy,
    pub sec_osc_notification: nexterm_config::ConsentPolicy,
    pub sec_plugin_read: nexterm_config::ConsentPolicy,
    /// `[security]` byte-size caps, edited as decimal text (focus 4..=6).
    pub sec_osc52_max_bytes: usize,
    pub sec_notification_max_bytes: usize,
    pub sec_plugin_read_max_bytes: usize,
    /// UI/UX v3 P1c follow-up: index of the focused widget inside the current
    /// category, replacing the seven per-tab counters this used to be
    /// (`window_field_focus`, `ssh_field_focus`, …).
    ///
    /// The value is a `WidgetId.index` for `self.category`, so what it means is
    /// defined by that category's descriptor builder in
    /// `renderer/overlay/widgets/settings_<tab>.rs` — its `row` constants where
    /// it has them, otherwise the order in which it pushes descriptors. The
    /// list-shaped tabs (Ssh, Keybindings) reserve 0 for the entry list itself;
    /// which entry is selected lives in `selected_host_index` /
    /// `selected_key_index`, because that outlives focus moving to the fields.
    ///
    /// Reset by [`Self::set_category`] on every category change: an index is
    /// only meaningful together with the category it was resolved against.
    pub focused_widget_index: u16,
    /// In-flight decimal edit for the Security byte-cap fields
    /// (Security `focused_widget_index` 4..=6).
    /// `Some` = edit mode on; Enter commits, Esc cancels.
    pub security_field_editing: Option<TextInputState>,
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
    /// Phase 5-11-8 Step 8-3 (Sub-phase A): in-flight SSH host field edit state.
    /// `Some(state)` = edit mode is on; `None` = off. Corresponds to
    /// Ssh `focused_widget_index` values 1/2/4 (name/host/username). Enter starts the
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

    // ===== Phase B4: additional Window-category fields =====
    /// `[cursor].blink_enabled` mirror. On save writes back to `[cursor].blink_enabled`.
    pub cursor_blink_enabled: bool,

    // ===== P2b: in-app acrylic blur toggle/strength =====
    /// `[window].in_app_blur_enabled` mirror.
    pub in_app_blur_enabled: bool,
    /// `[window].in_app_blur_strength` mirror (0.0..=1.0, snapped to 5% steps).
    pub in_app_blur_strength: f32,
    /// `scrollback_lines` mirror (top-level key). Adjusted in 1000-line steps,
    /// clamped to `100..=1_000_000`.
    pub scrollback_lines: usize,
    /// `[tab_bar].show_tab_number` mirror.
    pub tab_show_tab_number: bool,
    /// `[tab_bar].show_new_tab_button` mirror.
    pub tab_show_new_tab_button: bool,
    /// `[animations].enabled` mirror.
    pub animations_enabled: bool,
    /// `[animations].intensity` mirror.
    pub animations_intensity: nexterm_config::AnimationIntensity,

    // ===== Phase B4-P2: additional Window-category fields =====
    /// `[window].decorations` mirror. Cycled via ←/→ at Window `focused_widget_index == 11`.
    pub window_decorations: nexterm_config::WindowDecorations,
    /// `[window].close_action` mirror. Cycled via ←/→ at Window `focused_widget_index == 12`.
    pub window_close_action: nexterm_config::CloseAction,
    /// `[gpu].fps_limit` mirror. 0 = unlimited. Adjusted in 10-fps steps,
    /// clamped to `0..=480`, at Window `focused_widget_index == 13`.
    pub fps_limit: u32,

    // ===== Phase B4-P2: Theme-category fields =====
    /// `colors_follow_system` mirror (top-level key).
    pub colors_follow_system: bool,

    // ===== Phase B4-P2: Font-category fields =====
    /// `[font].ligatures` mirror.
    pub font_ligatures: bool,
    /// `[font].font_fallbacks` mirror, joined with `", "` for display/editing
    /// (split back on `,` on save; each entry trimmed; empty entries dropped).
    pub font_fallbacks_text: String,
    /// In-flight edit buffer for `font_fallbacks_text` (`None` = not editing).
    pub font_fallbacks_editing: Option<TextInputState>,

    // ===== Phase B4-P2: Keybindings-category leader key field =====
    /// `leader_key` mirror (top-level key), editable at Keybindings `focused_widget_index == 5`.
    pub leader_key: String,
    /// In-flight edit buffer for `leader_key` (`None` = not editing).
    pub leader_key_editing: Option<TextInputState>,

    // ===== Phase B4: Startup-category shell fields =====
    /// `[shell].program` mirror (editable via Startup `focused_widget_index == 2`).
    pub shell_program: String,
    /// `[shell].args` mirror, joined with a single space for editing
    /// (split back on whitespace on save). Editable via Startup `focused_widget_index == 3`.
    pub shell_args: String,
    /// In-flight edit buffer for the focused shell field (`None` = not editing).
    pub shell_field_editing: Option<TextInputState>,

    // ===== Phase B4: Profiles-category active-profile selector =====
    /// Index into `profiles` of the active profile, offset by 1 so that `0`
    /// means "no active profile" (matches `Config.active_profile == None`).
    pub active_profile_index: usize,
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
        // Phase B4: `active_profile_index == 0` means "none"; `i + 1` maps to
        // `profiles[i]`. Falls back to 0 when the configured name is stale
        // (e.g. the profile was removed from `nexterm.toml` externally).
        let active_profile_index = config
            .active_profile
            .as_ref()
            .and_then(|name| config.profiles.iter().position(|p| &p.name == name))
            .map(|i| i + 1)
            .unwrap_or(0);
        Self {
            is_open: false,
            open_progress: 0.0,
            drag_slider: None,
            category: SettingsCategory::Font,
            font_size: config.font.size,
            scheme_index,
            theme_hover_preview: None,
            hover_widget: None,
            opacity: config.window.background_opacity,
            dirty: false,
            font_family: config.font.family.clone(),
            font_family_editing: false,
            profiles,
            selected_profile: 0,
            ssh_hosts,
            selected_host_index: 0,
            focused_widget_index: 0,
            ssh_field_editing: None,
            ssh_delete_dialog_open: false,
            ssh_delete_dialog_confirm_focused: false,
            keybindings,
            selected_key_index: 0,
            key_editing: None,
            key_delete_dialog_open: false,
            key_delete_dialog_confirm_focused: false,
            startup_session: "main".to_string(),
            tab_rename_editing: None,
            tab_rename_text: String::new(),
            language_index,
            auto_check_update: config.auto_check_update,
            scroll: ScrollState::default(),
            blocks_enabled: config.blocks.enabled,
            blocks_border_width_px: config.blocks.border_width_px,
            blocks_show_exit_code_badge: config.blocks.show_exit_code_badge,
            sec_external_url: config.security.external_url,
            sec_osc52_clipboard: config.security.osc52_clipboard,
            sec_osc_notification: config.security.osc_notification,
            sec_plugin_read: config.security.plugin_read,
            sec_osc52_max_bytes: config.security.osc52_max_bytes,
            sec_notification_max_bytes: config.security.notification_max_bytes,
            sec_plugin_read_max_bytes: config.security.plugin_read_max_bytes,
            security_field_editing: None,
            cursor_style: config.cursor_style.clone(),
            // `padding_x` / `padding_y` are `u32` in the config but the UI
            // clamps them to 0..=32.
            padding_x: config.window.padding_x.min(32),
            padding_y: config.window.padding_y.min(32),
            present_mode: config.gpu.present_mode.clone(),
            // Phase 3 (UI 4-tasks): panel renders centered on first open.
            drag_offset: (0.0, 0.0),
            drag_anchor: None,
            // Phase 4 (UI/UX v2): start with no filter and search defocused.
            search_query: String::new(),
            search_focused: false,
            cursor_blink_enabled: config.cursor.blink_enabled,
            in_app_blur_enabled: config.window.in_app_blur_enabled,
            in_app_blur_strength: config.window.in_app_blur_strength,
            scrollback_lines: config.scrollback_lines,
            tab_show_tab_number: config.tab_bar.show_tab_number,
            tab_show_new_tab_button: config.tab_bar.show_new_tab_button,
            animations_enabled: config.animations.enabled,
            animations_intensity: config.animations.intensity.clone(),
            window_decorations: config.window.decorations.clone(),
            window_close_action: config.window.close_action,
            fps_limit: config.gpu.fps_limit,
            colors_follow_system: config.colors_follow_system,
            font_ligatures: config.font.ligatures,
            font_fallbacks_text: config.font.font_fallbacks.join(", "),
            font_fallbacks_editing: None,
            leader_key: config.leader_key.clone(),
            leader_key_editing: None,
            shell_program: config.shell.program.clone(),
            shell_args: config.shell.args.join(" "),
            shell_field_editing: None,
            active_profile_index,
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
        // Phase B1: start the next open unscrolled.
        self.scroll.reset();
        // Phase B4: also leave shell-field edit mode.
        self.shell_field_editing = None;
    }

    /// Switch to `cat`, resetting the state that only made sense in the
    /// category being left.
    ///
    /// Every category change goes through here — the arrow keys, the sidebar
    /// click and the search-filtered jump — so the reset cannot be forgotten on
    /// one path. Before the focus counters collapsed into
    /// `focused_widget_index`, the keyboard paths reset all seven by hand and
    /// the sidebar click reset none of them.
    pub fn set_category(&mut self, cat: SettingsCategory) {
        self.category = cat;
        // Phase B1: each category has its own content height, so the previous
        // scroll offset is meaningless in the new category.
        self.scroll.reset();
        // A widget index is only meaningful against the category it was
        // resolved against.
        self.focused_widget_index = 0;
        // Every field edit buffer is displayed only while
        // `focused_widget_index` matches its field, so one that outlived the
        // reset above would keep taking keystrokes while invisible.
        self.ssh_field_editing = None;
        self.security_field_editing = None;
        self.shell_field_editing = None;
        self.font_fallbacks_editing = None;
        self.leader_key_editing = None;
        self.font_family_editing = false;
        self.key_editing = None;
    }

    /// Move to the previous category in the sidebar.
    pub fn prev_category(&mut self) {
        let idx = Self::category_index(&self.category);
        let len = SettingsCategory::ALL.len();
        self.set_category(SettingsCategory::ALL[(idx + len - 1) % len].clone());
    }

    /// Move to the next category in the sidebar.
    pub fn next_category(&mut self) {
        let idx = Self::category_index(&self.category);
        self.set_category(SettingsCategory::ALL[(idx + 1) % SettingsCategory::ALL.len()].clone());
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

    #[test]
    fn category_switch_resets_scroll_offset() {
        // Phase B1: each category has its own content height, so a scroll
        // position from one category (e.g. deep in the Keybindings list)
        // must not leak into the next.
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.scroll.offset_px = 250.0;
        panel.next_category();
        assert_eq!(panel.scroll.offset_px, 0.0);

        panel.scroll.offset_px = 120.0;
        panel.prev_category();
        assert_eq!(panel.scroll.offset_px, 0.0);
    }

    /// The seven per-tab focus counters collapsed into one
    /// `focused_widget_index` (UI/UX v3 P1c follow-up), so a category change
    /// has to clear it: an index that addressed a Window row would otherwise
    /// point at nothing — or at an unrelated row — in the next category.
    #[test]
    fn category_change_resets_the_focused_widget() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);

        panel.focused_widget_index = 4;
        panel.next_category();
        assert_eq!(panel.focused_widget_index, 0);

        panel.focused_widget_index = 3;
        panel.prev_category();
        assert_eq!(panel.focused_widget_index, 0);
    }

    /// Leaving a category must drop the field edit buffers with it. Their
    /// display is gated on `focused_widget_index` matching the field, so an
    /// edit that survived a category change would stay live while being
    /// invisible — the user would type into a field they can no longer see.
    #[test]
    fn category_change_cancels_in_flight_field_edits() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);

        panel.ssh_field_editing = Some(TextInputState::new("half-typed".to_string()));
        panel.security_field_editing = Some(TextInputState::new("1024".to_string()));
        panel.shell_field_editing = Some(TextInputState::new("/bin/zsh".to_string()));
        panel.font_fallbacks_editing = Some(TextInputState::new("Noto".to_string()));
        panel.leader_key_editing = Some(TextInputState::new("ctrl+q".to_string()));
        panel.font_family_editing = true;
        panel.key_editing = Some(KeyEditMode::Record);

        panel.next_category();

        assert!(panel.ssh_field_editing.is_none());
        assert!(panel.security_field_editing.is_none());
        assert!(panel.shell_field_editing.is_none());
        assert!(panel.font_fallbacks_editing.is_none());
        assert!(panel.leader_key_editing.is_none());
        assert!(!panel.font_family_editing);
        assert!(panel.key_editing.is_none());
    }

    #[test]
    fn close_resets_scroll_offset() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.open();
        panel.scroll.offset_px = 80.0;
        panel.close();
        assert_eq!(panel.scroll.offset_px, 0.0);
    }
}
