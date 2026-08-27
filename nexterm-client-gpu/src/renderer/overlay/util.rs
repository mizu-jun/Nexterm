//! Shared helpers used by overlay rendering.

use super::settings::row::MIN_TEXT_CONTRAST;

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
    acrylic_mix: f32,
    bg_verts: &mut Vec<crate::glyph_atlas::BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    use crate::vertex_util::{
        add_px_rounded_rect_sdf, add_px_rounded_rect_sdf_with_acrylic, add_px_soft_shadow_sdf,
    };

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

    // 3. Panel background — the only part that samples acrylic, via the
    //    trailing acrylic_mix vertex field (UI/UX v3 P2b).
    let bg = tokens.surface_2;
    add_px_rounded_rect_sdf_with_acrylic(
        px,
        py,
        pw,
        ph,
        radius,
        bg,
        sw,
        sh,
        acrylic_mix,
        bg_verts,
        bg_idx,
    );
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

/// Opaque fill for a destructive action: `semantic_error` blended into the
/// panel's own `surface_1` at `strength`.
///
/// The raw ANSI red is deliberately never used as a fill — it leaves no
/// headroom for a readable label, which is why every call site had been
/// darkening it by hand before UI/UX v3 (G11 follow-up).
///
/// `strength` is what separates the two questions a destructive button
/// answers. The settings delete dialogs only turn red once focused, so they
/// step from a barely-tinted rest state to a mid blend. The close-window
/// dialog's Kill button is red at all times — it sits next to Cancel and must
/// read as the dangerous one before it is ever selected — so it steps from
/// that same mid blend to a strong one. Callers pass the pair that carries
/// their own semantics rather than sharing one focused/unfocused rule.
pub(super) fn danger_fill(tokens: &nexterm_config::DesignTokens, strength: f32) -> [f32; 4] {
    semantic_fill(tokens, tokens.semantic_error, strength)
}

/// Opaque fill for the *safe* side of a destructive choice, and for a
/// selected button in the consent dialog: `semantic_warning` blended into
/// `surface_1` the same way [`danger_fill`] blends the error hue.
///
/// Blending matters here for a measurable reason, not for symmetry. Used raw,
/// `semantic_warning` sits at a middling luminance on some schemes — on
/// Solarized neither a near-black nor a near-white label clears 4.5:1 against
/// it (the best either extreme manages is 4.37:1). Blending the hue into the
/// panel surface pulls the fill towards that surface's own end of the range,
/// which gives the label an extreme to contrast against again.
pub(super) fn caution_fill(tokens: &nexterm_config::DesignTokens, strength: f32) -> [f32; 4] {
    semantic_fill(tokens, tokens.semantic_warning, strength)
}

/// Blend a semantic hue into the panel's `surface_1`, walking the blend back
/// toward the surface until the label [`crate::color_util::on_surface_text`]
/// would pick clears [`MIN_TEXT_CONTRAST`] against it.
///
/// A fixed strength cannot serve all nine built-in schemes. At 0.85 the error
/// hue lands at a middling luminance on Nord (4.42:1) and the warning hue does
/// the same on Solarized (4.37:1) — luminances where neither a near-black nor
/// a near-white label has anything to contrast with. Stepping back toward the
/// panel surface always terminates: `surface_1` is derived from the scheme's
/// background, which is the end of the range a label can always be read
/// against.
///
/// The consequence is that some schemes get a slightly quieter fill than the
/// caller asked for. That is the intended trade — an unreadable label on a
/// destructive button is worse than a less saturated one.
fn semantic_fill(tokens: &nexterm_config::DesignTokens, hue: [f32; 4], strength: f32) -> [f32; 4] {
    let base = [
        tokens.surface_1[0],
        tokens.surface_1[1],
        tokens.surface_1[2],
    ];
    let blend = |s: f32| -> [f32; 4] {
        let rgb = crate::color_util::composite_over(crate::color_util::with_alpha(hue, s), base);
        [rgb[0], rgb[1], rgb[2], 1.0]
    };
    let mut s = strength;
    loop {
        let fill = blend(s);
        let label = crate::color_util::on_surface_text(fill);
        let cr = crate::color_util::contrast_ratio(
            [label[0], label[1], label[2]],
            [fill[0], fill[1], fill[2]],
        );
        if cr >= MIN_TEXT_CONTRAST || s <= 0.05 {
            return fill;
        }
        s -= 0.05;
    }
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

    /// Every fill the modal dialogs paint a label onto must leave that label
    /// above the project's 4.5:1 floor, on every built-in scheme.
    ///
    /// This is the test that forced `caution_fill` to exist: used raw,
    /// `semantic_warning` tops out at 4.37:1 on Solarized because neither
    /// extreme contrasts with a middling luminance. Measured after blending —
    /// worst cases: 4.63:1 (Gruvbox, Kill selected), 4.83:1 (Dark, Kill
    /// selected), 4.87:1 (Solarized, Cancel selected).
    #[test]
    fn dialog_button_labels_clear_the_contrast_floor() {
        // Mirrors `row::MIN_TEXT_CONTRAST`, which is scoped to the settings
        // panel's module tree.
        const FLOOR: f32 = 4.5;
        for scheme in [
            nexterm_config::BuiltinScheme::Dark,
            nexterm_config::BuiltinScheme::Light,
            nexterm_config::BuiltinScheme::Gruvbox,
            nexterm_config::BuiltinScheme::Solarized,
            nexterm_config::BuiltinScheme::Catppuccin,
            nexterm_config::BuiltinScheme::Dracula,
            nexterm_config::BuiltinScheme::Nord,
            nexterm_config::BuiltinScheme::OneDark,
            nexterm_config::BuiltinScheme::TokyoNight,
        ] {
            let tokens = tokens_for(scheme);
            let fills = [
                ("kill resting", danger_fill(&tokens, 0.55)),
                ("kill selected", danger_fill(&tokens, 0.85)),
                ("caution selected", caution_fill(&tokens, 0.85)),
            ];
            for (name, bg) in fills {
                let fg = crate::color_util::on_surface_text(bg);
                let cr =
                    crate::color_util::contrast_ratio([fg[0], fg[1], fg[2]], [bg[0], bg[1], bg[2]]);
                assert!(
                    cr >= FLOOR,
                    "{scheme:?} {name}: label only reached {cr} against {bg:?}"
                );
            }
        }
    }

    /// UI/UX v3 (G11 follow-up): every destructive fill across the dialogs is
    /// one hue at a different strength, so a stronger blend must always read
    /// as redder. That ordering is what carries both "this is the dangerous
    /// button" and "and it is the one currently selected"; if it inverted on
    /// some scheme, the close-window dialog would highlight Kill by making it
    /// *less* red.
    #[test]
    fn danger_fill_gets_redder_with_strength() {
        let redness = |c: [f32; 4]| c[0] - (c[1] + c[2]) / 2.0;
        for scheme in [
            nexterm_config::BuiltinScheme::Dark,
            nexterm_config::BuiltinScheme::Light,
            nexterm_config::BuiltinScheme::Gruvbox,
            nexterm_config::BuiltinScheme::Solarized,
        ] {
            let tokens = tokens_for(scheme);
            let weak = danger_fill(&tokens, 0.18);
            let mid = danger_fill(&tokens, 0.55);
            let strong = danger_fill(&tokens, 0.85);
            assert!(
                redness(strong) > redness(mid) && redness(mid) > redness(weak),
                "{scheme:?}: strength does not order by redness ({:?} / {:?} / {:?})",
                redness(weak),
                redness(mid),
                redness(strong)
            );
        }
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

#[cfg(test)]
mod acrylic_mix_tests {
    use super::*;

    #[test]
    fn fill_vertices_carry_the_requested_acrylic_mix() {
        // Built via `from_palette` with a built-in scheme, matching this
        // file's other token-consuming tests (see `tokens_for` above).
        let tokens = nexterm_config::DesignTokens::from_palette(
            &nexterm_config::BuiltinScheme::TokyoNight.palette(),
        );
        let mut bg_verts = Vec::new();
        let mut bg_idx = Vec::new();
        draw_overlay_panel(
            10.0,
            10.0,
            100.0,
            50.0,
            &tokens,
            128.0,
            6.0,
            800.0,
            600.0,
            0.75,
            &mut bg_verts,
            &mut bg_idx,
        );
        // The panel background fill is the *last* 4 vertices pushed (shadow,
        // then border, then fill — see draw_overlay_panel's own comments).
        let fill_verts = &bg_verts[bg_verts.len() - 4..];
        assert!(
            fill_verts
                .iter()
                .all(|v| (v.acrylic_mix - 0.75).abs() < f32::EPSILON)
        );
        // The shadow and border ring stay opaque regardless of the panel's
        // acrylic_mix — only the fill itself is translucent acrylic.
        let non_fill_verts = &bg_verts[..bg_verts.len() - 4];
        assert!(non_fill_verts.iter().all(|v| v.acrylic_mix == 0.0));
    }
}
