//! Vertex builders for modal overlays.
//!
//! Handles the password input modal / context menu / consent dialog.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::host_manager::PasswordModalView;
use crate::renderer::menu_layout;
use crate::state::{ClientState, CloseWindowDialog, ConsentDialog, ContextMenu};
use crate::vertex_util::{add_px_rect, add_run_verts, measure_run};

use super::super::WgpuState;
use super::util::{
    SCRIM_ALPHA_FLOOR, caution_fill, danger_fill, draw_overlay_panel, pane_id_for, preview_text,
    scrim_color, wrap_text,
};
use nexterm_config::SurfaceLevel;

impl WgpuState {
    /// Build vertices for the password input modal
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_password_modal_verts(
        &self,
        view: &PasswordModalView<'_>,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        acrylic_mix: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // UI/UX v3 P4b: the chrome type ramp. Deliberately not `scaled()` —
        // `FontManager::chrome_metrics` owns the DPI conversion.
        let metrics = nexterm_config::MetricTokens::default();
        let pw = 44.0 * cell_w;
        let ph = 6.0 * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome: drop-shadow + border ring + rounded background.
        let elevation = nexterm_config::ElevationScale::default().dialog;
        draw_overlay_panel(
            px,
            py,
            pw,
            ph,
            tokens,
            elevation,
            6.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );
        // Top accent stripe.
        let ap = tokens.accent_primary;
        add_px_rect(px, py, pw, 2.0, ap, sw, sh, bg_verts, bg_idx);

        // Title
        let title = format!("Password: {}@{}:{}", view.username, view.host, view.port);
        add_run_verts(
            &title,
            &metrics.type_ramp.body,
            px + cell_w,
            py + cell_h * 0.15,
            tokens.accent_primary,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Password input field (masked display)
        // HIGH H-6, made structural in UI/UX v3 P3b: this builder is handed a
        // `PasswordModalView`, which has no field the password could be in.
        // The mask is drawn from `input_len`, never from the characters.
        let masked = "*".repeat(view.input_len);
        let prompt = format!("> {}_", masked);
        add_run_verts(
            &prompt,
            &metrics.type_ramp.body,
            px + cell_w,
            py + cell_h * 1.3,
            tokens.text_on(SurfaceLevel::S2).primary,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Error message
        if let Some(err) = view.error {
            add_run_verts(
                err,
                &metrics.type_ramp.body,
                px + cell_w,
                py + cell_h * 2.5,
                tokens.semantic_error,
                sw,
                sh,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // remember state (toggle for storing in the OS keychain)
        let remember_label = if view.remember {
            "[X] Save to OS keychain (Tab to toggle)"
        } else {
            "[ ] Save to OS keychain (Tab to toggle)"
        };
        let remember_color = if view.remember {
            tokens.semantic_success
        } else {
            tokens.text_on(SurfaceLevel::S2).muted
        };
        add_run_verts(
            remember_label,
            &metrics.type_ramp.body,
            px + cell_w,
            py + cell_h * 3.2,
            remember_color,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
        if view.prefilled {
            add_run_verts(
                "(prefilled from the keychain)",
                &metrics.type_ramp.body,
                px + cell_w,
                py + cell_h * 2.0,
                tokens.semantic_info,
                sw,
                sh,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // Hint
        add_run_verts(
            "Enter=connect  Tab=toggle save  Esc=cancel",
            &metrics.type_ramp.body,
            px + cell_w,
            py + cell_h * 4.1,
            tokens.text_on(SurfaceLevel::S2).secondary,
            sw,
            sh,
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
        acrylic_mix: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
        now: std::time::Instant,
    ) {
        // Geometry comes from `menu_layout` (UI/UX v3 N-4a) — the same
        // functions both placement sites and both hit-tests call, so the panel
        // drawn here and the region the mouse responds to are one number.
        let menu_w = menu_layout::menu_width(&menu.items, cell_w, font);
        let menu_h = menu_layout::menu_height(&menu.items, cell_h);
        let mx = menu.x;
        let my = menu.y;

        // Panel chrome: drop-shadow + border ring + rounded background.
        let elevation = nexterm_config::ElevationScale::default().flyout;
        draw_overlay_panel(
            mx,
            my,
            menu_w,
            menu_h,
            tokens,
            elevation,
            4.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );

        // Top accent line (3px thick)
        let ap = tokens.accent_primary;
        add_px_rect(mx, my, menu_w, 3.0, ap, sw, sh, bg_verts, bg_idx);

        for (i, item) in menu.items.iter().enumerate() {
            use crate::state::ContextMenuAction;
            let item_y = menu_layout::row_y(my, i, cell_h);

            if matches!(item.action, ContextMenuAction::Separator) {
                // Separator: draw a horizontal line in the middle
                let sep_y = item_y + cell_h * 0.45;
                let sep_color = {
                    let [r, g, b, _] = tokens.border_subtle;
                    [r, g, b, 0.70]
                };
                add_px_rect(
                    mx + cell_w * 0.5,
                    sep_y,
                    menu_w - cell_w,
                    1.0,
                    sep_color,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                continue;
            }

            // UI/UX v3 P3b2: hover cross-fades rather than snapping. Three
            // properties move together — the row fill, the accent line and
            // the label — so the weight lerps each of them instead of an
            // extra layer being faded in.
            // UI/UX v3 P3b3: press raises the weight before dimming it; both
            // layers below carry the same treatment so the row and its accent
            // stay one object.
            let press = menu.press_pulse.weight(i, now);
            let hover_w = menu.hover_transition.weight(i, now);
            let w = hover_w.max(press);
            if w > 0.0 {
                let hab = tokens.tab_active_bg;
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    menu_w - 4.0,
                    cell_h - 2.0,
                    crate::color_util::press_fill([hab[0], hab[1], hab[2], 0.90 * w], press),
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
                    crate::color_util::press_fill([ap[0], ap[1], ap[2], 0.90 * w], press),
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // Label text (left padding 0.9 cells)
            // UI/UX v3 P3b3: deliberately reads only the hover weight, not
            // the press-boosted `w` above — press changes background fills
            // only, never a foreground colour.
            let text_color = crate::color_util::lerp_rgba(
                tokens.text_on(SurfaceLevel::S3).secondary,
                tokens.text_on(SurfaceLevel::S3).primary,
                hover_w,
            );
            add_run_verts(
                &item.label,
                &menu_layout::label_style(),
                mx + cell_w * 0.9,
                item_y + cell_h * 0.1,
                text_color,
                sw,
                sh,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );

            // Key hint text (right-aligned, muted). Right-aligning needs the
            // hint's own width, and it is measured at the step it is drawn at
            // — `menu_layout::hint_style` — so the gap to the panel edge is
            // the 0.5 cell it claims to be rather than whatever a cell count
            // happened to produce.
            if !item.hint.is_empty() {
                let hint_style = menu_layout::hint_style();
                let hint_w = measure_run(&item.hint, &hint_style, font);
                let hint_x = mx + menu_w - (hint_w + cell_w * 0.5);
                let hint_color = {
                    let [r, g, b, _] = tokens.text_on(SurfaceLevel::S3).muted;
                    [r, g, b, 0.80]
                };
                add_run_verts(
                    &item.hint,
                    &hint_style,
                    hint_x,
                    item_y + cell_h * 0.1,
                    hint_color,
                    sw,
                    sh,
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
        dialog: &ConsentDialog,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        acrylic_mix: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // UI/UX v3 P4b: the chrome type ramp. Deliberately not `scaled()` —
        // `FontManager::chrome_metrics` owns the DPI conversion.
        let metrics = nexterm_config::MetricTokens::default();
        use crate::state::ConsentKind;

        // Semi-transparent backdrop overlay (full screen)
        add_px_rect(
            0.0,
            0.0,
            sw,
            sh,
            scrim_color(tokens, SCRIM_ALPHA_FLOOR),
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
        let elevation = nexterm_config::ElevationScale::default().dialog;
        draw_overlay_panel(
            px,
            py,
            pw,
            ph,
            tokens,
            elevation,
            6.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );
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
        add_run_verts(
            &title,
            &metrics.type_ramp.title,
            px + cell_w,
            py + cell_h * 0.4,
            warn_color,
            sw,
            sh,
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
            add_run_verts(
                &label,
                &metrics.type_ramp.body,
                px + cell_w,
                content_y,
                tokens.text_on(SurfaceLevel::S2).secondary,
                sw,
                sh,
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
            add_run_verts(
                line,
                &metrics.type_ramp.body,
                px + cell_w,
                content_y + i as f32 * cell_h,
                tokens.text_on(SurfaceLevel::S2).primary,
                sw,
                sh,
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
        // UI/UX v3 P4c: the labels moved to the ramp's Body step, so the box
        // widths come from `measure_run` rather than from `visual_width *
        // cell_w`. The padding (1.5 cells) and the gap (0.5) are unchanged —
        // this is a text-metric change, not a spacing one — except that the
        // row now reserves `n - 1` gaps instead of `n`, which used to leave it
        // half a gap off centre. Nothing here reaches a click target: consent
        // dialogs are keyboard- and AccessKit-driven, which is what let this
        // move without the hit-region work the footer links needed.
        let btn_style = metrics.type_ramp.body;
        let (_size, btn_line_h, _bold) = font.chrome_metrics(&btn_style);
        let btn_widths: Vec<f32> = buttons
            .iter()
            .map(|b| measure_run(b, &btn_style, font) + cell_w * 1.5)
            .collect();
        let total_w: f32 =
            btn_widths.iter().sum::<f32>() + cell_w * 0.5 * (buttons.len() - 1) as f32;
        let mut bx = px + (pw - total_w) / 2.0;
        for (i, btn) in buttons.iter().enumerate() {
            let bw = btn_widths[i];
            let is_selected = dialog.selected == i;
            // UI/UX v3 (G11): the selected fill is the warning hue blended into
            // the panel surface, not the raw token. Used raw it sits at a
            // middling luminance on some schemes (Solarized: neither a dark nor
            // a light label clears 4.5:1 against it); blending gives the label
            // an extreme to contrast against. The 3 px stripe and the title
            // above keep the raw hue — they are a line and text, not a fill.
            let bg = if is_selected {
                caution_fill(tokens, 0.85)
            } else {
                tokens.surface_3
            };
            add_px_rect(bx, btn_y, bw, cell_h * 1.4, bg, sw, sh, bg_verts, bg_idx);
            // UI/UX v3 (G11): the selected button is filled with the warning
            // hue, so its label is chosen against *that* fill. `text_on_accent`
            // would answer for `accent_primary` and could put a light label on
            // a pale yellow button.
            let fg = if is_selected {
                crate::color_util::on_surface_text(bg)
            } else {
                tokens.text_on(SurfaceLevel::S3).primary
            };
            // Centred from the measured run, so a label that the ramp made
            // wider or narrower than its cell estimate still sits in the
            // middle of its box rather than drifting against the padding.
            let label_w = measure_run(btn, &btn_style, font);
            add_run_verts(
                btn,
                &btn_style,
                bx + (bw - label_w) * 0.5,
                btn_y + (cell_h * 1.4 - btn_line_h) * 0.5,
                fg,
                sw,
                sh,
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
        add_run_verts(
            &hint,
            &metrics.type_ramp.caption,
            px + cell_w,
            py + ph - cell_h * 1.0,
            tokens.text_on(SurfaceLevel::S2).secondary,
            sw,
            sh,
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
        dialog: &CloseWindowDialog,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        acrylic_mix: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // UI/UX v3 P4b: the chrome type ramp. Deliberately not `scaled()` —
        // `FontManager::chrome_metrics` owns the DPI conversion.
        let metrics = nexterm_config::MetricTokens::default();
        // Semi-transparent overlay (full screen; visual shield that prevents accidental clicks)
        add_px_rect(
            0.0,
            0.0,
            sw,
            sh,
            scrim_color(tokens, SCRIM_ALPHA_FLOOR),
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
        let elevation = nexterm_config::ElevationScale::default().dialog;
        draw_overlay_panel(
            px,
            py,
            pw,
            ph,
            tokens,
            elevation,
            6.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );
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
            add_run_verts(
                line,
                &metrics.type_ramp.body,
                px + cell_w,
                content_y + i as f32 * cell_h * 1.1,
                tokens.text_on(SurfaceLevel::S2).primary,
                sw,
                sh,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // Button row: Kill (left, selected_button == 0) + Cancel (right, selected_button == 1)
        //
        // UI/UX v3 (G11): both buttons' fills now come from tokens. Kill keeps
        // its "red even when not selected" reading — it has to be identifiable
        // as the destructive choice before the user moves onto it — so it steps
        // between two `danger_fill` strengths rather than reusing the settings
        // dialogs' focused/unfocused pair. Cancel keeps its warm selected fill
        // (the deliberate "you are on the safe side" signal that the consent
        // dialog already spells with `semantic_warning`).
        let buttons: [(&str, [f32; 4]); 2] = [
            (&dialog.kill_label, danger_fill(tokens, 0.55)),
            (&dialog.cancel_label, tokens.surface_3),
        ];
        let btn_y = py + ph - cell_h * 2.6;
        // UI/UX v3 P4c: as in the consent dialog above — box widths measured
        // at the ramp's Body step, padding (3 cells) and gap (0.8) unchanged.
        let btn_style = metrics.type_ramp.body;
        let (_size, btn_line_h, _bold) = font.chrome_metrics(&btn_style);
        let btn_widths: Vec<f32> = buttons
            .iter()
            .map(|(label, _)| measure_run(label, &btn_style, font) + cell_w * 3.0)
            .collect();
        let total_w: f32 =
            btn_widths.iter().sum::<f32>() + cell_w * 0.8 * (buttons.len() - 1) as f32;
        let mut bx = px + (pw - total_w) / 2.0;
        for (i, (label, base_bg)) in buttons.iter().enumerate() {
            let is_selected = dialog.selected_button as usize == i;
            // Selected: fill with the accent color; unselected: base color
            let bg = if is_selected {
                if i == 0 {
                    danger_fill(tokens, 0.85) // Kill selected: strongest red
                } else {
                    caution_fill(tokens, 0.85) // Cancel selected: warm (safe side)
                }
            } else {
                *base_bg
            };
            let bw = btn_widths[i];
            add_px_rect(bx, btn_y, bw, cell_h * 1.4, bg, sw, sh, bg_verts, bg_idx);
            // A semantic fill (either Kill state, or a selected Cancel) needs
            // its label chosen against that fill; a plain surface fill reads
            // best in the scheme's own foreground.
            let fg = if is_selected || i == 0 {
                crate::color_util::on_surface_text(bg)
            } else {
                tokens.text_on(SurfaceLevel::S3).primary
            };
            // Center the label, measured the same way the box was sized.
            let label_w = measure_run(label, &btn_style, font);
            add_run_verts(
                label,
                &btn_style,
                bx + (bw - label_w) * 0.5,
                btn_y + (cell_h * 1.4 - btn_line_h) * 0.5,
                fg,
                sw,
                sh,
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
        add_run_verts(
            hint,
            &metrics.type_ramp.body,
            px + cell_w,
            py + ph - cell_h * 1.0,
            tokens.text_on(SurfaceLevel::S2).secondary,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }

    /// Build vertices for the block-name input modal (Phase 2c-4 / 2c-1).
    ///
    /// Mirrors `build_password_modal_verts` but stripped of secret-handling
    /// and "remember" toggles: the modal only carries a plain text buffer
    /// for naming a command block. The frame is centred on the canvas.
    /// Drawing is gated by `state.block_name_modal.motion.is_visible()` so the
    /// exit animation still gets a frame after `is_open` flips to `false`.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_block_name_modal_verts(
        &self,
        state: &ClientState,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        acrylic_mix: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // UI/UX v3 P4b: the chrome type ramp. Deliberately not `scaled()` —
        // `FontManager::chrome_metrics` owns the DPI conversion.
        let metrics = nexterm_config::MetricTokens::default();
        if !state.block_name_modal.motion.is_visible() {
            return;
        }

        let pw = 44.0 * cell_w;
        let ph = 5.0 * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome.
        let elevation = nexterm_config::ElevationScale::default().dialog;
        draw_overlay_panel(
            px,
            py,
            pw,
            ph,
            tokens,
            elevation,
            6.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );
        // Top accent stripe in the same hue as block selection.
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            tokens.accent_primary,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Title.
        let title = nexterm_i18n::t("block-modal-title");
        add_run_verts(
            &title,
            &metrics.type_ramp.title,
            px + cell_w,
            py + cell_h * 0.15,
            tokens.accent_primary,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Input field with a trailing underscore caret.
        let prompt = format!("> {}_", state.block_name_modal.input());
        add_run_verts(
            &prompt,
            &metrics.type_ramp.body,
            px + cell_w,
            py + cell_h * 1.3,
            tokens.text_on(SurfaceLevel::S2).primary,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Help line.
        let help = nexterm_i18n::t("block-modal-help");
        add_run_verts(
            &help,
            &metrics.type_ramp.body,
            px + cell_w,
            py + cell_h * 2.8,
            tokens.text_on(SurfaceLevel::S2).muted,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Error message (if any).
        if let Some(err) = &state.block_name_modal.error {
            add_run_verts(
                err,
                &metrics.type_ramp.body,
                px + cell_w,
                py + cell_h * 3.6,
                tokens.semantic_error,
                sw,
                sh,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }
}
