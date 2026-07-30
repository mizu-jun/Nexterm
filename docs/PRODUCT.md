# Nexterm Product Requirements

- Status: living document, reviewed at each minor/major release (alongside `CHANGELOG.md`)
- Last reviewed: 2026-07-31 (v1.16.0)
- Owner: @mizu-jun

This document defines **what Nexterm is and why** — vision, target users, product
requirements, and explicit non-goals. It is intentionally coarse-grained: it does not
track in-flight work (that lives in [docs/plans/](plans/)) and it does not describe how
the system is built (that lives in [ARCHITECTURE.md](ARCHITECTURE.md)).

## Vision

A daemonless terminal multiplexer in Rust that combines the session resilience of
tmux, the rendering quality of a modern GPU terminal, and built-in remote
connectivity — in a single binary that works the same on Linux, macOS, and Windows.

"Daemonless" means: the server is an internal tokio task inside the `nexterm` binary
(or an optional standalone process); sessions survive client disconnects without the
user ever managing a daemon.

## Target users

1. **Terminal power users** who want tmux-style multiplexing (BSP splits, attach/detach,
   session sharing) without giving up GPU rendering, ligatures, images, and CJK/IME support.
2. **Remote operators** who live in SSH / SFTP / serial consoles and want host management,
   port forwarding, ProxyJump, and file transfer built into the terminal instead of a
   toolbox of separate utilities.
3. **Windows users seeking a tmux equivalent** — first-class ConPTY + Named Pipe support,
   MSI/winget/Scoop distribution, and feature parity with the Unix build.
4. **Teams with security / compliance requirements** who need supply-chain transparency
   (SBOM, SLSA provenance, signed updates, threat model) from their tooling.
5. **Non-English users** — the UI ships in 8 languages (en/ja/zh-CN/ko/de/fr/es/it).

## Differentiation

Versus WezTerm / kitty / Alacritty / Ghostty / Windows Terminal, Nexterm's sustained
differentiators (do not regress these):

| Differentiator | Notes |
|---|---|
| Sandboxed WASM plugin runtime | wasmi with fuel + memory caps; versioned stable ABI ([plugin-api.md](plugin-api.md)) |
| Embedded web terminal | axum + xterm.js with token / OAuth / TOTP auth and optional TLS; no other OSS terminal ships this by default |
| Full UI internationalization | 8 locales; among OSS terminals only Windows Terminal is comparable |
| Supply-chain posture | minisign-verified updates + SLSA provenance + CycloneDX SBOM + STRIDE threat model |
| In-app settings GUI | 7-category panel writing back to TOML; rare among OSS terminals |
| TOML + Lua two-tier configuration | static safety plus dynamic scripting, both hot-reloaded |
| Integrated SSH stack | agent auth, known-hosts, ProxyJump, SOCKS5, X11 forwarding, SFTP GUI, serial ports |
| Consent-based security UX | per-capability consent prompts for OSC 52 clipboard, notifications, URL opens |
| Accessibility | full AccessKit tree (NVDA / VoiceOver / Orca) for tabs, panes, dialogs, and the grid |

Competitive tracking (feature matrix, gap analysis) is a planning activity — see
[plans/gap-roadmap-2026h2.md](plans/gap-roadmap-2026h2.md) for the current roadmap.

## Product requirements by area

Requirements are stated at the capability level. Shipped capabilities are the current
contract; planned ones link to their plan file.

| Area | Requirement | Reference |
|---|---|---|
| Terminal emulation | VT100/ANSI with xterm extensions; Sixel, Kitty graphics, iTerm2 inline images; kitty keyboard (CSI u) and Text Sizing; OSC 8/52/133; underline extensions | `nexterm-vt`, [src/features/terminal.md](src/features/terminal.md) |
| Multiplexing | Sessions survive client disconnect; BSP splits to arbitrary depth; zoom, swap, break/join, drag-resize; tabs with tearing; floating panes; session snapshots with versioned auto-migration | [ARCHITECTURE.md](ARCHITECTURE.md), ADR-0005, ADR-0007 |
| Command blocks | OSC 133-based prompt → command → output → exit-code blocks, navigable and persistent (Warp-style) | [plans/blocks-implementation.md](plans/blocks-implementation.md) |
| Remote connectivity | Built-in SSH (agent, known-hosts, port forwarding, ProxyJump, SOCKS5, X11), SFTP with progress UI, serial ports, web terminal with authenticated access | [src/features/ssh.md](src/features/ssh.md), [src/features/web.md](src/features/web.md) |
| Extensibility | WASM plugins (sandboxed, versioned ABI, runtime load/unload/reload) and Lua (hooks, macros, status bar, key bindings) | [plugin-api.md](plugin-api.md), ADR-0004 |
| Configuration | TOML for static config + Lua for dynamic overrides, hot-reloaded; settings GUI writes back preserving comments | [CONFIGURATION.md](CONFIGURATION.md) |
| Internationalization | Every user-facing string localized in all 8 locales via `nexterm-i18n` | `nexterm-i18n/locales/` |
| Accessibility | Screen-reader support via AccessKit; contrast ≥ 4.5:1; full keyboard operation; IME composition | CLAUDE.md UI/UX guidelines |
| Security | Sandboxed extension surfaces; consent UI for sensitive escape sequences; UID-validated IPC; zeroized secrets; OS keychain integration | [THREAT_MODEL.md](THREAT_MODEL.md), [../SECURITY.md](../SECURITY.md) |
| Distribution | Single binary; Linux (tarball, AppImage, Flatpak), macOS (Homebrew), Windows (MSI, winget, Scoop); signed auto-update | Release workflow |
| Observability | PTY recording (raw + asciicast v2), access logs for the web terminal, `NEXTERM_LOG` tracing | `nexterm-ctl record` |

## Non-goals

Explicit decisions **not** to build (rationale recorded where the decision was made):

- **Mosh support** — XL effort for low demand ([plans/gap-roadmap-2026h2.md](plans/gap-roadmap-2026h2.md)).
- **Core-embedded AI assistant** — AI features belong in the plugin layer on top of the
  read API, keeping the core AI-free (same roadmap; the WASM sandbox + consent UI is
  the differentiator there).
- **A DOM/web-based desktop UI** — the GUI is rendered natively with wgpu + cosmic-text;
  no HTML/CSS/React layer (CLAUDE.md UI/UX guidelines). The web *terminal* feature is a
  remote-access endpoint, not the desktop UI.
- **A mandatory resident daemon** — the daemonless single-binary model is the identity
  of the product (see Vision).

## Quality attributes

- **Compatibility**: IPC protocol and snapshot schema are versioned (`PROTOCOL_VERSION`,
  `SNAPSHOT_VERSION`) with monotonic bumps and auto-migration for older snapshots
  (ADR-0002, ADR-0007). Plugin ABI changes bump the plugin API version ([plugin-api.md](plugin-api.md)).
- **Performance**: GPU-rendered with damage tracking and diff-based grid updates;
  regressions are guarded by the benchmarks in [benchmarks.md](benchmarks.md).
- **Reliability**: no `unwrap()` in production code; poison-lock recovery; fuzzing
  (cargo-fuzz) and property tests on the VT parser; 3-OS CI matrix.
- **Security**: STRIDE threat model maintained in [THREAT_MODEL.md](THREAT_MODEL.md);
  cargo-deny + SBOM + provenance in CI; periodic audit rounds ([plans/](plans/)).

## Related documents

| Document | Answers |
|---|---|
| This file | What to build and why; non-goals |
| [ARCHITECTURE.md](ARCHITECTURE.md) | How the system is built |
| [PROTOCOL.md](PROTOCOL.md), [plugin-api.md](plugin-api.md) | Interface contracts |
| [adr/](adr/README.md) | Why individual design decisions were made |
| [plans/](plans/) | What is being worked on now (phased work plans) |
| [CHANGELOG.md](../CHANGELOG.md) | What shipped when |
