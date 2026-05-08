// グロー（発光）エフェクト背景シェーダー
//
// 明るい色の背景セルに対してソフトな発光効果を加える。
// ハイライト・選択範囲がより際立つ。
//
// 使用方法（~/.config/nexterm/config.toml）:
//   [gpu]
//   custom_bg_shader = "~/.config/nexterm/shaders/glow.wgsl"

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = in.color;

    // 輝度が高いほど彩度を上げてグロー感を演出する
    let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let glow_strength = smoothstep(0.3, 0.9, luma) * 0.25;

    // 明るい成分を少し増幅する
    let bloomed = c.rgb + c.rgb * glow_strength;

    // HDR クランプ（1.0 を超えないように）
    return vec4<f32>(min(bloomed, vec3<f32>(1.0)), c.a);
}
