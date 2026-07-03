# Competitive Gap Roadmap — 2026 H2 (v1.12+)

Status: approved 2026-07-03. Source: codebase inventory audit (v1.11.0) + competitor research
(WezTerm / kitty 0.47 / Alacritty 0.17 / Ghostty 1.3 / Windows Terminal 1.25 / Warp / iTerm2 3.7 / Zellij).

## Context

Of the 15 weaknesses identified in the v1.1.0 audit, 10 have shipped by v1.11.0
(OSC 133 + Command Blocks, kitty keyboard CSI u, kitty Text Sizing, iTerm2 inline images,
Vi mode, tab tearing, AccessKit, Quake mode, WSL profile detection, theme gallery).
Industry findings: kitty protocols are becoming the de facto standard (Windows Terminal 1.25
adopted CSI u); block-based UI is the differentiation axis (Nexterm already ships it);
AI integration is trending but the OSS consensus is separation of concerns — expose it via
the plugin layer instead of the core.

Non-goals: Mosh (XL effort, low demand), core-embedded AI assistant.

## Phase 1 — Debt payoff + low-cost/high-compat protocols (2–3 weeks)

| # | Task | Effort | Notes |
|---|------|:------:|-------|
| 1 | OSC 4 / 10 / 11 color query & set (with responses) | S–M | All competitors support it; breaks vim/neovim theme auto-detection today. Rate-limit and cap response length (see Risks). Scope note: VT-layer state + query replies only; client-side visual application of dynamic colors and theme→server default wiring both need new IPC plumbing and move to Phase 2 as one task (see #10b). |
| 2 | G4: Web Terminal auth integration tests (axum: OAuth / TOTP / session) | M | Last remaining HIGH item from audit round 2. |
| 3 | G6: cargo-fuzz target for the Lua sandbox | S | Alongside the 5 existing targets in `nexterm-vt/fuzz`. |
| 4 | G5: rustls-pemfile → rustls-pki-types migration | S–M | Clears the `deny.toml` ignore for the unmaintained crate. |
| 5 | OSC 22 mouse pointer shape | S | WezTerm / kitty parity. |

## Phase 2 — Industry-standard protocol catch-up (3 weeks)

| # | Task | Effort | Notes |
|---|------|:------:|-------|
| 6 | OSC 9;4 progress bar (tab / status bar display) | M | Synergy with Command Blocks. |
| 7 | OSC 99 desktop notifications | M | Reuse the existing consent-UI foundation. |
| 8 | Pixel-precision + momentum scrolling | M | kitty 0.46 parity. |
| 9 | kitty drag & drop protocol | M | Extend the existing file-D&D path. |
| 10b | Dynamic colors end-to-end: apply OSC 4/10/11 sets to rendering + report theme defaults in queries | M | Requires a `PaneColorsChanged` server→client message and a client→server theme-color report (PROTOCOL bump); split out of Phase 1 #1. |

## Phase 3 — Named workspaces (2–3 weeks)

| # | Task | Effort | Notes |
|---|------|:------:|-------|
| 10 | Named workspace save / list / restore / switch | L | Extends snapshot v4; palette `@workspace` integration; SNAPSHOT_VERSION bump expected with auto-migration. |

## Phase 4 — Plugin API v2 expansion (3–4 weeks)

| # | Task | Effort | Notes |
|---|------|:------:|-------|
| 11 | F3: read_pane / read_grid / scrollback plugin API | L | Permission model with consent UI; ADR required (new trust boundary). |
| 12 | F2: plugin host end-to-end integration tests | M | load → dispatch_output → unload cycle. |
| 13 | (Optional) AI integration enabled *as a plugin* on top of #11 | — | Keeps core AI-free; WASM sandbox + consent UI is the differentiator. |

## Phase 5 — Backlog (decide after Phase 3)

Docker/Podman exec profile generation (E2), Windows JumpList, first-run wizard,
font presets, high-contrast themes (H3), OKLCH / wide gamut.

## Risks

- **HIGH — OSC response surface**: query responses (OSC 4/10/11) create an echo-back attack
  surface; Windows Terminal reverted OSC 7 for security reasons. Cap response length,
  rate-limit replies, and run security review before merge.
- **MEDIUM — snapshot migration**: workspaces bump SNAPSHOT_VERSION; follow the existing
  v1→v4 auto-migration pattern in `load_snapshot()`.
- **MEDIUM — plugin read access**: exposing grid contents to plugins opens a new trust
  boundary; gate behind per-plugin consent and document in an ADR.
- **LOW — Phase 2 independence**: the four protocol tasks are independent and can be
  reordered or dropped individually.

## Release mapping

Phase 1 → v1.12, Phase 2 → v1.13, Phase 3 → v1.14, Phase 4 → v2.0 candidate.
Total: roughly 10–13 weeks.
