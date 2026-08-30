//! The delete-confirmation modal, shared by the list-shaped tabs (UI/UX v3 N-4d).
//!
//! `ssh_tab.rs` and `keybindings_tab.rs` each had their own
//! `draw_delete_dialog`, one a copy of the other. Four differences had
//! accumulated between the copies, and none of them was a decision anyone made:
//!
//! 1. Two translation keys for the same button —
//!    `settings-dialog-cancel-plain` in SSH, `-bracketed` in Keybindings.
//! 2. Keybindings moved the cancel label `primary → secondary` when focus left
//!    it; SSH held `primary` in both states, so its cancel button signalled
//!    focus by background alone.
//! 3. The label sat at `cell_w * 0.5` in one and `cell_w` in the other, inside
//!    boxes that were the same width.
//! 4. SSH drew a hint line under the buttons; Keybindings drew nothing.
//!
//! Each resolves toward whichever copy was already right: one undecorated key,
//! Keybindings' focus colours, labels centred (so the offset ceases to exist),
//! and the hint optional.
//!
//! **The decoration was in the locale data, not just in the Rust.**
//! `"[ Cancel (Esc) ]"` and `"  Cancel (Esc)"` carried a button border and an
//! indent inside the translated string — stand-ins from when a cell was the
//! only unit of positioning available. `add_px_rect` draws the real border now,
//! and a two-space prefix moves a centred label off-centre by a cell, so the
//! decoration had to come out of all eight locales rather than out of this
//! file. It is the N-3 finding ("the label's decorative spaces are gone") one
//! layer further out.
//!
//! Button widths follow P4c: `measure_run(label) + padding`, with `n - 1` gaps
//! between `n` buttons. Fourteen cells fit `  Cancel (Esc)` and not
//! `[ Abbrechen (Esc) ]`.
//!
//! This stays hand-written rather than moving into `widgets/`: `CLAUDE.md`
//! records the decision that a modal over the panel is not a settings row.
//! N-4d de-duplicates it; it does not migrate it.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::vertex_util::{add_px_rect, add_run_verts, measure_run};

use super::super::util::{SCRIM_ALPHA_FLOOR, scrim_color};
use super::row::danger_button_colors;
use nexterm_config::SurfaceLevel;

/// What differs between the two tabs' modals — everything else is shared.
///
/// Strings rather than keys: `fl!` resolves at the call site, so a tab names
/// its own message without this module holding a table of which key belongs to
/// whom.
pub(super) struct DeleteDialogView {
    /// Localised title, e.g. "Delete this SSH host?".
    pub title: String,
    /// What is being deleted, interpolated into the shared confirm message.
    pub target: String,
    /// Localised label for the destructive button.
    pub confirm_label: String,
    /// Optional keyboard hint under the buttons. SSH has one; Keybindings does
    /// not, and whether it should is a UX question rather than a geometry one.
    pub hint: Option<String>,
    /// Whether the destructive button holds focus (`false` = Cancel does).
    pub confirm_focused: bool,
}

/// Horizontal padding inside a button, in cells. P4c's value for the consent
/// dialog, reused so the two look like each other.
const BTN_PAD_CELLS: f32 = 1.5;

/// Gap between the two buttons, in cells.
const BTN_GAP_CELLS: f32 = 2.0;

/// Draw the modal centred over the settings panel.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_delete_dialog(
    view: &DeleteDialogView,
    tokens: &nexterm_config::DesignTokens,
    px: f32,
    py: f32,
    panel_w: f32,
    panel_h: f32,
    sw: f32,
    sh: f32,
    cell_w: f32,
    cell_h: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    let metrics = nexterm_config::MetricTokens::default();
    let title_style = &metrics.type_ramp.title;
    let body_style = &metrics.type_ramp.body;
    let btn_style = &metrics.type_ramp.body_strong;
    let hint_style = &metrics.type_ramp.caption;

    add_px_rect(
        px,
        py,
        panel_w,
        panel_h,
        scrim_color(tokens, SCRIM_ALPHA_FLOOR),
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    let dialog_w = panel_w * 0.55;
    let dialog_h = cell_h * 8.5;
    let dialog_x = px + (panel_w - dialog_w) / 2.0;
    let dialog_y = py + (panel_h - dialog_h) / 2.0;

    // Danger ring, then the panel face.
    add_px_rect(
        dialog_x - 2.0,
        dialog_y - 2.0,
        dialog_w + 4.0,
        dialog_h + 4.0,
        {
            let [r, g, b, _] = tokens.semantic_error;
            [r, g, b, 0.80]
        },
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    add_px_rect(
        dialog_x,
        dialog_y,
        dialog_w,
        dialog_h,
        tokens.surface_0,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    add_run_verts(
        &view.title,
        title_style,
        dialog_x + cell_w,
        dialog_y + cell_h * 0.6,
        tokens.text_on(SurfaceLevel::S0).error,
        sw,
        sh,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let msg = nexterm_i18n::fl!("settings-delete-confirm-message", target = &*view.target);
    add_run_verts(
        &msg,
        body_style,
        dialog_x + cell_w,
        dialog_y + cell_h * 2.2,
        tokens.text_on(SurfaceLevel::S0).secondary,
        sw,
        sh,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    // Buttons. Widths come from the labels rather than a cell count, so a
    // translation cannot overflow its box (P4c).
    let cancel_label = nexterm_i18n::fl!("settings-dialog-cancel");
    let labels = [&cancel_label, &view.confirm_label];
    let pad = cell_w * BTN_PAD_CELLS;
    let gap = cell_w * BTN_GAP_CELLS;
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| measure_run(l, btn_style, font) + pad * 2.0)
        .collect();
    let total_w: f32 = widths.iter().sum::<f32>() + gap * (labels.len() - 1) as f32;

    let btn_h = cell_h * 1.4;
    let mut bx = dialog_x + (dialog_w - total_w) / 2.0;
    let by = dialog_y + dialog_h - cell_h * 2.5;
    let (_, btn_line_h, _) = font.chrome_metrics(btn_style);

    for (i, (label, &bw)) in labels.iter().zip(widths.iter()).enumerate() {
        let is_confirm = i == 1;
        let focused = view.confirm_focused == is_confirm;

        let (bg, fg) = if is_confirm {
            danger_button_colors(tokens, view.confirm_focused)
        } else {
            let bg = if focused {
                tokens.surface_3
            } else {
                tokens.surface_1
            };
            // Keybindings' behaviour: a button that shows focus only by its
            // background is the weaker of the two copies.
            let fg = if focused {
                tokens.text_on(SurfaceLevel::S3).primary
            } else {
                tokens.text_on(SurfaceLevel::S3).secondary
            };
            (bg, fg)
        };

        add_px_rect(bx, by, bw, btn_h, bg, sw, sh, bg_verts, bg_idx);
        // Centred, which is what removes the two copies' disagreement about
        // the label's x offset — and what the locale decoration would have
        // broken.
        let label_w = measure_run(label, btn_style, font);
        add_run_verts(
            label,
            btn_style,
            bx + (bw - label_w) * 0.5,
            by + (btn_h - btn_line_h) * 0.5,
            fg,
            sw,
            sh,
            font,
            atlas,
            queue,
            text_verts,
            text_idx,
        );
        bx += bw + gap;
    }

    if let Some(hint) = &view.hint {
        add_run_verts(
            hint,
            hint_style,
            dialog_x + cell_w,
            dialog_y + dialog_h - cell_h * 0.9,
            tokens.text_on(SurfaceLevel::S0).muted,
            sw,
            sh,
            font,
            atlas,
            queue,
            text_verts,
            text_idx,
        );
    }
}

#[cfg(test)]
mod tests {
    /// The two tabs describe the modal; neither draws one.
    ///
    /// The copies drifted in four places over the months they existed side by
    /// side (see this module's header). A second `draw_delete_dialog` is how
    /// that starts again.
    #[test]
    fn neither_tab_draws_its_own_delete_dialog() {
        for (name, src) in [
            ("ssh_tab.rs", include_str!("ssh_tab.rs")),
            ("keybindings_tab.rs", include_str!("keybindings_tab.rs")),
        ] {
            assert!(
                !src.contains("fn draw_delete_dialog"),
                "{name} defines its own delete modal again; delete_dialog owns it"
            );
            assert!(
                !src.contains("cell_w * 14.0"),
                "{name} sizes a dialog button by a cell count again; button \
                 widths come from measure_run (P4c)"
            );
        }
    }

    /// UI/UX v3 N-4e: the prose these two tabs own is on the ramp.
    ///
    /// The empty state, the list range-indicator and the edit-hint note were
    /// the last `add_string_verts` calls in either file. They are left-aligned
    /// at a fixed x and bound no hit region, so this was typography with no
    /// geometry attached — the easy half of N-4, kept honest by a gate so the
    /// next control added to these tabs does not quietly reintroduce the cell
    /// path.
    ///
    /// Widened in N-6c: **no** settings module draws text on the cell path
    /// now. When this gate was written it covered these two files only,
    /// because the rest of `settings/` still did.
    #[test]
    fn no_settings_module_draws_prose_on_the_cell_path() {
        for (name, src) in [
            ("ssh_tab.rs", include_str!("ssh_tab.rs")),
            ("keybindings_tab.rs", include_str!("keybindings_tab.rs")),
            ("mod.rs", include_str!("mod.rs")),
            ("sidebar.rs", include_str!("sidebar.rs")),
            ("row.rs", include_str!("row.rs")),
            ("blocks_tab.rs", include_str!("blocks_tab.rs")),
            ("profiles_tab.rs", include_str!("profiles_tab.rs")),
            ("font_tab.rs", include_str!("font_tab.rs")),
            ("startup_tab.rs", include_str!("startup_tab.rs")),
            ("theme_tab.rs", include_str!("theme_tab.rs")),
            ("window_tab.rs", include_str!("window_tab.rs")),
            ("security_tab.rs", include_str!("security_tab.rs")),
        ] {
            let src = src
                .split("#[cfg(test)]")
                .next()
                .expect("every file has a body before its tests");
            assert!(
                !src.contains("add_string_verts"),
                "{name} draws text on the cell path again; the settings panel is \
                 entirely on the chrome ramp (N-4e, N-6a-c)"
            );
        }
    }

    /// G-decoration: a button's label is text, not a drawn border.
    ///
    /// `"[ Cancel (Esc) ]"` and `"  Cancel (Esc)"` put a border and an indent
    /// inside the translation. `add_px_rect` draws the border, and a centred
    /// label cannot carry a leading indent without going off-centre — so this
    /// is the gate that stops the decoration returning through a translation
    /// PR, which is the only door left open to it.
    #[test]
    fn no_button_label_carries_its_own_decoration() {
        for locale in ["en", "ja", "de", "fr", "es", "it", "ko", "zh-CN"] {
            let raw = std::fs::read_to_string(format!(
                "{}/../nexterm-i18n/locales/{locale}.json",
                env!("CARGO_MANIFEST_DIR")
            ))
            .expect("locale file is readable");
            let map: serde_json::Value =
                serde_json::from_str(&raw).expect("locale file is valid JSON");
            let obj = map.as_object().expect("locale file is an object");

            assert!(
                !obj.contains_key("settings-dialog-cancel-plain")
                    && !obj.contains_key("settings-dialog-cancel-bracketed"),
                "{locale}: the two decorated cancel keys are replaced by one"
            );

            const KEY: &str = "settings-dialog-cancel";
            let v = obj
                .get(KEY)
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("{locale}: {KEY} is missing"));
            assert_eq!(v.trim(), v, "{locale}: {KEY} is padded: {v:?}");
            assert!(
                !v.starts_with('[') && !v.ends_with(']'),
                "{locale}: {KEY} draws its own button border: {v:?}"
            );
        }
    }
}
