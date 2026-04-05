// グレースケールテキストシェーダー
//
// すべてのテキストを白黒（輝度ベース）で描画する。
// ダークモード・フォーカスモードに適した落ち着いた外観。
//
// 使用方法（~/.config/nexterm/config.toml）:
//   [gpu]
//   custom_text_shader = "~/.config/nexterm/shaders/grayscale_text.wgsl"

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
    // 輝度変換（Rec. 709）
    let luma = dot(in.color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(vec3<f32>(luma), in.color.a * alpha);
}
