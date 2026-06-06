//! Pane rendering state — `PaneState`, placed images, floating pane geometry
//!
//! Extracted from `state/mod.rs`:
//! - `FloatRect` — floating pane position and size
//! - `PlacedImage` — Sixel / Kitty placed image metadata + RGBA
//! - `PaneState` — per-pane state bundling grid / cursor / scrollback / images / prompt anchors

use std::collections::HashMap;

use nexterm_proto::Grid;

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
    /// Current working directory reported via OSC 7 (Sprint 5-2 / B2)
    ///
    /// Updated when the shell emits `printf '\\033]7;file://...' "$PWD"` or similar.
    /// Used for tab tooltips and inheriting the parent CWD when creating a new pane.
    /// `None` if OSC 7 has never been received.
    pub cwd: Option<String>,
    /// Records the scrollback length at the moment an OSC 133 A (PromptStart) mark arrives (Sprint 5-2 / B1)
    ///
    /// Expressed in the same "row index inside scrollback" space as `scroll_offset`.
    /// `jump_prev_prompt` / `jump_next_prompt` traverse this list to jump between prompts.
    /// This is approximate and can drift slightly across redraws and resizes.
    pub prompt_anchors: Vec<usize>,
    /// True when grid content has changed since the last vertex build.
    ///
    /// Set on `FullRefresh` / `apply_diff`. Cleared by the renderer after building
    /// vertex data for this pane. Used by the partial-redraw cache (C4).
    pub content_dirty: bool,
}

impl PaneState {
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
            cwd: None,
            prompt_anchors: Vec::new(),
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
