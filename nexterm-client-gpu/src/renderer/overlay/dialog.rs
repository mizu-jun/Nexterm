//! Vertex builders for modal overlays.
//!
//! Handles the password input modal / context menu / consent dialog.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::{ClientState, ContextMenu};
use crate::vertex_util::{add_px_rect, add_string_verts, visual_width};

use super::super::WgpuState;
use super::util::{draw_overlay_panel, pane_id_for, preview_text, wrap_text};

impl WgpuState {
    /// Build vertices for the password input modal
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_password_modal_verts(
        &self,
        state: &ClientState,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let Some(modal) = &state.host_manager.password_modal else {
            return;
        };

        let pw = 44.0 * cell_w;
        let ph = 6.0 * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome: drop-shadow + border ring + rounded background.
        draw_overlay_panel(px, py, pw, ph, tokens, 4.0, 6.0, sw, sh, bg_verts, bg_idx);
        // Top accent stripe.
        let ap = tokens.accent_primary;
        add_px_rect(px, py, pw, 2.0, ap, sw, sh, bg_verts, bg_idx);

        // Title
        let title = format!(
            "Password: {}@{}:{}",
            modal.host.username, modal.host.host, modal.host.port
        );
        add_string_verts(
            &title,
            px + cell_w,
            py + cell_h * 0.15,
            tokens.accent_primary,
            true,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Password input field (masked display)
        // HIGH H-6: `input` is a private Zeroizing<String>, so only retrieve the char count via input_len()
        let masked = "*".repeat(modal.input_len());
        let prompt = format!("> {}_", masked);
        add_string_verts(
            &prompt,
            px + cell_w,
            py + cell_h * 1.3,
            [1.0, 1.0, 1.0, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Error message
        if let Some(err) = &modal.error {
            add_string_verts(
                err,
                px + cell_w,
                py + cell_h * 2.5,
                tokens.semantic_error,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // remember state (toggle for storing in the OS keychain)
        let remember_label = if modal.remember {
            "[X] Save to OS keychain (Tab to toggle)"
        } else {
            "[ ] Save to OS keychain (Tab to toggle)"
        };
        let remember_color = if modal.remember {
            [0.4, 0.9, 0.5, 1.0]
        } else {
            [0.6, 0.6, 0.6, 1.0]
        };
        add_string_verts(
            remember_label,
            px + cell_w,
            py + cell_h * 3.2,
            remember_color,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
        if modal.prefilled {
            add_string_verts(
                "(prefilled from the keychain)",
                px + cell_w,
                py + cell_h * 2.0,
                [0.5, 0.7, 1.0, 1.0],
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // Hint
        add_string_verts(
            "Enter=connect  Tab=toggle save  Esc=cancel",
            px + cell_w,
            py + cell_h * 4.1,
            tokens.text_secondary,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }

    /// Build vertices for the context menu (right-click popup)
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_context_menu_verts(
        &self,
        menu: &ContextMenu,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // Compute the menu width dynamically from the max visual width of labels and hints
        let max_label_w = menu
            .items
            .iter()
            .map(|item| visual_width(&item.label))
            .max()
            .unwrap_or(8);
        let max_hint_w = menu
            .items
            .iter()
            .map(|item| visual_width(&item.hint))
            .max()
            .unwrap_or(0);
        // Left padding (0.9) + label + gap (2) + hint + right padding (1.5)
        let min_cells = max_label_w + max_hint_w + 5;
        let menu_w = (min_cells as f32).max(16.0) * cell_w;
        let menu_h = menu.items.len() as f32 * cell_h;
        let mx = menu.x;
        let my = menu.y;

        // Panel chrome: drop-shadow + border ring + rounded background.
        draw_overlay_panel(mx, my, menu_w, menu_h, tokens, 3.0, 4.0, sw, sh, bg_verts, bg_idx);

        // Top accent line (3px thick)
        let ap = tokens.accent_primary;
        add_px_rect(mx, my, menu_w, 3.0, ap, sw, sh, bg_verts, bg_idx);

        for (i, item) in menu.items.iter().enumerate() {
            use crate::state::ContextMenuAction;
            let item_y = my + i as f32 * cell_h;

            if matches!(item.action, ContextMenuAction::Separator) {
                // Separator: draw a horizontal line in the middle
                let sep_y = item_y + cell_h * 0.45;
                add_px_rect(
                    mx + cell_w * 0.5,
                    sep_y,
                    menu_w - cell_w,
                    1.0,
                    [0.28, 0.32, 0.45, 0.70],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                continue;
            }

            // Hover highlight background (non-separator items)
            if menu.hovered == Some(i) {
                let hab = tokens.tab_active_bg;
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    menu_w - 4.0,
                    cell_h - 2.0,
                    [hab[0], hab[1], hab[2], 0.90],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // Left accent line on hover (3px)
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    3.0,
                    cell_h - 2.0,
                    [ap[0], ap[1], ap[2], 0.90],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // Label text (left padding 0.9 cells)
            let text_color = if menu.hovered == Some(i) {
                tokens.text_primary
            } else {
                tokens.text_secondary
            };
            add_string_verts(
                &item.label,
                mx + cell_w * 0.9,
                item_y + cell_h * 0.1,
                text_color,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );

            // Key hint text (right-aligned, muted)
            if !item.hint.is_empty() {
                let hint_visual_w = visual_width(&item.hint) as f32;
                let hint_x = mx + menu_w - (hint_visual_w * cell_w + cell_w * 0.5);
                add_string_verts(
                    &item.hint,
                    hint_x,
                    item_y + cell_h * 0.1,
                    [0.45, 0.48, 0.60, 0.80],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
        }
    }

    /// Build vertices for the consent dialog (Sprint 4-1: sensitive-operation confirmation modal)
    ///
    /// Center floating. Renders title, preview, and buttons depending on the kind.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_consent_dialog_verts(
        &self,
        state: &ClientState,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        use crate::state::ConsentKind;

        let Some(dialog) = &state.pending_consent else {
            return;
        };

        // Semi-transparent backdrop overlay (full screen)
        add_px_rect(
            0.0,
            0.0,
            sw,
            sh,
            [0.0, 0.0, 0.0, 0.55],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Dialog dimensions (60 cells wide, 12 cells tall)
        let pw = (60.0 * cell_w).min(sw - cell_w * 4.0);
        let ph = 12.0 * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome: drop-shadow + border ring + rounded background.
        draw_overlay_panel(px, py, pw, ph, tokens, 5.0, 6.0, sw, sh, bg_verts, bg_idx);
        // Top accent stripe (warning color).
        let warn_color = tokens.semantic_warning;
        add_px_rect(px, py, pw, 3.0, warn_color, sw, sh, bg_verts, bg_idx);

        // Title
        let title_key = match dialog.kind {
            ConsentKind::OpenUrl(_) => "consent-title-open-url",
            ConsentKind::ClipboardWrite { .. } => "consent-title-clipboard-write",
            ConsentKind::Notification { .. } => "consent-title-notification",
        };
        let title = nexterm_i18n::t(title_key);
        add_string_verts(
            &title,
            px + cell_w,
            py + cell_h * 0.4,
            warn_color,
            true,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Requesting pane info
        let mut content_y = py + cell_h * 1.8;
        if let Some(pane_id) = pane_id_for(&dialog.kind) {
            let label = nexterm_i18n::fl!("consent-source-pane", pane_id = pane_id);
            add_string_verts(
                &label,
                px + cell_w,
                content_y,
                tokens.text_secondary,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
            content_y += cell_h * 1.3;
        }

        // Payload preview (up to 2 lines, 56 chars each)
        let preview = preview_text(&dialog.kind);
        for (i, line) in wrap_text(&preview, 56).iter().take(2).enumerate() {
            add_string_verts(
                line,
                px + cell_w,
                content_y + i as f32 * cell_h,
                tokens.text_primary,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // Button row (4 buttons; highlight the selected one)
        let buttons = [
            nexterm_i18n::t("consent-allow"),
            nexterm_i18n::t("consent-deny"),
            nexterm_i18n::t("consent-always-allow"),
            nexterm_i18n::t("consent-always-deny"),
        ];
        let btn_y = py + ph - cell_h * 2.6;
        let total_w: f32 = buttons
            .iter()
            .map(|b| visual_width(b) as f32 * cell_w + cell_w * 2.0)
            .sum();
        let mut bx = px + (pw - total_w) / 2.0;
        for (i, btn) in buttons.iter().enumerate() {
            let bw = visual_width(btn) as f32 * cell_w + cell_w * 1.5;
            let is_selected = dialog.selected == i;
            let bg = if is_selected {
                warn_color
            } else {
                tokens.surface_3
            };
            add_px_rect(bx, btn_y, bw, cell_h * 1.4, bg, sw, sh, bg_verts, bg_idx);
            let fg = if is_selected {
                [0.10, 0.10, 0.10, 1.0]
            } else {
                tokens.text_primary
            };
            add_string_verts(
                btn,
                bx + cell_w * 0.75,
                btn_y + cell_h * 0.2,
                fg,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
            bx += bw + cell_w * 0.5;
        }

        // Operation hint (last row)
        let hint = nexterm_i18n::t("consent-hint");
        add_string_verts(
            &hint,
            px + cell_w,
            py + ph - cell_h * 1.0,
            tokens.text_secondary,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }

    /// Render the Window-close confirmation dialog (Sprint 5-9 Phase 4-6).
    ///
    /// Called only when `state.close_window_dialog` is `Some`; displays a modal
    /// dialog in the center of the screen. Follows the same decoration pattern
    /// as `build_consent_dialog_verts` (semi-transparent overlay + error-color
    /// accent + two centered buttons) to keep visual consistency.
    ///
    /// Button layout:
    /// - Left (selected_button = 0): "Close (Kill)" — red background
    /// - Right (selected_button = 1): "Cancel" — gray background
    ///
    /// Confirmation signal values (`0xFE` = Kill confirmed / `0xFF` = Cancel
    /// confirmed) are written from the `input_handler` side and consumed by
    /// `poll_pending_close_request` on the next frame.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_close_window_dialog_verts(
        &self,
        state: &ClientState,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let Some(dialog) = &state.close_window_dialog else {
            return;
        };

        // Semi-transparent overlay (full screen; visual shield that prevents accidental clicks)
        add_px_rect(
            0.0,
            0.0,
            sw,
            sh,
            [0.0, 0.0, 0.0, 0.55],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Dialog dimensions (56 cells wide / 10 cells tall; clamped to screen size)
        let pw = (56.0 * cell_w).min(sw - cell_w * 4.0);
        let ph = 10.0 * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome: drop-shadow + border ring + rounded background.
        draw_overlay_panel(px, py, pw, ph, tokens, 5.0, 6.0, sw, sh, bg_verts, bg_idx);
        // Top accent stripe (error/danger color; stronger alert than the consent dialog).
        let err_color = tokens.semantic_error;
        add_px_rect(px, py, pw, 3.0, err_color, sw, sh, bg_verts, bg_idx);

        // Title = render the confirmation message directly (short enough to skip a separate title).
        // If it overflows the width, wrap_text breaks it to up to 2 lines.
        let content_y = py + cell_h * 1.2;
        let max_cols = ((pw - cell_w * 2.0) / cell_w).max(20.0) as usize;
        for (i, line) in wrap_text(&dialog.message, max_cols)
            .iter()
            .take(3)
            .enumerate()
        {
            add_string_verts(
                line,
                px + cell_w,
                content_y + i as f32 * cell_h * 1.1,
                tokens.text_primary,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // Button row: Kill (left, selected_button == 0) + Cancel (right, selected_button == 1)
        let buttons: [(&str, [f32; 4]); 2] = [
            (&dialog.kill_label, [0.75, 0.25, 0.25, 1.0]),
            (&dialog.cancel_label, tokens.surface_3),
        ];
        let btn_y = py + ph - cell_h * 2.6;
        let btn_widths: Vec<f32> = buttons
            .iter()
            .map(|(label, _)| visual_width(label) as f32 * cell_w + cell_w * 3.0)
            .collect();
        let total_w: f32 = btn_widths.iter().sum::<f32>() + cell_w * 0.8;
        let mut bx = px + (pw - total_w) / 2.0;
        for (i, (label, base_bg)) in buttons.iter().enumerate() {
            let is_selected = dialog.selected_button as usize == i;
            // Selected: fill with the accent color; unselected: base color
            let bg = if is_selected {
                if i == 0 {
                    [0.95, 0.40, 0.40, 1.0] // Kill selected: vivid red
                } else {
                    [0.95, 0.85, 0.40, 1.0] // Cancel selected: yellow (safe side)
                }
            } else {
                *base_bg
            };
            let bw = btn_widths[i];
            add_px_rect(bx, btn_y, bw, cell_h * 1.4, bg, sw, sh, bg_verts, bg_idx);
            let fg = if is_selected {
                [0.10, 0.10, 0.10, 1.0]
            } else {
                tokens.text_primary
            };
            // Center the label
            let label_w = visual_width(label) as f32 * cell_w;
            let label_x = bx + (bw - label_w) / 2.0;
            add_string_verts(
                label,
                label_x,
                btn_y + cell_h * 0.2,
                fg,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
            bx += bw + cell_w * 0.8;
        }

        // Operation hint (last row). Uses concise English + symbols rather than reusing
        // an i18n key (symbol-heavy phrasing reads the same across locales).
        let hint = "Enter / Y: confirm  •  Esc / N: cancel  •  ← →: switch";
        add_string_verts(
            hint,
            px + cell_w,
            py + ph - cell_h * 1.0,
            tokens.text_secondary,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }
}
