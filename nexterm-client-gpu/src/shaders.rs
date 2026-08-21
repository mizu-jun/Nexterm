//! WGSL shader constants — background, text, and image render passes.

/// Background-quad shader.
///
/// Multi-mode pipeline (Sprint 5-15 / UI/UX v2 Phase 1, extended by
/// UI/UX v3 P2a):
///   * all extensions off: classic flat rectangle, fragment is the vertex
///     color (premultiplied on output).
///   * `corner_radius > 0`: signed-distance-field rounded rectangle with a
///     1 px smoothstep edge for anti-aliasing. `rect_center` /
///     `rect_half_size` are in framebuffer pixel coordinates (y-down), the
///     same space as `@builtin(position).xy` in the fragment stage, so no
///     uniform / push-constant is required.
///   * `shadow_softness > 0`: the 1 px edge widens into a penumbra of that
///     half-width — a soft drop shadow. The quad must be grown by the same
///     amount (`add_px_soft_shadow_sdf` does).
///   * `stroke_width > 0`: paints only an outline band hugging the inside
///     of the rect edge instead of a fill (wins over `shadow_softness`).
///     The quad stays tight, so no growing is needed (`add_px_stroke_sdf`).
///
/// **Custom-shader contract** (`[gpu] custom_bg_shader`), changes:
///   * since UI/UX v2 Phase 1: `rect_center`, `rect_half_size`,
///     `corner_radius` added (early-return on `corner_radius <= 0` retains
///     the v1 behavior);
///   * since UI/UX v3 P0 (**breaking**): the fragment output must be
///     **premultiplied alpha** (`rgb * a`). The surface is
///     `CompositeAlphaMode::PreMultiplied` and every pipeline blends with
///     `PREMULTIPLIED_ALPHA_BLENDING` (fixes the washed-out translucency of
///     issue #35).
///   * since UI/UX v3 P2a (additive): `shadow_softness` and `stroke_width`
///     complete what was then a 7-attribute layout. Existing custom
///     shaders that read only the first five attributes keep working —
///     wgpu validates that the shader's inputs are a subset of the buffer
///     layout, not an exact match.
///   * since UI/UX v3 P2b (additive): `acrylic_mix` is the 8th vertex
///     attribute, and `@group(0)` gains `acrylic_tex` / `acrylic_sampler` /
///     `acrylic` (an `AcrylicUniform`) so the fragment stage can sample the
///     blurred/tinted scene texture a later task (Task 6) populates.
///     `acrylic_mix > 0.0` only on overlay panel fills — every other quad
///     keeps drawing its flat `color` unchanged. Existing custom shaders
///     that ignore `@location(7)` and `@group(0)` keep working for the
///     same subset-of-bindings reason as P2a.
pub(crate) const BG_SHADER: &str = r#"
struct AcrylicUniform {
    tint: vec4<f32>,
    viewport_size: vec2<f32>,
    strength: f32,
    _pad: f32,
}

@group(0) @binding(0) var acrylic_tex: texture_2d<f32>;
@group(0) @binding(1) var acrylic_sampler: sampler;
@group(0) @binding(2) var<uniform> acrylic: AcrylicUniform;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) rect_center: vec2<f32>,
    @location(3) rect_half_size: vec2<f32>,
    @location(4) corner_radius: f32,
    @location(5) shadow_softness: f32,
    @location(6) stroke_width: f32,
    @location(7) acrylic_mix: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) rect_center: vec2<f32>,
    @location(2) rect_half_size: vec2<f32>,
    @location(3) corner_radius: f32,
    @location(4) shadow_softness: f32,
    @location(5) stroke_width: f32,
    @location(6) acrylic_mix: f32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    out.rect_center = in.rect_center;
    out.rect_half_size = in.rect_half_size;
    out.corner_radius = in.corner_radius;
    out.shadow_softness = in.shadow_softness;
    out.stroke_width = in.stroke_width;
    out.acrylic_mix = in.acrylic_mix;
    return out;
}

// Cheap hash-based procedural noise (UI/UX v3 P2b) — no texture asset, no
// licensing question. Fixed intensity, not tied to `acrylic.strength`.
fn acrylic_noise(p: vec2<f32>) -> f32 {
    let h = fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    return (h - 0.5) * 0.03; // +-1.5% luma grain
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var base_color = in.color;
    if (in.acrylic_mix > 0.0) {
        let uv = in.clip_position.xy / acrylic.viewport_size;
        let blurred = textureSample(acrylic_tex, acrylic_sampler, uv);
        let tinted = mix(blurred.rgb, acrylic.tint.rgb, acrylic.strength);
        let grain = acrylic_noise(in.clip_position.xy);
        base_color = vec4<f32>(
            mix(in.color.rgb, tinted + vec3<f32>(grain), in.acrylic_mix),
            in.color.a,
        );
    }
    // Output is premultiplied alpha (see the custom-shader contract above).
    if (in.corner_radius <= 0.0 && in.shadow_softness <= 0.0 && in.stroke_width <= 0.0) {
        return vec4<f32>(base_color.rgb * base_color.a, base_color.a);
    }
    // Standard rounded-box SDF (Inigo Quilez formulation).
    let p = in.clip_position.xy;
    let d = abs(p - in.rect_center) - in.rect_half_size + vec2<f32>(in.corner_radius);
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - in.corner_radius;

    var coverage: f32;
    if (in.stroke_width > 0.0) {
        // Outline band on the inside of the edge (dist in [-w, 0]), with
        // the same 1 px AA as the fill on both borders of the band.
        let half_w = in.stroke_width * 0.5;
        coverage = 1.0 - smoothstep(half_w - 0.5, half_w + 0.5, abs(dist + half_w));
    } else {
        // spread == 0.5 reproduces the pre-P2a 1 px AA edge exactly; a
        // shadow widens it into a penumbra centred on the rect border.
        let spread = max(in.shadow_softness, 0.5);
        coverage = 1.0 - smoothstep(-spread, spread, dist);
    }
    let alpha = base_color.a * coverage;
    return vec4<f32>(base_color.rgb * alpha, alpha);
}
"#;

/// Image-rendering shader: samples the texture, tints it by the vertex color
/// (the background-image pass carries its `opacity` in `color.a`; every other
/// caller passes opaque white), and outputs premultiplied alpha.
pub(crate) const IMAGE_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}
@group(0) @binding(0) var img_texture: texture_2d<f32>;
@group(0) @binding(1) var img_sampler: sampler;
@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(img_texture, img_sampler, in.uv) * in.color;
    return vec4<f32>(c.rgb * c.a, c.a);
}
"#;

/// Text shader sampling the glyph atlas (the alpha channel masks the
/// foreground color). Outputs premultiplied alpha; custom `[gpu]
/// custom_text_shader` files must do the same (see `BG_SHADER` contract).
pub(crate) const TEXT_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@group(0) @binding(0) var glyph_texture: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let mask = textureSample(glyph_texture, glyph_sampler, in.uv).a;
    let alpha = in.color.a * mask;
    return vec4<f32>(in.color.rgb * alpha, alpha);
}
"#;

/// Dual-Kawase blur (Bjørge, "Bandwidth-Efficient Rendering", 2015; the
/// same downsample/upsample tap pattern Godot and Unity's URP use for
/// cheap glass-blur effects). UI/UX v3 P2b: the downsample pass halves
/// resolution each step (4 taps), the upsample pass doubles it back up
/// (8 taps, weighted 1/1/1/1 axis-aligned + 2/2/2/2 diagonal). Tap
/// offsets here must match `acrylic::kawase_downsample_offsets` /
/// `kawase_upsample_offsets` in `renderer/acrylic.rs`.
#[allow(dead_code)] // wired into render pipelines in Task 6; exercised by its own test until then.
pub(crate) const KAWASE_BLUR_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// A single oversized triangle covering the full viewport — cheaper than a
// quad (no index buffer, no diagonal seam) for a fullscreen pass.
@vertex
fn vs_fullscreen(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(i32(idx) - 1) * 2.0;
    let y = f32(i32(idx & 1u) * 2 - 1) * 2.0;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    return out;
}

struct BlurParams {
    texel_size: vec2<f32>,
    _pad: vec2<f32>,
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;

@fragment
fn fs_downsample(in: VertexOutput) -> @location(0) vec4<f32> {
    let h = params.texel_size * 0.5;
    var c = textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-h.x, -h.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(h.x, -h.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-h.x, h.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(h.x, h.y));
    return c * 0.25;
}

@fragment
fn fs_upsample(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = params.texel_size;
    var c = textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-2.0 * t.x, 0.0));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(2.0 * t.x, 0.0));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(0.0, -2.0 * t.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(0.0, 2.0 * t.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-t.x, t.y)) * 2.0;
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(t.x, t.y)) * 2.0;
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-t.x, -t.y)) * 2.0;
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(t.x, -t.y)) * 2.0;
    return c * (1.0 / 12.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in shader must parse and validate as WGSL (UI/UX v3
    /// P2a). This is the only shader check CI can run — actual pipeline
    /// creation needs a GPU — and it catches syntax slips in the string
    /// constants before they fail at startup on a user's machine.
    #[test]
    fn builtin_shaders_parse_and_validate() {
        for (name, src) in [
            ("bg", BG_SHADER),
            ("image", IMAGE_SHADER),
            ("text", TEXT_SHADER),
            ("kawase_blur", KAWASE_BLUR_SHADER),
        ] {
            let module = wgpu::naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("{name} shader failed to parse: {e}"));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name} shader failed validation: {e:?}"));
        }
    }
}
