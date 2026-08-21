//! Shared helpers used by overlay rendering.

/// Extract the requesting pane ID from a consent-dialog kind
pub(super) fn pane_id_for(kind: &crate::state::ConsentKind) -> Option<u32> {
    use crate::state::ConsentKind;
    match kind {
        ConsentKind::OpenUrl(_) => None,
        ConsentKind::ClipboardWrite { source_pane, .. } => *source_pane,
        ConsentKind::Notification { source_pane, .. } => Some(*source_pane),
    }
}

/// Return the preview string for a consent-dialog kind
pub(super) fn preview_text(kind: &crate::state::ConsentKind) -> String {
    use crate::state::ConsentKind;
    match kind {
        ConsentKind::OpenUrl(url) => url.clone(),
        ConsentKind::ClipboardWrite { text, .. } => {
            // Replace control chars and newlines with spaces for safety
            let safe: String = text
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            // Truncate by byte length
            if safe.len() > 200 {
                let mut end = 200;
                while end > 0 && !safe.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &safe[..end])
            } else {
                safe
            }
        }
        ConsentKind::Notification { title, body, .. } => format!("{title}: {body}"),
    }
}

/// Wrap text to multiple lines at the given column width (CJK full-width chars count as 2 columns)
pub(super) fn wrap_text(s: &str, max_cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_cols = 0usize;
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if current_cols + w > max_cols && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_cols = 0;
        }
        current.push(c);
        current_cols += w;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Draw the shared chrome for every floating overlay panel:
/// soft drop-shadow → 1 px border ring → rounded filled background.
///
/// All colors are taken from `tokens` so the panel adapts to the active color scheme.
/// The border ring is drawn by overdrawing the background with a slightly larger rect
/// using `tokens.border_default` at reduced opacity.
///
/// The shadow is derived from `elevation` — a Fluent `ElevationScale` value
/// (UI/UX v3 P2a) — via [`shadow_params`], so a dialog visibly floats above
/// a flyout instead of every panel sharing one hard offset quad.
///
/// Corners are rendered with the same SDF rounded-rect used by the tab bar's
/// pills, so panel and chrome rounding match in quality (the previous
/// three-rect CPU approximation left visible notches at the corners).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_overlay_panel(
    px: f32,
    py: f32,
    pw: f32,
    ph: f32,
    tokens: &nexterm_config::DesignTokens,
    elevation: f32,
    radius: f32,
    sw: f32,
    sh: f32,
    bg_verts: &mut Vec<crate::glyph_atlas::BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    use crate::vertex_util::{add_px_rounded_rect_sdf, add_px_soft_shadow_sdf};

    // 1. Soft drop shadow scaled by the surface's Fluent elevation
    //    (UI/UX v3 P2a; was a hard offset quad with a per-caller offset).
    let shadow = shadow_params(elevation);
    add_px_soft_shadow_sdf(
        px + shadow.offset,
        py + shadow.offset,
        pw,
        ph,
        radius,
        [0.0, 0.0, 0.0, shadow.alpha],
        shadow.softness,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // 2. Border ring — 1 px wider on every side, tokens.border_default at ~18% alpha.
    let bd = tokens.border_default;
    let border_color = [bd[0], bd[1], bd[2], 0.18];
    add_px_rounded_rect_sdf(
        px - 1.0,
        py - 1.0,
        pw + 2.0,
        ph + 2.0,
        radius + 1.0,
        border_color,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // 3. Panel background — tokens.surface_2, fully opaque.
    let bg = tokens.surface_2;
    add_px_rounded_rect_sdf(px, py, pw, ph, radius, bg, sw, sh, bg_verts, bg_idx);
}

/// Soft-shadow recipe for one overlay surface (UI/UX v3 P2a).
pub(super) struct ShadowParams {
    /// Down-right offset of the shadow rect, in physical pixels.
    pub offset: f32,
    /// Penumbra half-width fed to the shader's `shadow_softness` attribute.
    pub softness: f32,
    /// Shadow color alpha (the hue is always black).
    pub alpha: f32,
}

/// Map a Fluent elevation value onto shadow geometry.
///
/// Initial mapping — offset = elevation/16 (1..8 px), softness =
/// elevation/8 (1..24 px), alpha fixed at 0.45 — chosen so the relative
/// ordering of the `ElevationScale` surfaces is visible; the absolute
/// values are subject to on-device tuning (GPU output is not
/// CI-verifiable).
pub(super) fn shadow_params(elevation: f32) -> ShadowParams {
    ShadowParams {
        offset: (elevation / 16.0).clamp(1.0, 8.0),
        softness: (elevation / 8.0).clamp(1.0, 24.0),
        alpha: 0.45,
    }
}

/// Alpha floor for a modal scrim.
///
/// High enough that a translucent terminal behind a modal stops reading as
/// still-interactive, low enough that the veil stays a veil.
pub(super) const SCRIM_ALPHA_FLOOR: f32 = 0.55;

/// Full-screen veil drawn behind a modal surface.
///
/// `surface_0` is the deepest background in the active scheme, so the veil
/// stays in the same colour family as the surface it sits behind, in light and
/// dark themes alike. The settings panel already reasoned this way about its
/// own scrim; UI/UX v3 (G11 follow-up) brings the other call sites — the
/// password modal, the close-window dialog and the two settings-tab delete
/// confirmations, which each carried their own hard-coded black — onto the
/// same helper.
///
/// `alpha` stays a parameter because the settings panel fades its scrim in
/// with the panel's open animation, while the modal dialogs snap straight to
/// [`SCRIM_ALPHA_FLOOR`].
pub(super) fn scrim_color(tokens: &nexterm_config::DesignTokens, alpha: f32) -> [f32; 4] {
    crate::color_util::with_alpha(tokens.surface_0, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_params_scale_with_the_elevation_table() {
        // UI/UX v3 P2a: higher surfaces cast farther, softer shadows. The
        // ordering must follow the Fluent elevation scale so a dialog reads
        // as sitting above a flyout, which sits above a tooltip.
        let e = nexterm_config::ElevationScale::default();
        let dialog = shadow_params(e.dialog);
        let flyout = shadow_params(e.flyout);
        let tooltip = shadow_params(e.tooltip);
        assert!(dialog.offset > flyout.offset);
        assert!(dialog.softness > flyout.softness);
        assert!(flyout.softness > tooltip.softness);
        // Concrete anchors of the initial mapping (subject to on-device
        // tuning): offset = elevation/16, softness = elevation/8.
        assert!((dialog.offset - 8.0).abs() < 1e-6);
        assert!((flyout.softness - 4.0).abs() < 1e-6);
    }

    #[test]
    fn shadow_params_keep_low_surfaces_visible() {
        // Resting controls (elevation 2) still get a minimal 1 px shadow
        // instead of degenerating to zero.
        let control = shadow_params(2.0);
        assert!(control.offset >= 1.0);
        assert!(control.softness >= 1.0);
        assert!(control.alpha > 0.0);
    }

    // ---- UI/UX v3 G11 follow-up: the shared modal scrim ----

    fn tokens_for(scheme: nexterm_config::BuiltinScheme) -> nexterm_config::DesignTokens {
        nexterm_config::DesignTokens::from_palette(&scheme.palette())
    }

    #[test]
    fn scrim_takes_the_deepest_surface_at_the_requested_alpha() {
        let tokens = tokens_for(nexterm_config::BuiltinScheme::TokyoNight);
        let scrim = scrim_color(&tokens, SCRIM_ALPHA_FLOOR);
        assert_eq!(
            [scrim[0], scrim[1], scrim[2]],
            [
                tokens.surface_0[0],
                tokens.surface_0[1],
                tokens.surface_0[2]
            ]
        );
        assert!((scrim[3] - SCRIM_ALPHA_FLOOR).abs() < 1e-6);
    }

    /// Regression guard against a hard-coded black coming back: a light scheme
    /// and a dark one must veil in different colours, the way the settings
    /// panel's scrim already did before the other call sites were migrated.
    #[test]
    fn scrim_follows_the_active_scheme() {
        let dark = scrim_color(&tokens_for(nexterm_config::BuiltinScheme::Dark), 0.55);
        let light = scrim_color(&tokens_for(nexterm_config::BuiltinScheme::Light), 0.55);
        assert_ne!([dark[0], dark[1], dark[2]], [light[0], light[1], light[2]]);
        // A light scheme's veil must actually be the light surface, not a
        // near-black one that happens to differ in the last decimal.
        assert!(
            light[0] > dark[0] + 0.3,
            "light scrim {light:?} is not meaningfully lighter than {dark:?}"
        );
    }
}
