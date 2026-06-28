//! WGSL shader constants — background, text, and image render passes.

/// Background-quad shader.
///
/// Two-mode pipeline (Sprint 5-15 / UI/UX Modernization v2 Phase 1):
///   * `corner_radius == 0`: classic flat rectangle, fragment is the vertex
///     color unmodified.
///   * `corner_radius > 0`: signed-distance-field rounded rectangle with a
///     1 px smoothstep edge for anti-aliasing. `rect_center` /
///     `rect_half_size` are in framebuffer pixel coordinates (y-down), the
///     same space as `@builtin(position).xy` in the fragment stage, so no
///     uniform / push-constant is required.
///
/// **Breaking change for custom shaders**: the `[gpu] custom_bg_shader` hook
/// now expects the 5-attribute vertex layout. Custom shaders authored before
/// this change must add the three new attributes (`rect_center`,
/// `rect_half_size`, `corner_radius`) and may early-return on
/// `corner_radius <= 0` to retain the v1 behavior.
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
    if (in.corner_radius <= 0.0) {
        return in.color;
    }
    // Standard rounded-box SDF (Inigo Quilez formulation).
    let p = in.clip_position.xy;
    let d = abs(p - in.rect_center) - in.rect_half_size + vec2<f32>(in.corner_radius);
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - in.corner_radius;
    // 1-pixel AA edge.
    let alpha = 1.0 - smoothstep(-0.5, 0.5, dist);
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

/// Image-rendering shader (passes the sampled texture RGBA straight through).
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
    return textureSample(img_texture, img_sampler, in.uv);
}
"#;

/// Text shader sampling the glyph atlas (the alpha channel masks the foreground color).
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
    let alpha = textureSample(glyph_texture, glyph_sampler, in.uv).a;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;
