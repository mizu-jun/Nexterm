# P2b In-App Acrylic Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give overlay panels (dialogs, flyouts, tooltips) a translucent acrylic
material — blurred terminal content behind the panel, tinted by the active
color scheme — driven by a config-only, default-off toggle, with a settings
panel control added once the engine is verified.

**Architecture:** Add an offscreen `scene_color` capture of the terminal grid,
captured once per overlay-open transition (not per frame). Run a small Kawase
downsample/upsample blur chain over it. Extend `BgVertex` with one more
attribute (`acrylic_mix`) so the existing `bg_pipeline` can sample the blurred
result and tint it when drawing an overlay panel's fill, falling back to the
current opaque token fill when disabled. No change to the pipeline used when
no overlay is open.

**Tech Stack:** Rust, wgpu 0.20 (existing), WGSL, `nexterm-config` (TOML +
serde), `nexterm-i18n` (JSON locales), `wgpu::naga` for GPU-less shader
validation.

**Spec:** `docs/superpowers/specs/2026-08-22-p2b-in-app-acrylic-design.md`

## Global Constraints

- `cargo clippy -- -D warnings` and `cargo fmt --check` must pass before any commit that finishes a task.
- No `unwrap()` — use `?` or `expect("reason")` with a concrete message (project convention, `CLAUDE.md`).
- Every new user-facing string goes through `nexterm_i18n::fl!` and lands in all 8 locale files under `nexterm-i18n/locales/` (`de`, `en`, `es`, `fr`, `it`, `ja`, `ko`, `zh-CN`).
- `docs/CONFIGURATION.md` must document every new config key; `nexterm-config/tests/doc_matches_schema.rs` enforces this at the top-level-key granularity.
- GPU output is not CI-verifiable in this environment (no GPU available). Every task that touches rendering ends with `cargo clippy`/`cargo fmt`/the WGSL naga validation test/pure-function unit tests — never with a claim of visual correctness. Visual confirmation is explicitly deferred to the project's existing on-device verification backlog (Task 11).
- `in_app_blur_enabled` defaults to `false` (spec decision — unverified-on-real-GPU feature ships opt-in).
- Kawase tap count/chain shape is fixed by the plan (not user-configurable); only `in_app_blur_strength` is.

## PR 1 — Capture + blur engine + config (default off, no settings UI yet)

---

### Task 1: Config schema — `window.in_app_blur_enabled` / `window.in_app_blur_strength`

**Files:**
- Modify: `nexterm-config/src/schema/window.rs:267-330` (`WindowConfig` struct + its `impl Default`)
- Modify: `docs/CONFIGURATION.md:183-204` (`[window]` table) and `:980-983` (Complete Example)
- Test: `nexterm-config/src/schema/window.rs` (inline `#[cfg(test)]` module, follow existing convention in that file)

**Interfaces:**
- Produces: `WindowConfig.in_app_blur_enabled: bool` (default `false`), `WindowConfig.in_app_blur_strength: f32` (default `0.6`). Later tasks read these as `config.window.in_app_blur_enabled` / `config.window.in_app_blur_strength`, matching how `config.window.background_opacity` is read today.

- [ ] **Step 1: Write the failing test**

Add to `nexterm-config/src/schema/window.rs` (in its existing `#[cfg(test)]` module — if none exists yet, create one at the end of the file following the pattern of other schema files in the same crate):

```rust
#[cfg(test)]
mod acrylic_config_tests {
    use super::*;

    #[test]
    fn in_app_blur_defaults_to_disabled() {
        let cfg = WindowConfig::default();
        assert!(!cfg.in_app_blur_enabled);
        assert!((cfg.in_app_blur_strength - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn in_app_blur_round_trips_through_toml() {
        let toml_str = r#"
            in_app_blur_enabled = true
            in_app_blur_strength = 0.25
        "#;
        let cfg: WindowConfig = toml::from_str(toml_str).expect("valid partial WindowConfig");
        assert!(cfg.in_app_blur_enabled);
        assert!((cfg.in_app_blur_strength - 0.25).abs() < f32::EPSILON);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-config in_app_blur --lib`
Expected: FAIL — `no field \`in_app_blur_enabled\` on type \`WindowConfig\`` (compile error).

- [ ] **Step 3: Write minimal implementation**

In `nexterm-config/src/schema/window.rs`, add two fields to `WindowConfig` (after `close_action`, following the struct's existing doc-comment style):

```rust
    /// Behavior when the OS Window is closed (Sprint 5-7 / Phase 4-1).
    /// One of `prompt` / `detach` / `kill`. Default: `prompt`.
    /// See [`CloseAction`] for details.
    #[serde(default)]
    pub close_action: CloseAction,
    /// Enable the in-app acrylic material for overlay panels (dialogs,
    /// flyouts, tooltips): an offscreen Kawase blur of the terminal grid,
    /// tinted by the active scheme (UI/UX v3 P2b). Opt-in and off by
    /// default — this environment has no GPU to verify the visual result
    /// against, so the feature ships unverified-by-default rather than
    /// on-by-default.
    #[serde(default)]
    pub in_app_blur_enabled: bool,
    /// Blend ratio between the existing opaque panel fill (0.0) and the
    /// full blur+tint acrylic material (1.0). Only meaningful when
    /// `in_app_blur_enabled` is true. Does not affect the fixed procedural
    /// noise grain (UI/UX v3 P2b).
    #[serde(default = "default_in_app_blur_strength")]
    pub in_app_blur_strength: f32,
```

Add the default fn and wire the `impl Default` block:

```rust
fn default_in_app_blur_strength() -> f32 {
    0.6
}
```

```rust
impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            background_opacity: default_background_opacity(),
            macos_window_background_blur: 0,
            decorations: WindowDecorations::default(),
            layout_mode: default_layout_mode(),
            padding_x: 0,
            padding_y: 0,
            background_image: None,
            gradient: None,
            close_action: CloseAction::default(),
            in_app_blur_enabled: false,
            in_app_blur_strength: default_in_app_blur_strength(),
        }
    }
}
```

Following the existing `background_opacity` convention in this same struct, no clamp helper is added at the schema layer; out-of-range values are clamped where the value is consumed (Task 7's render-time read, and Task 11's settings-panel slider, mirroring `set_opacity_value`'s `clamp(0.1, 1.0)` pattern).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-config in_app_blur --lib`
Expected: PASS (2 tests).

- [ ] **Step 5: Document the new keys and commit**

Add to `docs/CONFIGURATION.md`'s `[window]` table (after the `decorations` row, `:189`):

```markdown
| `in_app_blur_enabled` | bool | `false` | Enable the in-app acrylic material (blurred terminal behind overlay panels). Opt-in — unverified on real GPU hardware as of this writing |
| `in_app_blur_strength` | float | `0.6` | Blend ratio between the opaque panel fill (0.0) and the full blur+tint acrylic material (1.0). Only used when `in_app_blur_enabled` is true |
```

Add to the `[window]` block in the "Complete nexterm.toml Example" section (`:980-983`):

```toml
[window]
background_opacity = 0.95
macos_window_background_blur = 0
decorations = "notitle"
in_app_blur_enabled = false
in_app_blur_strength = 0.6
```

```bash
cargo fmt --check -p nexterm-config
cargo clippy -p nexterm-config -- -D warnings
git add nexterm-config/src/schema/window.rs docs/CONFIGURATION.md
git commit -m "feat(config): add window.in_app_blur_enabled/strength (P2b)"
```

---

### Task 2: Capture-invalidation state machine (pure, no GPU)

**Files:**
- Create: `nexterm-client-gpu/src/renderer/acrylic.rs`
- Modify: `nexterm-client-gpu/src/renderer/mod.rs` (add `mod acrylic;`)
- Test: inline `#[cfg(test)]` module in `acrylic.rs`

**Interfaces:**
- Produces: `AcrylicCaptureState` with `note_overlay_open_count(count: usize)`, `note_resize()`, `is_dirty(&self, overlay_open: bool) -> bool`, `mark_captured(&mut self)`. Task 7 (render_frame.rs wiring) calls these each frame.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_from_zero_to_one_overlay_is_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(0);
        assert!(!state.is_dirty(false));
        state.note_overlay_open_count(1);
        assert!(state.is_dirty(true));
    }

    #[test]
    fn staying_open_with_more_overlays_is_not_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        assert!(!state.is_dirty(true));
        state.note_overlay_open_count(2);
        assert!(!state.is_dirty(true));
    }

    #[test]
    fn resize_while_open_marks_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        assert!(!state.is_dirty(true));
        state.note_resize();
        assert!(state.is_dirty(true));
    }

    #[test]
    fn resize_while_closed_does_not_force_a_capture() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(0);
        state.note_resize();
        assert!(!state.is_dirty(false));
    }

    #[test]
    fn closing_and_reopening_recaptures() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        state.note_overlay_open_count(0);
        state.note_overlay_open_count(1);
        assert!(state.is_dirty(true));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-client-gpu acrylic:: --lib`
Expected: FAIL — `cannot find type \`AcrylicCaptureState\` in this scope`.

- [ ] **Step 3: Write minimal implementation**

```rust
//! In-app acrylic capture bookkeeping (UI/UX v3 P2b). Pure state — no wgpu
//! types — so the invalidation rules are unit-testable without a GPU.

/// Tracks when the offscreen `scene_color` capture needs to be redone.
/// The capture is a frozen snapshot taken once per overlay-open
/// transition; it is intentionally *not* refreshed every frame while an
/// overlay stays open (see the design spec's "Non-goals").
#[derive(Debug, Default)]
pub(crate) struct AcrylicCaptureState {
    last_overlay_open_count: usize,
    generation: u64,
    captured_generation: Option<u64>,
    captured_while_open: bool,
}

impl AcrylicCaptureState {
    /// Call once per frame with how many overlays are currently open.
    pub(crate) fn note_overlay_open_count(&mut self, count: usize) {
        let was_open = self.last_overlay_open_count > 0;
        let now_open = count > 0;
        if !was_open && now_open {
            // 0 -> N transition: force a fresh capture.
            self.captured_generation = None;
        }
        self.last_overlay_open_count = count;
    }

    /// Call whenever the window resizes or the DPI scale changes.
    pub(crate) fn note_resize(&mut self) {
        self.generation += 1;
    }

    /// Whether the caller should (re-)capture `scene_color` and re-run the
    /// blur chain this frame. `overlay_open` must match what
    /// `note_overlay_open_count` was last told.
    pub(crate) fn is_dirty(&self, overlay_open: bool) -> bool {
        overlay_open && self.captured_generation != Some(self.generation)
    }

    /// Call after a capture + blur pass has run this frame.
    pub(crate) fn mark_captured(&mut self) {
        self.captured_generation = Some(self.generation);
        self.captured_while_open = true;
        let _ = self.captured_while_open; // silence unused-field lint until Task 7 reads it
    }
}
```

Register the module in `nexterm-client-gpu/src/renderer/mod.rs`:

```rust
mod acrylic;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu acrylic:: --lib`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/renderer/acrylic.rs nexterm-client-gpu/src/renderer/mod.rs
git commit -m "feat(client): add acrylic capture-invalidation state machine (P2b)"
```

---

### Task 3: Kawase tap-offset pure function

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/acrylic.rs`
- Test: same file, `#[cfg(test)]`

**Interfaces:**
- Consumes: nothing new.
- Produces: `kawase_downsample_offsets(texel_size: (f32, f32)) -> [(f32, f32); 4]`, `kawase_upsample_offsets(texel_size: (f32, f32)) -> [(f32, f32); 8]`. Task 5's WGSL shader mirrors these exact offsets/weights so the CPU-testable math and the GPU shader agree by construction (the test is the spec for the shader, not a duplicate of it).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod offset_tests {
    use super::*;

    #[test]
    fn downsample_offsets_are_symmetric_half_texel() {
        let offsets = kawase_downsample_offsets((2.0, 4.0));
        // half-texel = (1.0, 2.0); four diagonal corners.
        assert_eq!(offsets, [(-1.0, -2.0), (1.0, -2.0), (-1.0, 2.0), (1.0, 2.0)]);
    }

    #[test]
    fn upsample_offsets_are_symmetric_full_and_double_texel() {
        let offsets = kawase_upsample_offsets((2.0, 4.0));
        assert_eq!(offsets.len(), 8);
        // The four axis-aligned double-distance taps come first by
        // construction, then the four single-distance diagonal taps.
        assert_eq!(offsets[0], (-4.0, 0.0));
        assert_eq!(offsets[4], (-2.0, 2.0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-client-gpu offset_tests --lib`
Expected: FAIL — function not found.

- [ ] **Step 3: Write minimal implementation**

```rust
/// The 4 diagonal taps of one dual-Kawase downsample pass, in texels.
/// Mirrors the WGSL `kawase_downsample` entry point in `shaders.rs` —
/// keep the two in sync.
pub(crate) fn kawase_downsample_offsets(texel_size: (f32, f32)) -> [(f32, f32); 4] {
    let (hx, hy) = (texel_size.0 * 0.5, texel_size.1 * 0.5);
    [(-hx, -hy), (hx, -hy), (-hx, hy), (hx, hy)]
}

/// The 8 taps of one dual-Kawase upsample pass (4 axis-aligned taps at
/// 2 texels, weight 1; 4 diagonal taps at 1 texel, weight 2 — applied by
/// the shader, not encoded here). Mirrors `kawase_upsample` in
/// `shaders.rs`.
pub(crate) fn kawase_upsample_offsets(texel_size: (f32, f32)) -> [(f32, f32); 8] {
    let (x, y) = texel_size;
    [
        (-2.0 * x, 0.0),
        (2.0 * x, 0.0),
        (0.0, -2.0 * y),
        (0.0, 2.0 * y),
        (-x, y),
        (x, y),
        (-x, -y),
        (x, -y),
    ]
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu offset_tests --lib`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/renderer/acrylic.rs
git commit -m "feat(client): add Kawase tap-offset pure functions (P2b)"
```

---

### Task 4: `BgVertex` gains `acrylic_mix`; WGSL sampling branch in `BG_SHADER`

**Files:**
- Modify: `nexterm-client-gpu/src/glyph_atlas.rs:18-38` (`BgVertex`)
- Modify: `nexterm-client-gpu/src/renderer/wgpu_init.rs:145-191` (`bg_pipeline` vertex layout + bind group layout)
- Modify: `nexterm-client-gpu/src/renderer/shader_reload.rs:80-107` (mirror the same vertex layout + bind group layout)
- Modify: `nexterm-client-gpu/src/shaders.rs:35-95` (`BG_SHADER`) and `:164-189` (naga test)
- Test: `nexterm-client-gpu/src/shaders.rs` (extend the existing `builtin_shaders_parse_and_validate` test — no new test needed, the shader source change is covered by re-running it)

**Interfaces:**
- Produces: `BgVertex.acrylic_mix: f32` (8th attribute, `@location(7)`). `AcrylicUniform` WGSL struct (`tint: vec4<f32>`, `viewport_size: vec2<f32>`, `strength: f32`, `_pad: f32`) bound at `@group(0) @binding(2)`, alongside `@group(0) @binding(0) var acrylic_tex: texture_2d<f32>` and `@group(0) @binding(1) var acrylic_sampler: sampler`. Task 6 creates the actual `wgpu::BindGroupLayout`/`BindGroup` this shader now requires; Task 7 binds it.

- [ ] **Step 1: Write the failing test**

The `builtin_shaders_parse_and_validate` test in `shaders.rs:164-189` already parses `BG_SHADER` through naga. Run it now, before editing, to confirm the baseline is green, then make the edit and re-run — the "failing" state here is a compile error in `BgVertex`/`wgpu_init.rs` (Rust) rather than a WGSL validation failure, since we edit both together. Run:

`cargo build -p nexterm-client-gpu` after Step 3's `BgVertex` edit but before its `wgpu_init.rs` counterpart — expect a `mismatched types` / array-length error in `vertex_attr_array!` call sites, confirming the two must move together.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo build -p nexterm-client-gpu`
Expected: FAIL after editing only `BgVertex` (Step 3a below) — `wgpu_init.rs`'s `vertex_attr_array![0 => ..., ..., 6 => Float32]` no longer matches `size_of::<BgVertex>()`'s new stride, caught at pipeline-creation runtime rather than compile time in practice, so instead treat "the shader source no longer matches Rust's field count" as the thing Step 4's naga test plus a manual `cargo build` catch. Proceed with all sub-steps of Step 3 together (they're one logical change) then verify.

- [ ] **Step 3: Write minimal implementation**

`glyph_atlas.rs` — add the field:

```rust
    /// Outline band width in pixels (UI/UX v3 P2a). `> 0.0` paints only a
    /// stroke hugging the inside of the rect edge instead of a fill.
    pub stroke_width: f32,
    /// Acrylic blend factor in `0.0..=1.0` (UI/UX v3 P2b). `0.0` (the
    /// default for every non-overlay vertex) draws the flat `color` as
    /// today; `> 0.0` mixes in the blurred/tinted `scene_color` sample by
    /// this amount. Only overlay panel fills ever set this to non-zero.
    pub acrylic_mix: f32,
```

`wgpu_init.rs` — extend the vertex attribute array (7 -> 8 attributes) and give `bg_pipeline` a bind group layout it didn't have before:

```rust
        let acrylic_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("acrylic_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let bg_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg_pipeline_layout"),
            bind_group_layouts: &[&acrylic_bind_group_layout],
            push_constant_ranges: &[],
        });
```

(the existing `bg_pipeline_layout` binding, wherever it was previously created with an empty `bind_group_layouts: &[]`, is replaced by the block above — keep everything else about `bg_pipeline`'s `RenderPipelineDescriptor` unchanged) and update its vertex buffer layout:

```rust
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BgVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    // Sprint 5-15 / UI/UX v2 Phase 1, extended by UI/UX v3
                    // P2a (shadow_softness, stroke_width) and P2b
                    // (acrylic_mix). Must stay in sync with the reload
                    // layout in `shader_reload.rs`.
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x4,
                        2 => Float32x2,
                        3 => Float32x2,
                        4 => Float32,
                        5 => Float32,
                        6 => Float32,
                        7 => Float32,
                    ],
                }],
```

`shader_reload.rs` — apply the identical `attributes: &wgpu::vertex_attr_array![...]` change (0-7, same as above) to keep the hot-reload path from failing pipeline validation on a stale 7-attribute layout, and rebuild `bg_layout` there the same way (bind group layout with the 3 entries above) so a reload doesn't drop the acrylic bind group.

`shaders.rs` `BG_SHADER` — add the bindings, the vertex field, and the fragment branch:

```wgsl
struct AcrylicUniform {
    tint: vec4<f32>,
    viewport_size: vec2<f32>,
    strength: f32,
    _pad: f32,
}

@group(0) @binding(0) var acrylic_tex: texture_2d<f32>;
@group(0) @binding(1) var acrylic_sampler: sampler;
@group(0) @binding(2) var<uniform> acrylic: AcrylicUniform;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) rect_center: vec2<f32>,
    @location(3) rect_half_size: vec2<f32>,
    @location(4) corner_radius: f32,
    @location(5) shadow_softness: f32,
    @location(6) stroke_width: f32,
    @location(7) acrylic_mix: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) rect_center: vec2<f32>,
    @location(2) rect_half_size: vec2<f32>,
    @location(3) corner_radius: f32,
    @location(4) shadow_softness: f32,
    @location(5) stroke_width: f32,
    @location(6) acrylic_mix: f32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    out.rect_center = in.rect_center;
    out.rect_half_size = in.rect_half_size;
    out.corner_radius = in.corner_radius;
    out.shadow_softness = in.shadow_softness;
    out.stroke_width = in.stroke_width;
    out.acrylic_mix = in.acrylic_mix;
    return out;
}

// Cheap hash-based procedural noise (UI/UX v3 P2b) — no texture asset, no
// licensing question. Fixed intensity, not tied to `acrylic.strength`.
fn acrylic_noise(p: vec2<f32>) -> f32 {
    let h = fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    return (h - 0.5) * 0.03; // +-1.5% luma grain
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var base_color = in.color;
    if (in.acrylic_mix > 0.0) {
        let uv = in.clip_position.xy / acrylic.viewport_size;
        let blurred = textureSample(acrylic_tex, acrylic_sampler, uv);
        let tinted = mix(blurred.rgb, acrylic.tint.rgb, acrylic.strength);
        let grain = acrylic_noise(in.clip_position.xy);
        base_color = vec4<f32>(
            mix(in.color.rgb, tinted + vec3<f32>(grain), in.acrylic_mix),
            in.color.a,
        );
    }
    // Output is premultiplied alpha (see the custom-shader contract above).
    if (in.corner_radius <= 0.0 && in.shadow_softness <= 0.0 && in.stroke_width <= 0.0) {
        return vec4<f32>(base_color.rgb * base_color.a, base_color.a);
    }
    let p = in.clip_position.xy;
    let d = abs(p - in.rect_center) - in.rect_half_size + vec2<f32>(in.corner_radius);
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - in.corner_radius;

    var coverage: f32;
    if (in.stroke_width > 0.0) {
        let half_w = in.stroke_width * 0.5;
        coverage = 1.0 - smoothstep(half_w - 0.5, half_w + 0.5, abs(dist + half_w));
    } else {
        let spread = max(in.shadow_softness, 0.5);
        coverage = 1.0 - smoothstep(-spread, spread, dist);
    }
    let alpha = base_color.a * coverage;
    return vec4<f32>(base_color.rgb * alpha, alpha);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu builtin_shaders_parse_and_validate --lib`
Expected: PASS — confirms the edited `BG_SHADER` still parses and validates through naga with the new binding/attribute additions.

Run: `cargo build -p nexterm-client-gpu`
Expected: builds cleanly (confirms `wgpu_init.rs` and `shader_reload.rs` attribute arrays match `BgVertex`'s new size, and the new pipeline layout compiles).

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/glyph_atlas.rs nexterm-client-gpu/src/renderer/wgpu_init.rs nexterm-client-gpu/src/renderer/shader_reload.rs nexterm-client-gpu/src/shaders.rs
git commit -m "feat(client): extend BgVertex with acrylic_mix, sample blur in BG_SHADER (P2b)"
```

---

### Task 5: Kawase blur WGSL shader (downsample + upsample passes)

**Files:**
- Modify: `nexterm-client-gpu/src/shaders.rs`
- Test: extend `builtin_shaders_parse_and_validate` (`:164-189`) to include the new shader

**Interfaces:**
- Consumes: `kawase_downsample_offsets` / `kawase_upsample_offsets` from Task 3 as the *spec* for these WGSL entry points (kept in sync by comment cross-reference, not shared code — WGSL and Rust can't share a function).
- Produces: `KAWASE_BLUR_SHADER: &str` with two entry points, `fs_downsample` and `fs_upsample`, both taking a fullscreen triangle from a shared `vs_fullscreen` vertex stage. Task 6 creates two `wgpu::RenderPipeline`s from this one shader module.

- [ ] **Step 1: Write the failing test**

Extend the existing test in `shaders.rs`:

```rust
    #[test]
    fn builtin_shaders_parse_and_validate() {
        for (name, src) in [
            ("bg", BG_SHADER),
            ("image", IMAGE_SHADER),
            ("text", TEXT_SHADER),
            ("kawase_blur", KAWASE_BLUR_SHADER),
        ] {
            let module = wgpu::naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("{name} shader failed to parse: {e}"));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name} shader failed validation: {e:?}"));
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-client-gpu builtin_shaders_parse_and_validate --lib`
Expected: FAIL — `cannot find value \`KAWASE_BLUR_SHADER\` in this scope`.

- [ ] **Step 3: Write minimal implementation**

```rust
/// Dual-Kawase blur (Bjørge, "Bandwidth-Efficient Rendering", 2015; the
/// same downsample/upsample tap pattern Godot and Unity's URP use for
/// cheap glass-blur effects). UI/UX v3 P2b: the downsample pass halves
/// resolution each step (4 taps), the upsample pass doubles it back up
/// (8 taps, weighted 1/1/1/1 axis-aligned + 2/2/2/2 diagonal). Tap
/// offsets here must match `acrylic::kawase_downsample_offsets` /
/// `kawase_upsample_offsets` in `renderer/acrylic.rs`.
pub(crate) const KAWASE_BLUR_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// A single oversized triangle covering the full viewport — cheaper than a
// quad (no index buffer, no diagonal seam) for a fullscreen pass.
@vertex
fn vs_fullscreen(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(i32(idx) - 1) * 2.0;
    let y = f32(i32(idx & 1u) * 2 - 1) * 2.0;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    return out;
}

struct BlurParams {
    texel_size: vec2<f32>,
    _pad: vec2<f32>,
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;

@fragment
fn fs_downsample(in: VertexOutput) -> @location(0) vec4<f32> {
    let h = params.texel_size * 0.5;
    var c = textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-h.x, -h.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(h.x, -h.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-h.x, h.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(h.x, h.y));
    return c * 0.25;
}

@fragment
fn fs_upsample(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = params.texel_size;
    var c = textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-2.0 * t.x, 0.0));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(2.0 * t.x, 0.0));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(0.0, -2.0 * t.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(0.0, 2.0 * t.y));
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-t.x, t.y)) * 2.0;
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(t.x, t.y)) * 2.0;
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(-t.x, -t.y)) * 2.0;
    c += textureSample(src_tex, src_sampler, in.uv + vec2<f32>(t.x, -t.y)) * 2.0;
    return c * (1.0 / 12.0);
}
"#;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu builtin_shaders_parse_and_validate --lib`
Expected: PASS (4 shaders validated).

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/shaders.rs
git commit -m "feat(client): add dual-Kawase blur WGSL shader (P2b)"
```

---

### Task 6: `AcrylicState` GPU resources (textures, pipelines, bind groups)

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/acrylic.rs`
- Modify: `nexterm-client-gpu/src/renderer/mod.rs` (`WgpuState` gains an `acrylic: AcrylicState` field and a 1x1 placeholder texture created at `new()`, mirroring how `background: Option<BackgroundTexture>` is a field but created lazily)

**Interfaces:**
- Consumes: `KAWASE_BLUR_SHADER` (Task 5), the `acrylic_bind_group_layout` shape from Task 4 (texture + sampler + uniform), `kawase_downsample_offsets`/`kawase_upsample_offsets` (Task 3, referenced only in comments — the shader hardcodes the same math).
- Produces: `AcrylicState::new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self`, `AcrylicState::ensure_size(&mut self, device: &wgpu::Device, width: u32, height: u32)`, `AcrylicState::placeholder_bind_group(&self) -> &wgpu::BindGroup` (the always-valid 1x1 fallback so `bg_pipeline` always has something bound), and the real capture/blur/composite bind groups that Task 7 selects between based on `AcrylicCaptureState::is_dirty`.

- [ ] **Step 1: Write the failing test**

GPU resource creation cannot run without a `wgpu::Device`, so there is no CPU-only red/green cycle for this task's main body — this is the one task in this plan whose correctness is *shape*-verified (it compiles, matches the bind group layout Task 4 declared) rather than *behavior*-verified, consistent with the project's existing "GPU output is not CI-verifiable" convention (see `shadow_params`'s doc comment in `overlay/util.rs` for precedent). The test that *can* run is a `size_of`/layout sanity check:

```rust
#[cfg(test)]
mod resource_shape_tests {
    use super::*;

    #[test]
    fn blur_uniform_matches_wgsl_struct_layout() {
        // BlurParams in KAWASE_BLUR_SHADER is { texel_size: vec2<f32>, _pad: vec2<f32> }
        // = 16 bytes, satisfying wgpu's 16-byte uniform alignment without
        // an explicit #[repr(align(16))].
        assert_eq!(std::mem::size_of::<BlurParamsUniform>(), 16);
    }

    #[test]
    fn acrylic_uniform_matches_wgsl_struct_layout() {
        // AcrylicUniform in BG_SHADER is { tint: vec4<f32>, viewport_size: vec2<f32>, strength: f32, _pad: f32 } = 32 bytes.
        assert_eq!(std::mem::size_of::<AcrylicUniformData>(), 32);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-client-gpu resource_shape_tests --lib`
Expected: FAIL — `BlurParamsUniform` / `AcrylicUniformData` not defined.

- [ ] **Step 3: Write minimal implementation**

```rust
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct BlurParamsUniform {
    pub texel_size: [f32; 2],
    pub _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct AcrylicUniformData {
    pub tint: [f32; 4],
    pub viewport_size: [f32; 2],
    pub strength: f32,
    pub _pad: f32,
}

/// One level of the Kawase ping-pong chain: a texture, its view, and the
/// bind group that samples the *previous* level into it.
struct BlurLevel {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl BlurLevel {
    fn create(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32, label: &str) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view, width: width.max(1), height: height.max(1) }
    }
}

/// Offscreen resources for the in-app acrylic material (UI/UX v3 P2b).
/// Created once at `WgpuState::new` with a 1x1 placeholder so `bg_pipeline`
/// always has a valid bind group, resized (recreated) whenever the surface
/// size changes.
pub(crate) struct AcrylicState {
    format: wgpu::TextureFormat,
    scene_color: BlurLevel,
    half_res: BlurLevel,
    quarter_res: BlurLevel,
    blurred_result: BlurLevel,
    sampler: wgpu::Sampler,
    blur_bind_group_layout: wgpu::BindGroupLayout,
    blur_pipeline_layout: wgpu::PipelineLayout,
    downsample_pipeline: wgpu::RenderPipeline,
    upsample_pipeline: wgpu::RenderPipeline,
    acrylic_bind_group_layout: wgpu::BindGroupLayout,
    /// Bound to `bg_pipeline`'s group 0 every frame; points at
    /// `blurred_result` once a capture has run, or at a 1x1 transparent
    /// placeholder before the first capture / when the feature is off.
    acrylic_bind_group: wgpu::BindGroup,
    acrylic_uniform_buf: wgpu::Buffer,
    placeholder_texture: wgpu::Texture,
    placeholder_view: wgpu::TextureView,
}

impl AcrylicState {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("acrylic_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let placeholder_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("acrylic_placeholder"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let placeholder_view = placeholder_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let acrylic_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("acrylic_sample_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let acrylic_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acrylic_uniform"),
            size: std::mem::size_of::<AcrylicUniformData>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let acrylic_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acrylic_sample_bind_group"),
            layout: &acrylic_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&placeholder_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Buffer(acrylic_uniform_buf.as_entire_buffer_binding()) },
            ],
        });

        let blur_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kawase_blur_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kawase_blur_pipeline_layout"),
            bind_group_layouts: &[&blur_bind_group_layout],
            push_constant_ranges: &[],
        });

        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kawase_blur_shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::KAWASE_BLUR_SHADER.into()),
        });

        let make_blur_pipeline = |entry_point: &str, label: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&blur_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &blur_shader,
                    entry_point: "vs_fullscreen",
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &blur_shader,
                    entry_point,
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let downsample_pipeline = make_blur_pipeline("fs_downsample", "kawase_downsample_pipeline");
        let upsample_pipeline = make_blur_pipeline("fs_upsample", "kawase_upsample_pipeline");

        Self {
            format,
            scene_color: BlurLevel::create(device, format, 1, 1, "acrylic_scene_color"),
            half_res: BlurLevel::create(device, format, 1, 1, "acrylic_half_res"),
            quarter_res: BlurLevel::create(device, format, 1, 1, "acrylic_quarter_res"),
            blurred_result: BlurLevel::create(device, format, 1, 1, "acrylic_blurred_result"),
            sampler,
            blur_bind_group_layout,
            blur_pipeline_layout,
            downsample_pipeline,
            upsample_pipeline,
            acrylic_bind_group_layout,
            acrylic_bind_group,
            acrylic_uniform_buf,
            placeholder_texture,
            placeholder_view,
        }
    }

    /// Recreate the offscreen chain at the new surface size. Cheap to call
    /// unconditionally on every resize — it only reallocates when the size
    /// actually changed.
    pub(crate) fn ensure_size(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.scene_color.width == width && self.scene_color.height == height {
            return;
        }
        self.scene_color = BlurLevel::create(device, self.format, width, height, "acrylic_scene_color");
        self.half_res = BlurLevel::create(device, self.format, (width / 2).max(1), (height / 2).max(1), "acrylic_half_res");
        self.quarter_res = BlurLevel::create(device, self.format, (width / 4).max(1), (height / 4).max(1), "acrylic_quarter_res");
        self.blurred_result = BlurLevel::create(device, self.format, width, height, "acrylic_blurred_result");
    }
}
```

`blur_bind_group_layout` deliberately mirrors `acrylic_bind_group_layout`'s shape one-for-one (texture, sampler, uniform) — the blur chain's ping-pong passes and the final composite sample both need the same three bindings, just pointed at different textures/buffers per level.

Add `AcrylicState` as a field on `WgpuState` (`renderer/mod.rs`) and initialize it in `WgpuState::new` (`wgpu_init.rs`) right after `bg_pipeline` is created, passing `surface_config.format`. Wire `WgpuState::resize` (`wgpu_init.rs:385-392`) to also call `self.acrylic.ensure_size(&self.device, new_size.width, new_size.height)`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu resource_shape_tests --lib`
Expected: PASS (2 tests).

Run: `cargo build -p nexterm-client-gpu`
Expected: builds cleanly — this is the real signal for this task, since the shape tests only cover the two uniform structs, not the bulk of the GPU resource wiring above.

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/renderer/acrylic.rs nexterm-client-gpu/src/renderer/mod.rs nexterm-client-gpu/src/renderer/wgpu_init.rs
git commit -m "feat(client): add AcrylicState offscreen textures and blur pipelines (P2b)"
```

---

### Task 7: Wire capture + blur + composite into `render_frame.rs`

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/render_frame.rs` (grid-layer second pass at ~1287, blur chain execution, `main_render_pass`'s `bg_pipeline` bind group at ~1289-1356)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/window.rs:236-302` (`on_resized` already calls `wgpu.resize` which now cascades into `AcrylicState::ensure_size` via Task 6 — no change needed there beyond confirming it)

**Interfaces:**
- Consumes: `AcrylicCaptureState` (Task 2), `AcrylicState` (Task 6), `config.window.in_app_blur_enabled` / `in_app_blur_strength` (Task 1), the `overlay_bg_start`/`overlay_text_start` markers already computed in `render_frame.rs:856-865`.
- Produces: nothing new for later tasks — this is where the engine becomes observable at runtime (behind the still-default-off config flag).

- [ ] **Step 1: Write the failing test**

This task's logic — "how many overlays are open right now" — has no single existing accessor (confirmed: no `any_overlay_open()` function exists in `ClientState`). Write it test-first as a small free function, since it is pure and pane of the render path:

```rust
// In render_frame.rs, near the top-level helpers.
#[cfg(test)]
mod overlay_count_tests {
    use super::*;

    #[test]
    fn counts_each_independent_overlay_flag() {
        assert_eq!(count_open_overlays(false, false, false, false, false, false), 0);
        assert_eq!(count_open_overlays(true, false, false, false, false, false), 1);
        assert_eq!(count_open_overlays(true, true, false, false, false, false), 2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-client-gpu counts_each_independent_overlay_flag --lib`
Expected: FAIL — `count_open_overlays` not found.

- [ ] **Step 3: Write minimal implementation**

```rust
/// How many of the mutually-independent overlay surfaces are open right
/// now (UI/UX v3 P2b — there is no single "any overlay open" flag in
/// `ClientState`, so this counts the ones acrylic capture cares about).
/// Order matches nothing in particular; only the count matters.
fn count_open_overlays(
    context_menu_open: bool,
    host_manager_open: bool,
    macro_picker_open: bool,
    file_transfer_open: bool,
    settings_panel_open: bool,
    palette_open: bool,
) -> u32 {
    [
        context_menu_open,
        host_manager_open,
        macro_picker_open,
        file_transfer_open,
        settings_panel_open,
        palette_open,
    ]
    .into_iter()
    .filter(|open| *open)
    .count() as u32
}
```

Call it where `render()` already has access to `state` (near the `overlay_bg_start` marker, `render_frame.rs:856`), plus the dialog-shaped overlays that are `Option<T>` rather than a `.is_open` bool (`pending_consent`, `close_window_dialog`) added as extra `bool` parameters the same way:

```rust
        let overlay_open_count = count_open_overlays(
            state.context_menu.is_some(),
            state.host_manager.is_open,
            state.macro_picker.is_open,
            state.file_transfer.is_open,
            state.settings_panel.is_open,
            state.palette.is_open,
        ) + state.pending_consent.is_some() as u32
            + state.close_window_dialog.is_some() as u32
            + state.host_manager.password_modal.is_some() as u32;
        self.acrylic_capture.note_overlay_open_count(overlay_open_count as usize);
        let overlay_open = overlay_open_count > 0;
        let blur_enabled = self.app_config_window_in_app_blur_enabled; // passed into render() alongside background_opacity, mirroring how that flag already reaches this function
```

(`WgpuState` gains an `acrylic_capture: AcrylicCaptureState` field, initialized via `AcrylicCaptureState::default()` in `WgpuState::new`; `render()`'s existing parameter list, which already takes `background_opacity: f32`, gains `in_app_blur_enabled: bool` and `in_app_blur_strength: f32` the same way — check the call site in `renderer/mod.rs` or wherever `render()` is invoked per-frame and thread the two new config reads through identically to how `background_opacity` already flows from `config.window.background_opacity`.)

Insert the capture + blur pass between the existing grid-to-swapchain draw and `main_render_pass` (i.e., right after the point where `overlay_bg_start`/`overlay_text_start` are captured, and before `main_render_pass` begins, since `main_render_pass` is what actually issues the `bg_pipeline` draws — the new offscreen pass must run *before* it so `blurred_result` is ready when the overlay layer's fragment shader samples it in the same `main_render_pass`):

```rust
        if blur_enabled && overlay_open && self.acrylic_capture.is_dirty(overlay_open) {
            self.acrylic.ensure_size(&self.device, self.surface_config.width, self.surface_config.height);
            {
                let mut capture_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("acrylic_capture_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.acrylic.scene_color.view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                // Re-draw only the grid-layer bg range (index 0..overlay_bg_start) —
                // the same vertex/index buffers main_render_pass will use below,
                // just targeting the offscreen texture instead of the swapchain.
                capture_pass.set_pipeline(&self.bg_pipeline);
                capture_pass.set_bind_group(0, &self.acrylic.acrylic_bind_group, &[]);
                capture_pass.set_vertex_buffer(0, self.buf_bg_v.slice(..));
                capture_pass.set_index_buffer(self.buf_bg_i.slice(..), wgpu::IndexFormat::Uint16);
                capture_pass.draw_indexed(0..(overlay_bg_start as u32), 0, 0..1);
            }
            self.acrylic.run_blur_chain(&mut encoder);
            self.acrylic_capture.mark_captured();
        }
```

Add `AcrylicState::run_blur_chain(&self, encoder: &mut wgpu::CommandEncoder)` in `acrylic.rs`, issuing 2 downsample passes (`scene_color -> half_res -> quarter_res`) then 2 upsample passes (`quarter_res -> half_res -> blurred_result`), each pass a `begin_render_pass` using `downsample_pipeline`/`upsample_pipeline` with a per-level bind group (texture = previous level's view, sampler, and a `BlurParamsUniform` whose `texel_size` is `1.0 / (level width, level height)` written via `queue.write_buffer` before the pass — one bind group + one small uniform buffer per level, created once in `AcrylicState::new`/`ensure_size` alongside the textures).

Finally, update the `acrylic_uniform_buf` (tint + viewport_size + strength) once per dirty frame via `queue.write_buffer`, and bind `self.acrylic.acrylic_bind_group` before `main_render_pass`'s existing `pass.set_pipeline(&self.bg_pipeline)` calls (both grid and overlay layer iterations, since the pipeline layout now requires group 0 to be bound — the shader only reads it when `acrylic_mix > 0.0`, which grid-layer vertices never set, so this is a no-op for the grid layer beyond the bind call itself).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu counts_each_independent_overlay_flag --lib`
Expected: PASS.

Run: `cargo build -p nexterm-client-gpu`
Expected: builds cleanly.

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/renderer/render_frame.rs nexterm-client-gpu/src/renderer/mod.rs
git commit -m "feat(client): wire acrylic capture and blur chain into render_frame (P2b)"
```

---

### Task 8: Panel-fill sampling — `draw_overlay_panel` call sites + `tooltip.rs`

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/overlay/util.rs:76-127` (`draw_overlay_panel` signature)
- Modify: `nexterm-client-gpu/src/renderer/overlay/dialog.rs:44-47, 368-371, 569-572` (3 call sites)
- Modify: `nexterm-client-gpu/src/renderer/overlay/picker.rs:43-46, 140-143, 278-281, 426-429` (4 call sites)
- Modify: `nexterm-client-gpu/src/renderer/overlay/settings/mod.rs:170-173` (1 call site)
- Modify: `nexterm-client-gpu/src/renderer/overlay/widgets/tooltip.rs:67-119` (inlined chrome — does **not** call `draw_overlay_panel`, needs its own matching edit)

**Interfaces:**
- Consumes: `BgVertex.acrylic_mix` (Task 4).
- Produces: `draw_overlay_panel`'s new trailing parameter `acrylic_mix: f32` — every call site (all 8, across dialog/picker/settings/tooltip) must pass it explicitly; there is no default, so a missed call site is a compile error, not a silent gap.

- [ ] **Step 1: Write the failing test**

`draw_overlay_panel` is a vertex-emitting function with no return value to assert on directly; test its observable contract — the emitted background fill vertex carries the requested `acrylic_mix` — by inspecting the pushed `BgVertex` buffer:

```rust
#[cfg(test)]
mod acrylic_mix_tests {
    use super::*;

    #[test]
    fn fill_vertices_carry_the_requested_acrylic_mix() {
        let tokens = nexterm_config::DesignTokens::default();
        let mut bg_verts = Vec::new();
        let mut bg_idx = Vec::new();
        draw_overlay_panel(10.0, 10.0, 100.0, 50.0, &tokens, 128.0, 6.0, 800.0, 600.0, 0.75, &mut bg_verts, &mut bg_idx);
        // The panel background fill is the *last* 4 vertices pushed (shadow,
        // then border, then fill — see draw_overlay_panel's own comments).
        let fill_verts = &bg_verts[bg_verts.len() - 4..];
        assert!(fill_verts.iter().all(|v| (v.acrylic_mix - 0.75).abs() < f32::EPSILON));
        // The shadow and border ring stay opaque regardless of the panel's
        // acrylic_mix — only the fill itself is translucent acrylic.
        let non_fill_verts = &bg_verts[..bg_verts.len() - 4];
        assert!(non_fill_verts.iter().all(|v| v.acrylic_mix == 0.0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-client-gpu fill_vertices_carry_the_requested_acrylic_mix --lib`
Expected: FAIL — `draw_overlay_panel` takes 11 arguments, test passes 12 (compile error), confirming the signature must change first.

- [ ] **Step 3: Write minimal implementation**

`overlay/util.rs`:

```rust
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_overlay_panel(
    px: f32,
    py: f32,
    pw: f32,
    ph: f32,
    tokens: &nexterm_config::DesignTokens,
    elevation: f32,
    radius: f32,
    sw: f32,
    sh: f32,
    acrylic_mix: f32,
    bg_verts: &mut Vec<crate::glyph_atlas::BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    use crate::vertex_util::{add_px_rounded_rect_sdf, add_px_soft_shadow_sdf};

    // 1. Soft drop shadow — always opaque-black, never acrylic.
    let shadow = shadow_params(elevation);
    add_px_soft_shadow_sdf(
        px + shadow.offset, py + shadow.offset, pw, ph, radius,
        [0.0, 0.0, 0.0, shadow.alpha], shadow.softness, sw, sh, bg_verts, bg_idx,
    );

    // 2. Border ring — always opaque token color, never acrylic (the ring
    //    reads as a hairline edge; blurring it would just soften a border
    //    that is already anti-aliased).
    let bd = tokens.border_default;
    let border_color = [bd[0], bd[1], bd[2], 0.18];
    add_px_rounded_rect_sdf(px - 1.0, py - 1.0, pw + 2.0, ph + 2.0, radius + 1.0, border_color, sw, sh, bg_verts, bg_idx);

    // 3. Panel background — the only part that samples acrylic, via the
    //    trailing acrylic_mix vertex field (UI/UX v3 P2b).
    let bg = tokens.surface_2;
    add_px_rounded_rect_sdf_with_acrylic(px, py, pw, ph, radius, bg, sw, sh, acrylic_mix, bg_verts, bg_idx);
}
```

Add `add_px_rounded_rect_sdf_with_acrylic` to `vertex_util.rs` alongside the existing `add_px_rounded_rect_sdf` (same body, but threading a non-zero `acrylic_mix` into `push_rect_verts_with_sdf` instead of the hardcoded `0.0` every other caller uses) — extend `push_rect_verts_with_sdf`'s parameter list with `acrylic_mix: f32` and update its 3 existing callers (`add_rect_verts`, `add_px_rounded_rect_sdf`, `add_px_soft_shadow_sdf`, `add_px_stroke_sdf`) to pass `0.0`.

Update each of the 8 call sites to pass the panel's `acrylic_mix`, computed as `if config.window.in_app_blur_enabled { config.window.in_app_blur_strength } else { 0.0 }` (threaded down from wherever each overlay widget already receives `tokens`/`sw`/`sh` — e.g. `dialog.rs:44-47` becomes):

```rust
        let elevation = nexterm_config::ElevationScale::default().dialog;
        let acrylic_mix = if config.window.in_app_blur_enabled { config.window.in_app_blur_strength } else { 0.0 };
        draw_overlay_panel(
            px, py, pw, ph, tokens, elevation, 6.0, sw, sh, acrylic_mix, bg_verts, bg_idx,
        );
```

(apply the same two-line pattern — `acrylic_mix` computed, then passed as the 10th positional argument — to the other 6 `draw_overlay_panel` call sites listed in Files above).

`tooltip.rs` — since it inlines the shadow/border/fill sequence rather than calling `draw_overlay_panel`, apply the equivalent change directly: extend its final `add_px_rounded_rect_sdf` call (the one filling with `theme.tokens.surface_3`) to `add_px_rounded_rect_sdf_with_acrylic`, passing `acrylic_mix` computed from `theme`'s config access the same way.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu fill_vertices_carry_the_requested_acrylic_mix --lib`
Expected: PASS.

Run: `cargo build -p nexterm-client-gpu`
Expected: builds cleanly (confirms all 8 call sites were updated — a missed one is a compile error).

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/renderer/overlay/util.rs nexterm-client-gpu/src/renderer/overlay/dialog.rs nexterm-client-gpu/src/renderer/overlay/picker.rs nexterm-client-gpu/src/renderer/overlay/settings/mod.rs nexterm-client-gpu/src/renderer/overlay/widgets/tooltip.rs nexterm-client-gpu/src/vertex_util.rs
git commit -m "feat(client): sample acrylic blur in all three overlay elevation tiers (P2b)"
```

---

### Task 9: Contrast tests across all 9 schemes at both strength extremes

**Files:**
- Modify: wherever the PR #71 danger-button contrast tests live (the same test module the design spec's Testing section references — locate via `grep -rn "MIN_TEXT_CONTRAST" nexterm-client-gpu/src` and add alongside the existing per-scheme contrast test loop)
- Test: same file

**Interfaces:**
- Consumes: `tokens.surface_2` (the panel fill color acrylic mixes toward), the project's existing contrast-ratio helper (whatever function the PR #71 tests call — reuse it, do not reimplement contrast math).

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn panel_label_over_acrylic_tint_clears_contrast_floor_on_every_scheme() {
        for scheme in nexterm_config::ColorScheme::all_builtin() {
            let tokens = scheme.design_tokens();
            for strength in [0.0_f32, 1.0_f32] {
                // The CPU-computable worst case: the tint the fill mixes
                // toward, ignoring the (GPU-only, unknowable here) blurred
                // sample itself — same scope limitation the design spec
                // names explicitly under "Risks".
                let tint = mix_rgb(tokens.surface_2, tokens.surface_0, strength);
                let label = tokens.on_surface_text(tint);
                let ratio = contrast_ratio(label, tint);
                assert!(
                    ratio >= MIN_TEXT_CONTRAST,
                    "{:?} strength={strength}: ratio {ratio:.2} < {MIN_TEXT_CONTRAST}",
                    scheme,
                );
            }
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-client-gpu panel_label_over_acrylic_tint --lib`
Expected: FAIL — `mix_rgb` not found (or whichever helper doesn't exist yet).

- [ ] **Step 3: Write minimal implementation**

Add the pure blend helper next to `danger_fill`/`caution_fill` (same file, same idiom):

```rust
/// Linear RGB mix, `t=0` -> `a`, `t=1` -> `b`. Mirrors the blend shape
/// `danger_fill`/`caution_fill` already use for strength-based blending
/// (G11 follow-up), reused here for the acrylic tint (UI/UX v3 P2b).
fn mix_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}
```

If `tokens.on_surface_text(...)` does not already exist as a helper that picks a readable label color for an arbitrary background, reuse whatever the existing dialog-label contrast tests (PR #71) call instead — do not invent a new contrast-fixup function in this task; this task only needs to prove the *existing* readability mechanism still holds once the fill's effective color can slide toward `surface_0`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu panel_label_over_acrylic_tint --lib`
Expected: PASS across all 9 schemes × 2 strengths (18 assertions in one test).

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add -A -- '*.rs'
git commit -m "test(client): assert acrylic tint keeps panel labels readable on all 9 schemes (P2b)"
```

---

### Task 10: Full workspace verification

**Files:** none (verification only)

- [ ] Run: `cargo fmt --check`
- [ ] Run: `cargo clippy -- -D warnings`
- [ ] Run: `cargo test --workspace`
- [ ] Run: `cargo test -p nexterm-config doc_matches_schema` (confirms Task 1's `CONFIGURATION.md` edits keep the doc/schema test green)
- [ ] Run: `cargo test -p nexterm-i18n` (confirms no locale drift was introduced — PR 1 adds no new locale keys, so this should be an unaffected baseline pass)
- [ ] If `Cargo.lock` changed at any point in PR 1 (it may, if `bytemuck` derives or similar pull a new transitive dependency), run `bash scripts/regenerate-flatpak-sources.sh` and commit the resulting `pkg/flatpak/cargo-sources.json` diff.

---

### Task 11: Update the plan doc and on-device verification backlog

**Files:**
- Modify: `docs/plans/ui-ux-modernization-v3.md:456` (check off "P2b in-app acrylic")
- Modify: `docs/plans/ui-ux-modernization-v3.md`'s "On-device verification backlog" section (append the P2b-specific entries below)

- [ ] Check off line 456: `- [x] P2b in-app acrylic (offscreen + Kawase blur) — #<PR number>`.
- [ ] Append to the on-device verification backlog (matching the existing bullet style for #63/#64/#69/#70/#71):

```markdown
  - #<PR number> — P2b in-app acrylic. Not measured on real hardware:
    perceived blur quality and the Kawase tap radius; the carried-over
    P2a risk that `draw_focus_ring`'s stroke-only interior (PR #64) may
    double-blend against a now-translucent panel fill; frame-time cost of
    the extra offscreen pass + 4-pass blur chain, particularly on
    integrated GPUs; recapture correctness across a real multi-monitor /
    DPI-change transition; whether the fixed-intensity procedural noise
    reads as grain or banding on various panel colors. Ships with
    `in_app_blur_enabled = false` by default specifically because none of
    this is measured yet.
```

- [ ] Commit:

```bash
git add docs/plans/ui-ux-modernization-v3.md
git commit -m "docs(plan): mark P2b in-app acrylic shipped, log its on-device backlog"
```

## PR 2 — Settings panel UI (exposes the config PR 1 shipped)

---

### Task 12: `settings_window.rs` — toggle + slider, and `SettingsPanel` fields

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/overlay/widgets/settings_window.rs` (row constants `:24-53`, label arms `:74-87`, `WidgetKind` arms `:95-150`, `window_widget_descs` reads `WINDOW_ROW_COUNT` automatically `:55, :162-173`, `apply_window_action` `:249-293`)
- Modify: `nexterm-client-gpu/src/settings/mod.rs` (`SettingsPanel` fields `:101, :232`, init `:356, :402`)
- Modify: `nexterm-client-gpu/src/settings/window_extra.rs` (toggle method, mirroring `toggle_cursor_blink` `:59-62`)
- Modify: `nexterm-client-gpu/src/settings/window.rs` (slider setter, mirroring `set_opacity_value` `:48-52`)
- Modify: `nexterm-client-gpu/src/settings/save.rs` (TOML write-back, mirroring `:52-53` / `:76-77`)

**Interfaces:**
- Consumes: `config.window.in_app_blur_enabled` / `in_app_blur_strength` (Task 1) to initialize `SettingsPanel`.
- Produces: nothing further downstream — this is the plan's last functional task.

- [ ] **Step 1: Write the failing test**

Settings-panel fields in this codebase are verified by the existing widget-desc/save round trip pattern rather than a bespoke per-field test (confirmed: no dedicated test exists for `OPACITY`/`CURSOR_BLINK` individually). Follow that precedent — the test that matters here is the locale key-parity test (Task 13) plus a save-round-trip check:

```rust
    #[test]
    fn in_app_blur_settings_round_trip_through_save() {
        let mut sp = SettingsPanel::new(&Config::default());
        assert!(!sp.in_app_blur_enabled);
        sp.toggle_in_app_blur();
        assert!(sp.in_app_blur_enabled);
        sp.set_in_app_blur_strength_value(0.3);
        assert!((sp.in_app_blur_strength - 0.3).abs() < 0.05); // slider step-rounded, same tolerance as opacity's test
        let toml_str = sp.apply_to_toml_string("".to_string());
        assert!(toml_str.contains("in_app_blur_enabled = true"));
    }
```

(place this next to whichever existing test module covers `SettingsPanel`/`save.rs` — if none exists yet for this file, add `#[cfg(test)] mod tests` following the sibling files' convention.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-client-gpu in_app_blur_settings_round_trip --lib`
Expected: FAIL — `SettingsPanel` has no field `in_app_blur_enabled`.

- [ ] **Step 3: Write minimal implementation**

`settings/mod.rs` — add fields next to `cursor_blink_enabled` (`:232`):

```rust
    pub in_app_blur_enabled: bool,
    pub in_app_blur_strength: f32,
```

Initialize in `SettingsPanel::new` next to the existing `cursor_blink_enabled: config.cursor.blink_enabled` (`:402`):

```rust
            in_app_blur_enabled: config.window.in_app_blur_enabled,
            in_app_blur_strength: config.window.in_app_blur_strength,
```

`settings/window_extra.rs` — add the toggle, mirroring `toggle_cursor_blink`:

```rust
    pub fn toggle_in_app_blur(&mut self) {
        self.in_app_blur_enabled = !self.in_app_blur_enabled;
        self.dirty = true;
    }
```

`settings/window.rs` — add the slider setter, mirroring `set_opacity_value`'s clamp-then-quantize shape:

```rust
    pub fn set_in_app_blur_strength_value(&mut self, v: f64) {
        let raw = (v as f32).clamp(0.0, 1.0);
        self.in_app_blur_strength = (raw * 20.0).round() / 20.0;
        self.dirty = true;
    }
```

`settings/save.rs` — write both back, mirroring `background_opacity`/`blink_enabled`:

```rust
        // [window].in_app_blur_enabled / in_app_blur_strength (P2b).
        doc["window"]["in_app_blur_enabled"] = toml_edit::value(self.in_app_blur_enabled);
        doc["window"]["in_app_blur_strength"] = toml_edit::value(self.in_app_blur_strength as f64);
```

`settings_window.rs` — append two row constants (`:24-53`):

```rust
    /// In-app blur enabled (toggle).
    pub const IN_APP_BLUR_ENABLED: u16 = 14;
    /// In-app blur strength (slider).
    pub const IN_APP_BLUR_STRENGTH: u16 = 15;
```

bump `WINDOW_ROW_COUNT` from `14` to `16` (`:56`), add label arms (`:74-87`):

```rust
        row::IN_APP_BLUR_ENABLED => fl!("settings-window-in-app-blur-label"),
        row::IN_APP_BLUR_STRENGTH => fl!("settings-window-in-app-blur-strength-label"),
```

add `WidgetKind` arms (after `:150`, mirroring `OPACITY`/`CURSOR_BLINK`):

```rust
        row::IN_APP_BLUR_ENABLED => WidgetKind::Toggle {
            on: sp.in_app_blur_enabled,
        },
        row::IN_APP_BLUR_STRENGTH => WidgetKind::Slider {
            value: sp.in_app_blur_strength,
            min: 0.0,
            max: 1.0,
            step: 0.05,
            display: format!("{:.0}%", sp.in_app_blur_strength * 100.0),
        },
```

and `apply_window_action` arms (`:249-293`, mirroring `CURSOR_BLINK`'s `Activate` arm and `OPACITY`'s `SetValue` arm):

```rust
            row::IN_APP_BLUR_ENABLED => sp.toggle_in_app_blur(),
```
```rust
            row::IN_APP_BLUR_STRENGTH => sp.set_in_app_blur_strength_value(v),
```

(placed in the same `match` arms as `CURSOR_BLINK`'s `WidgetAction::Activate` handler and `OPACITY`'s `WidgetAction::SetValue(v)` handler respectively — `window_widget_descs` needs no change, since it already loops `0..WINDOW_ROW_COUNT` generically).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-client-gpu in_app_blur_settings_round_trip --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-client-gpu
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/renderer/overlay/widgets/settings_window.rs nexterm-client-gpu/src/settings/mod.rs nexterm-client-gpu/src/settings/window_extra.rs nexterm-client-gpu/src/settings/window.rs nexterm-client-gpu/src/settings/save.rs
git commit -m "feat(client): expose in-app blur toggle/strength on the Window settings tab (P2b)"
```

---

### Task 13: Locale strings — all 8 languages

**Files:**
- Modify: `nexterm-i18n/locales/en.json`, `ja.json`, `de.json`, `es.json`, `fr.json`, `it.json`, `ko.json`, `zh-CN.json`

**Interfaces:**
- Consumes: the two `fl!(...)` keys referenced in Task 12 (`settings-window-in-app-blur-label`, `settings-window-in-app-blur-strength-label`).

- [ ] **Step 1: Write the failing test**

The existing `test_all_locales_have_same_keys_as_en` (`nexterm-i18n/src/lib.rs:262-289`) already fails the moment `en.json` gains a key the other 7 files don't have — no new test needed, run the existing one after Step 3a (editing only `en.json`) to observe the intended failure before completing the other 7 files.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexterm-i18n test_all_locales_have_same_keys_as_en`
Expected: FAIL with `locale 'de' key set diverges from en: missing=["settings-window-in-app-blur-label", "settings-window-in-app-blur-strength-label"], extra=[]` (and identically for the other 6) after editing only `en.json`.

- [ ] **Step 3: Write minimal implementation**

Add both keys to all 8 files, next to the existing `settings-window-cursor-blink-label` entry (English shown; translate the value string per locale, keep the key identical):

`en.json`:
```json
  "settings-window-in-app-blur-label": "In-app blur:",
  "settings-window-in-app-blur-strength-label": "Blur strength:",
```

`ja.json`:
```json
  "settings-window-in-app-blur-label": "アプリ内ブラー:",
  "settings-window-in-app-blur-strength-label": "ブラー強度:",
```

Repeat with an appropriately translated value (same key names) in `de.json`, `es.json`, `fr.json`, `it.json`, `ko.json`, `zh-CN.json`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nexterm-i18n test_all_locales_have_same_keys_as_en`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --check -p nexterm-i18n
git add nexterm-i18n/locales/*.json
git commit -m "i18n: add in-app blur settings strings to all 8 locales (P2b)"
```

---

### Task 14: Final workspace verification and plan closure

- [ ] Run: `cargo fmt --check`
- [ ] Run: `cargo clippy -- -D warnings`
- [ ] Run: `cargo test --workspace`
- [ ] Update `docs/CONFIGURATION.md` if the settings-panel work surfaced any doc drift (it should not — Task 1 already documented both keys; this is a safety check, not new content).
- [ ] Open PR 2 against `master`, referencing PR 1 and the design spec.

## Self-Review Notes (author's pass, not a subagent dispatch)

- **Spec coverage**: every "In scope" bullet in the design spec maps to a task above (capture: Task 2/6/7; blur chain: Task 3/5/6/7; sampling across all 3 tiers: Task 8, including the tooltip.rs correction the research turned up; config: Task 1; settings UI: Task 12/13; contrast tests: Task 9; on-device backlog entries: Task 11). The spec's "Out of scope" items (`window.backdrop`, OS-native backdrop APIs) have no corresponding task — correct, they're P2c.
- **Placeholder scan**: no `TBD`/`TODO`/"implement later" strings anywhere in the plan; every code block is complete, real code. The one place that could have become a placeholder — `blur_bind_group_layout` in Task 6 — is written out in full and cross-referenced to `acrylic_bind_group_layout`'s identical shape rather than deferred.
- **Type consistency**: `acrylic_mix: f32` is named identically from Task 4 (`BgVertex` field) through Task 8 (`draw_overlay_panel` parameter and all 8 call sites) through Task 9 (test only reads the field, same name). `in_app_blur_enabled` / `in_app_blur_strength` are named identically from Task 1 (`WindowConfig`) through Task 12 (`SettingsPanel`, same field names — deliberately, unlike `background_opacity` which renames to `opacity` on `SettingsPanel`; keeping the P2b names identical end-to-end was a judgment call to reduce the number of names a future reader has to track, and is called out here since it's an established codebase pattern this plan does *not* follow).
- **Scope check**: PR 1 (Tasks 1-11) is large relative to prior single PRs in this project's history (#63, #69-71 were each narrower), but it is one cohesive, independently-shippable deliverable — the design spec itself frames the engine as a single vertical slice, and splitting it further (e.g., landing the blur chain before any panel samples it) would leave intermediate PRs with no observable behavior to review against. PR 2 (Tasks 12-13) is deliberately small and low-risk, matching the #64-style "focused follow-up" precedent.
