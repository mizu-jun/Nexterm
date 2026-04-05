# Nexterm シェーダーギャラリー

Nexterm の GPU レンダラーは WGSL（WebGPU Shading Language）によるカスタムシェーダーをサポートしています。
背景（セル色）とテキスト（グリフ）の 2 種類のシェーダーを個別にカスタマイズできます。

## 設定方法

`~/.config/nexterm/config.toml` に以下を追加します:

```toml
[gpu]
# 背景（セル色）シェーダーのパス（省略時はビルトインを使用）
custom_bg_shader = "~/.config/nexterm/shaders/crt.wgsl"

# テキスト（グリフ）シェーダーのパス（省略時はビルトインを使用）
custom_text_shader = "~/.config/nexterm/shaders/amber_text.wgsl"
```

シェーダーファイルを保存すると自動的に再読み込みされます（ホットリロード対応）。

## 収録シェーダー

### 背景シェーダー

| ファイル | 効果 |
|---------|------|
| `crt.wgsl` | スキャンライン + ビネット効果。レトロ CRT モニター風 |
| `matrix.wgsl` | 全体をグリーンフォスフォレッセンスに染める Matrix 風 |
| `glow.wgsl` | 明るいセルにソフトな発光エフェクトを追加 |

### テキストシェーダー

| ファイル | 効果 |
|---------|------|
| `grayscale_text.wgsl` | 白黒変換。モノクロームのクラシックな外観 |
| `amber_text.wgsl` | 琥珀色変換。1980年代 CRT モニター風のアンバー色 |

## カスタムシェーダーの書き方

### 背景シェーダーのインターフェース

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,  // NDC座標 [-1, 1]
    @location(1) color: vec4<f32>,     // セルの背景色 RGBA [0, 1]
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput { ... }

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> { ... }
```

### テキストシェーダーのインターフェース

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,  // NDC座標
    @location(1) uv: vec2<f32>,        // グリフアトラス UV
    @location(2) color: vec4<f32>,     // 前景色 RGBA
}

// グリフアトラステクスチャ（アルファチャンネルにグリフマスク）
@group(0) @binding(0) var glyph_texture: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;
```

## 制約事項

- エントリポイントは `vs_main`（頂点）と `fs_main`（フラグメント）に固定
- 現時点では uniform バッファ（時刻・解像度）は渡されない
- シェーダーに構文エラーがある場合はビルトインシェーダーにフォールバックする
