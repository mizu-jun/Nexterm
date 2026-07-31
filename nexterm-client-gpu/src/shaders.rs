//! WGSL shader constants — background, text, and image render passes.

/// Background-quad shader.
///
/// Two-mode pipeline (Sprint 5-15 / UI/UX Modernization v2 Phase 1):
///   * `corner_radius == 0`: classic flat rectangle, fragment is the vertex
///     color (premultiplied on output).
///   * `corner_radius > 0`: signed-distance-field rounded rectangle with a
///     1 px smoothstep edge for anti-aliasing. `rect_center` /
///     `rect_half_size` are in framebuffer pixel coordinates (y-down), the
///     same space as `@builtin(position).xy` in the fragment stage, so no
///     uniform / push-constant is required.
///
/// **Custom-shader contract** (`[gpu] custom_bg_shader`), breaking changes:
///   * since UI/UX v2 Phase 1: the 5-attribute vertex layout above
///     (`rect_center`, `rect_half_size`, `corner_radius` added; early-return
///     on `corner_radius <= 0` retains the v1 behavior);
///   * since UI/UX v3 P0: the fragment output must be **premultiplied alpha**
///     (`rgb * a`). The surface is `CompositeAlphaMode::PreMultiplied` and
///     every pipeline blends with `PREMULTIPLIED_ALPHA_BLENDING` (fixes the
///     washed-out translucency of issue #35).
pub(crate) const BG_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) rect_center: vec2<f32>,
    @location(3) rect_half_size: vec2<f32>,
    @location(4) corner_radius: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) rect_center: vec2<f32>,
    @location(2) rect_half_size: vec2<f32>,
    @location(3) corner_radius: f32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    out.rect_center = in.rect_center;
    out.rect_half_size = in.rect_half_size;
    out.corner_radius = in.corner_radius;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Output is premultiplied alpha (see the custom-shader contract above).
    if (in.corner_radius <= 0.0) {
        return vec4<f32>(in.color.rgb * in.color.a, in.color.a);
    }
    // Standard rounded-box SDF (Inigo Quilez formulation).
    let p = in.clip_position.xy;
    let d = abs(p - in.rect_center) - in.rect_half_size + vec2<f32>(in.corner_radius);
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - in.corner_radius;
    // 1-pixel AA edge.
    let aa = 1.0 - smoothstep(-0.5, 0.5, dist);
    let alpha = in.color.a * aa;
    return vec4<f32>(in.color.rgb * alpha, alpha);
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
