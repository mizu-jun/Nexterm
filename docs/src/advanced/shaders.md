# カスタムシェーダー

Nexterm の GPU レンダラーは **WGSL（WebGPU Shading Language）** で書かれたカスタムシェーダーを
読み込む機能を備えています。ビジュアルエフェクト（CRT スキャンライン、グロー、ブラーなど）を
追加できます。

## 設定

```toml
# nexterm.toml

[gpu]
# 背景（セル背景色）シェーダー
custom_bg_shader = "~/.config/nexterm/shaders/bg.wgsl"

# テキスト（グリフ）シェーダー  
custom_text_shader = "~/.config/nexterm/shaders/text.wgsl"

# FPS 制限（0 = 制限なし、デフォルト: 60）
fps_limit = 60

# グリフアトラスサイズ（デフォルト: 2048）
# 高 DPI や大きなフォントサイズを使う場合は 4096 を推奨
atlas_size = 2048
```

---

## 背景シェーダーの仕様

### 頂点入力

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,  // NDC 座標 [-1, 1]
    @location(1) color: vec4<f32>,     // RGBA [0, 1]
}
```

### エントリポイント

```wgsl
@vertex fn vs_main(in: VertexInput) -> VertexOutput { ... }
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> { ... }
```

### デフォルトシェーダー（参考）

```wgsl
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
    return in.color;
}
```

---

## テキストシェーダーの仕様

### 頂点入力・バインディング

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,      // グリフアトラス UV 座標
    @location(2) color: vec4<f32>,   // 前景色 RGBA
}

// グリフアトラステクスチャ（2048×2048 RGBA）
@group(0) @binding(0) var glyph_texture: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;
```

---

## カスタムシェーダーの例

### CRT スキャンライン効果（背景シェーダー）

```wgsl
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
    // スキャンライン: 偶数ピクセル行を少し暗くする
    let scanline_factor = select(0.85, 1.0, (u32(in.clip_position.y) % 2u) == 0u);
    return vec4<f32>(in.color.rgb * scanline_factor, in.color.a);
}
```

### 発光（グロー）テキスト効果（テキストシェーダー）

```wgsl
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
    // 明るい文字にグロー効果を追加する
    let glow = pow(alpha, 0.5) * 0.3;
    let final_color = in.color.rgb + glow;
    return vec4<f32>(final_color, in.color.a * alpha);
}
```

---

## トラブルシューティング

シェーダーファイルが見つからない、またはコンパイルエラーが発生した場合は
ビルトインシェーダーに自動フォールバックします。エラーはログに出力されます:

```
WARN カスタム背景シェーダーの読み込みに失敗しました（ビルトインを使用）: /path/to/bg.wgsl: ...
```

WGSL の構文は [WebGPU Shading Language Spec](https://www.w3.org/TR/WGSL/) を参照してください。
