# Custom Shaders

Nexterm's GPU renderer supports custom **WGSL (WebGPU Shading Language)** shaders.
You can add visual effects such as CRT scanlines, glow, bloom, and more.

## Configuration

```toml
# nexterm.toml

[gpu]
# Background (cell background color) shader
custom_bg_shader = "~/.config/nexterm/shaders/bg.wgsl"

# Text (glyph) shader
custom_text_shader = "~/.config/nexterm/shaders/text.wgsl"

# FPS cap (0 = uncapped, default: 60)
fps_limit = 60

# Glyph atlas size (default: 2048)
# Use 4096 for high-DPI displays or large font sizes
atlas_size = 2048
```

---

## Background Shader Specification

### Vertex Input

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,  // NDC coordinates [-1, 1]
    @location(1) color: vec4<f32>,     // RGBA [0, 1]
}
```

### Entry Points

```wgsl
@vertex fn vs_main(in: VertexInput) -> VertexOutput { ... }
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> { ... }
```

### Default Shader (reference)

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

## Text Shader Specification

### Vertex Input & Bindings

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,      // Glyph atlas UV coordinates
    @location(2) color: vec4<f32>,   // Foreground color RGBA
}

// Glyph atlas texture (2048×2048 RGBA)
@group(0) @binding(0) var glyph_texture: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;
```

---

## Custom Shader Examples

### CRT Scanline Effect (background shader)

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
    // Scanline: dim every even pixel row slightly
    let scanline_factor = select(0.85, 1.0, (u32(in.clip_position.y) % 2u) == 0u);
    return vec4<f32>(in.color.rgb * scanline_factor, in.color.a);
}
```

### Glow Text Effect (text shader)

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
    // Add a glow halo to bright characters
    let glow = pow(alpha, 0.5) * 0.3;
    let final_color = in.color.rgb + glow;
    return vec4<f32>(final_color, in.color.a * alpha);
}
```

---

## Troubleshooting

If the shader file is missing or fails to compile, Nexterm automatically falls back to the
built-in shader. The error is logged:

```
WARN failed to load custom background shader (using built-in): /path/to/bg.wgsl: ...
```

Refer to the [WebGPU Shading Language Spec](https://www.w3.org/TR/WGSL/) for WGSL syntax.
