//! Pane rendering state — `PaneState`, placed images, floating pane geometry
//!
//! Extracted from `state/mod.rs`:
//! - `FloatRect` — floating pane position and size
//! - `PlacedImage` — Sixel / Kitty placed image metadata + RGBA
//! - `PaneState` — per-pane state bundling grid / cursor / scrollback / images / prompt anchors

use std::collections::HashMap;

use nexterm_proto::Grid;

use crate::command_blocks::{CommandBlock, SemanticMark};
use crate::scrollback::Scrollback;

/// Floating pane position and size information
#[derive(Clone, Debug)]
pub struct FloatRect {
    #[allow(dead_code)]
    pub col_off: u16,
    #[allow(dead_code)]
    pub row_off: u16,
    #[allow(dead_code)]
    pub cols: u16,
    #[allow(dead_code)]
    pub rows: u16,
}

/// OSC 66 placed text-sizing overlay (Kitty Text Sizing Protocol).
pub struct PlacedTextSize {
    pub col: u16,
    pub row: u16,
    pub scale_num: u8,
    pub scale_den: u8,
    pub width_cells: u16,
    // Alignment hints from the protocol; reserved for future rendering use.
    #[allow(dead_code)]
    pub valign: u8,
    #[allow(dead_code)]
    pub halign: u8,
    pub text: String,
}

/// Placed image
pub struct PlacedImage {
    pub col: u16,
    pub row: u16,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Dynamic-color overrides applied via OSC 4/10/11 (roadmap #10b).
///
/// Received as full snapshots (`ServerToClient::PaneColorsChanged`) and
/// consulted by `color_util::resolve_color_with_overrides` ahead of the
/// scheme palette during vertex construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneColorOverrides {
    /// OSC 10 dynamic foreground (`None` = theme default).
    pub fg: Option<[u8; 3]>,
    /// OSC 11 dynamic background (`None` = theme default).
    pub bg: Option<[u8; 3]>,
    /// OSC 4 palette overrides keyed by palette index.
    pub palette: HashMap<u8, [u8; 3]>,
}

impl PaneColorOverrides {
    /// True when no override is active (the common case — lets the renderer
    /// skip the override lookups entirely).
    pub fn is_empty(&self) -> bool {
        self.fg.is_none() && self.bg.is_none() && self.palette.is_empty()
    }
}

/// Pane rendering state
pub struct PaneState {
    pub grid: Grid,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub scrollback: Scrollback,
    /// Scrollback offset (0 = latest screen)
    pub scroll_offset: usize,
    /// Placed images (image_id → PlacedImage)
    pub images: HashMap<u32, PlacedImage>,
    /// OSC 66 text-sizing overlays pending render.
    pub text_sizes: Vec<PlacedTextSize>,
    /// Background activity flag (true when output arrives while not focused)
    pub has_activity: bool,
    /// Title set via OSC 0/2 (shells and vim set the window title)
    pub title: String,
    /// Phase 2c (UI/UX v2): foreground process name reported by the
    /// server's 1 Hz polling ticker (`ServerToClient::ProcessChanged`).
    /// `None` when the shell is at the prompt, detection failed, or
    /// the OS does not support process inspection (e.g. BSDs). The
    /// tab-bar renderer maps this to a Nerd Font glyph through
    /// [`crate::tab_icons::glyph_for_process`].
    pub process_name: Option<String>,
    /// Current working directory reported via OSC 7 (Sprint 5-2 / B2)
    ///
    /// Updated when the shell emits `printf '\\033]7;file://...' "$PWD"` or similar.
    /// Used for tab tooltips and inheriting the parent CWD when creating a new pane.
    /// `None` if OSC 7 has never been received.
    pub cwd: Option<String>,
    /// Pointer shape requested via OSC 22 (PROTOCOL_VERSION 10).
    ///
    /// `None` means "no override" (platform default). The renderer applies it
    /// through [`pointer_shape_to_cursor_icon`] while the cursor hovers the
    /// grid area; unknown names fall back to the default icon there.
    pub pointer_shape: Option<String>,
    /// Dynamic-color overrides from OSC 4/10/11 (roadmap #10b).
    pub color_overrides: PaneColorOverrides,
    /// OSC 9;4 progress indicator as `(state, percent)` — `None` when no
    /// progress is being reported (state 0 clears it). State 1 = normal,
    /// 2 = error, 3 = indeterminate, 4 = paused. Rendered in the tab bar.
    pub progress: Option<(u8, u8)>,
    /// Records the scrollback length at the moment an OSC 133 A (PromptStart) mark arrives (Sprint 5-2 / B1)
    ///
    /// Expressed in the same "row index inside scrollback" space as `scroll_offset`.
    /// `jump_prev_prompt` / `jump_next_prompt` traverse this list to jump between prompts.
    /// This is approximate and can drift slightly across redraws and resizes.
    pub prompt_anchors: Vec<usize>,
    /// OSC 133 semantic-mark accumulator (Phase 1 of the command-blocks feature).
    ///
    /// Each entry is recorded with `row` set to the scrollback-absolute index at
    /// the moment the mark arrived, mirroring `prompt_anchors`. The accumulator
    /// is bounded by [`Self::MAX_SEMANTIC_MARKS`]; once exceeded the oldest
    /// `MAX_SEMANTIC_MARKS / 4` entries are dropped so that `BlockId`s for
    /// recently-named blocks stay stable for as long as practical.
    pub marks: Vec<SemanticMark>,
    /// Derived view computed by [`crate::command_blocks::extract_command_blocks`]
    /// over [`Self::marks`]. Recomputed whenever a new mark arrives.
    pub blocks: Vec<CommandBlock>,
    /// True when grid content has changed since the last vertex build.
    ///
    /// Set on `FullRefresh` / `apply_diff`. Cleared by the renderer after building
    /// vertex data for this pane. Used by the partial-redraw cache (C4).
    pub content_dirty: bool,
}

impl PaneState {
    /// Soft cap on the OSC 133 mark accumulator (memory-DoS guard).
    ///
    /// When the buffer grows past this, the oldest quarter is dropped. The cap
    /// is generous enough (8 192 marks ≈ 2 048 complete blocks) that real
    /// sessions never hit it, while keeping a hostile shell from exhausting
    /// memory.
    pub const MAX_SEMANTIC_MARKS: usize = 8192;

    // Sprint 5-11-2 Step 2-1: widened from `pub(super)` to `pub(crate)` so that
    // `accessibility::tests` can build panes manually. Production code only
    // invokes this via `apply_server_message` (state/server_message.rs).
    pub(crate) fn new(cols: u16, rows: u16, scrollback_capacity: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            cursor_col: 0,
            cursor_row: 0,
            scrollback: Scrollback::new(scrollback_capacity),
            scroll_offset: 0,
            images: HashMap::new(),
            text_sizes: Vec::new(),
            has_activity: false,
            title: String::new(),
            process_name: None,
            cwd: None,
            pointer_shape: None,
            color_overrides: PaneColorOverrides::default(),
            progress: None,
            prompt_anchors: Vec::new(),
            marks: Vec::new(),
            blocks: Vec::new(),
            content_dirty: true,
        }
    }

    pub(super) fn apply_diff(
        &mut self,
        dirty_rows: Vec<nexterm_proto::DirtyRow>,
        cursor_col: u16,
        cursor_row: u16,
    ) {
        for dirty in dirty_rows {
            if let Some(row) = self.grid.rows.get_mut(dirty.row as usize) {
                // Push the pre-scrollout row onto the scrollback
                self.scrollback.push_line(row.clone());
                *row = dirty.cells;
            }
        }
        self.cursor_col = cursor_col;
        self.cursor_row = cursor_row;
        // New output arrived, so snap back to the latest screen
        self.scroll_offset = 0;
        self.content_dirty = true;
    }
}

/// Maps an OSC 22 pointer-shape name onto a winit cursor icon.
///
/// Names follow the CSS cursor keywords, which is what xterm, WezTerm, and
/// kitty accept. Unknown names fall back to the platform default.
pub fn pointer_shape_to_cursor_icon(name: &str) -> winit::window::CursorIcon {
    use winit::window::CursorIcon;
    match name {
        "text" => CursorIcon::Text,
        "pointer" | "hand" => CursorIcon::Pointer,
        "crosshair" => CursorIcon::Crosshair,
        "wait" => CursorIcon::Wait,
        "progress" => CursorIcon::Progress,
        "help" => CursorIcon::Help,
        "not-allowed" => CursorIcon::NotAllowed,
        "no-drop" => CursorIcon::NoDrop,
        "move" => CursorIcon::Move,
        "grab" => CursorIcon::Grab,
        "grabbing" => CursorIcon::Grabbing,
        "cell" => CursorIcon::Cell,
        "vertical-text" => CursorIcon::VerticalText,
        "alias" => CursorIcon::Alias,
        "copy" => CursorIcon::Copy,
        "zoom-in" => CursorIcon::ZoomIn,
        "zoom-out" => CursorIcon::ZoomOut,
        "col-resize" => CursorIcon::ColResize,
        "row-resize" => CursorIcon::RowResize,
        "e-resize" => CursorIcon::EResize,
        "w-resize" => CursorIcon::WResize,
        "n-resize" => CursorIcon::NResize,
        "s-resize" => CursorIcon::SResize,
        "ne-resize" => CursorIcon::NeResize,
        "nw-resize" => CursorIcon::NwResize,
        "se-resize" => CursorIcon::SeResize,
        "sw-resize" => CursorIcon::SwResize,
        "ew-resize" => CursorIcon::EwResize,
        "ns-resize" => CursorIcon::NsResize,
        _ => CursorIcon::Default,
    }
}

#[cfg(test)]
mod pointer_shape_tests {
    use super::pointer_shape_to_cursor_icon;
    use winit::window::CursorIcon;

    #[test]
    fn maps_common_css_cursor_names() {
        assert_eq!(pointer_shape_to_cursor_icon("text"), CursorIcon::Text);
        assert_eq!(pointer_shape_to_cursor_icon("pointer"), CursorIcon::Pointer);
        // xterm's historical alias for pointer.
        assert_eq!(pointer_shape_to_cursor_icon("hand"), CursorIcon::Pointer);
        assert_eq!(pointer_shape_to_cursor_icon("wait"), CursorIcon::Wait);
        assert_eq!(
            pointer_shape_to_cursor_icon("ns-resize"),
            CursorIcon::NsResize
        );
    }

    #[test]
    fn unknown_names_fall_back_to_default() {
        assert_eq!(pointer_shape_to_cursor_icon(""), CursorIcon::Default);
        assert_eq!(pointer_shape_to_cursor_icon("default"), CursorIcon::Default);
        assert_eq!(
            pointer_shape_to_cursor_icon("no-such-shape"),
            CursorIcon::Default
        );
    }
}
