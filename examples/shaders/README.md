# Nexterm Shader Gallery

Nexterm's GPU renderer supports custom shaders written in WGSL (WebGPU Shading Language).
You can customise two shader stages independently: the background (cell colours) and the text (glyphs).

## Configuration

Add the following to `~/.config/nexterm/config.toml`:

```toml
[gpu]
# Path to the background (cell colour) shader (built-in is used if omitted)
custom_bg_shader = "~/.config/nexterm/shaders/crt.wgsl"

# Path to the text (glyph) shader (built-in is used if omitted)
custom_text_shader = "~/.config/nexterm/shaders/amber_text.wgsl"
```

Shader files are reloaded automatically when saved (hot-reload supported).

## Included shaders

### Background shaders

| File | Effect |
|------|--------|
| `crt.wgsl` | Scanlines + vignette. Retro CRT monitor look |
| `matrix.wgsl` | Tints the whole screen with green phosphor — Matrix-style |
| `glow.wgsl` | Adds a soft bloom to bright cells |

### Text shaders

| File | Effect |
|------|--------|
| `grayscale_text.wgsl` | Black-and-white conversion. Classic monochrome look |
| `amber_text.wgsl` | Amber conversion. 1980s CRT-style amber tint |

## Writing a custom shader

### Background shader interface

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,  // NDC coordinates [-1, 1]
    @location(1) color: vec4<f32>,     // Cell background colour, RGBA [0, 1]
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

### Text shader interface

```wgsl
struct VertexInput {
    @location(0) position: vec2<f32>,  // NDC coordinates
    @location(1) uv: vec2<f32>,        // Glyph atlas UV
    @location(2) color: vec4<f32>,     // Foreground colour RGBA
}

// Glyph atlas texture (glyph mask in the alpha channel)
@group(0) @binding(0) var glyph_texture: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;
```

## Constraints

- Entry points are fixed: `vs_main` (vertex) and `fs_main` (fragment).
- No uniform buffers (time, resolution) are passed at present.
- If the shader has a syntax error, the renderer falls back to the built-in shader.
