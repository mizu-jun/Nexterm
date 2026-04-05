// Matrix 風グリーンフォスフォレッセンス背景シェーダー
//
// 背景色を緑系に染めて Matrix スタイルのターミナル風外観を実現する。
//
// 使用方法（~/.config/nexterm/config.toml）:
//   [gpu]
//   custom_bg_shader = "~/.config/nexterm/shaders/matrix.wgsl"

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

    // 輝度を計算してグリーンチャンネルに変換
    let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));

    // 背景色: 深い緑（0x001400 相当）にマッピング
    // 文字色は元の色を保持しつつ緑にシフト
    let green = vec3<f32>(0.0, luma * 1.2 + 0.05, 0.0);

    // 元の色が黒に近い（背景）場合は完全な緑黒にする
    let is_bg = step(0.05, dot(c.rgb, vec3<f32>(1.0)));
    let final_color = mix(vec3<f32>(0.0, 0.04, 0.0), green, is_bg);

    return vec4<f32>(final_color, c.a);
}
