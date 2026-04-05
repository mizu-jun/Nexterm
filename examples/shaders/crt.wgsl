// CRT モニター風シェーダー（背景用）
//
// スキャンライン、ビネット（周辺減光）、RGBドット分離を模倣した
// レトロな CRT モニター風エフェクト。
//
// 使用方法（~/.config/nexterm/config.toml）:
//   [gpu]
//   custom_bg_shader = "~/.config/nexterm/shaders/crt.wgsl"

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
    let pos = in.clip_position.xy;
    let color = in.color;

    // スキャンライン: 2px ごとに輝度を 80% に下げる
    let scan = select(1.0, 0.8, (u32(pos.y) % 2u) == 0u);

    // ビネット: 中心から離れるほど暗くする
    // clip_position は物理ピクセル座標なので NDC に戻す
    let ndc = in.color.rg; // ここでは色で代用（本来は uniform でサイズを渡す）
    // シンプルなビネットはスクリーン端の色を少し落とすだけ
    let vign = clamp(1.0 - 0.1 * length(vec2<f32>(pos.x - 640.0, pos.y - 400.0) / 640.0), 0.7, 1.0);

    let final_color = color.rgb * scan * vign;
    return vec4<f32>(final_color, color.a);
}
