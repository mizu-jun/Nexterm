# nexterm

A terminal multiplexer written in Rust, inspired by tmux/zellij, featuring GPU rendering via wgpu and a Lua configuration system.

> **日本語ドキュメント:** [README.ja.md](README.ja.md)

[![CI](https://github.com/mizu-jun/nexterm/actions/workflows/ci.yml/badge.svg)](https://github.com/mizu-jun/nexterm/actions/workflows/ci.yml)
[![Coverage](https://github.com/mizu-jun/nexterm/actions/workflows/coverage.yml/badge.svg)](https://github.com/mizu-jun/nexterm/actions/workflows/coverage.yml)

For release history, see [CHANGELOG.md](CHANGELOG.md) and the [GitHub Releases](https://github.com/mizu-jun/Nexterm/releases) page.
For upgrade notes between versions with breaking changes, see [docs/MIGRATION.md](docs/MIGRATION.md).

---

## Highlights

- **Daemonless** — Server holds PTYs; sessions survive client disconnects, tmux-style attach/share between multiple clients.
- **GPU rendering** — wgpu + cosmic-text glyph atlas; alternate screen buffer, CJK width, ligatures, font fallback chain.
- **SSH built-in** — russh-based client with host registry, agent auth, known-hosts verification, port forwarding (-L/-R), ProxyJump, SOCKS5, X11 forwarding, OS keychain integration; SFTP upload/download with progress.
- **BSP pane layout** — Arbitrary-depth splits, pane swap, zoom, break/join, drag-resize.
- **Command Blocks (Warp-style)** — OSC 133 prompt markers fold each prompt → command → output → exit-code into a navigable block; named blocks persist; works with WezTerm / kitty / Ghostty integration snippets.
- **Image protocols** — Sixel, Kitty, iTerm2 inline images.
- **Vim copy mode + Vi mode** — Selection, search, motions; status badge for current mode.
- **Lua + TOML config** — Hot-reload, status bar widgets, event hooks, key bindings, macros.
- **In-app settings GUI** — `Ctrl+,` opens a 7-category panel that writes back to `nexterm.toml` via `toml_edit`.
- **WASM plugin runtime** — wasmi sandbox (fuel + memory caps), stable Plugin API v2, runtime load/unload/reload.
- **Screen reader support** — Full AccessKit tree (NVDA / VoiceOver / Orca) for tabs, panes, dialogs, terminal grid.
- **Web terminal** — Embedded axum WebSocket server + xterm.js (token / OAuth / TOTP auth, optional TLS).
- **Recording** — Raw PTY logs and asciicast v2 via `nexterm-ctl record`.
- **Cross-platform** — Linux / macOS / Windows (ConPTY + Named Pipe), 8-language UI.
- **Distribution** — Homebrew, Scoop, winget, MSI, Flatpak, tarball.
- **Security & supply chain** — Sandboxed Lua/WASM, sensitive-op consent prompts, cargo-deny in CI, CycloneDX SBOM, SLSA build provenance, minisign update verification, STRIDE threat model.

Full feature inventory: [docs/src/features/](docs/src/features/).

---

## Quick Start

```sh
# macOS
brew install mizu-jun/nexterm/nexterm && nexterm

# Linux (tarball)
tar xzf nexterm-vX.Y.Z-linux-x86_64.tar.gz && ./install.sh && nexterm

# Windows
# Install the MSI from the Releases page, then run:
nexterm.exe
```

`nexterm` is a single binary — the server runs as an internal tokio task, so you do not start anything else manually. Detailed install / first-run / troubleshooting steps live in the user guide:

- [Installation](docs/src/install.md) · [Quick Start](docs/src/quickstart.md) · [Windows Quick Start](docs/src/windows.md) · [Troubleshooting](docs/src/troubleshooting.md)

Windows 10 **1809+** is required (ConPTY); the GPU client requires a DirectX 11–capable adapter.

---

## Build from source

```bash
# Prerequisites: Rust 1.85+ (workspace edition = "2024")
# Linux: sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev

cargo build --release
cargo test
cargo clippy -- -D warnings        # required for PR merge
cargo fmt --check
```

For the full development workflow, coding conventions, and PR guidelines, see [CONTRIBUTING.md](CONTRIBUTING.md) and [CLAUDE.md](CLAUDE.md).

---

## Crate layout

```
nexterm/
├── nexterm-proto         # IPC message types (postcard)
├── nexterm-vt            # VT100 parser, virtual screen, image decode
├── nexterm-server        # PTY server (IPC + session management)
├── nexterm-config        # Config loader (TOML + Lua) + status bar evaluator
├── nexterm-client-core   # Shared IPC connection layer
├── nexterm-client-tui    # TUI client (ratatui + crossterm)
├── nexterm-client-gpu    # GPU client (wgpu + winit + cosmic-text); built bin is `nexterm`
├── nexterm-ctl           # Session / plugin management CLI
├── nexterm-i18n          # Localization (8 languages)
├── nexterm-ssh           # SSH client (russh)
└── nexterm-plugin        # WASM plugin host runtime (wasmi, API v2)
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for crate dependency graph, process layout, BSP layout engine, IPC framing, render pipeline, and threading model.

---

## Key bindings

Common shortcuts:

| Key | Action |
|-----|--------|
| `Ctrl+,` | Open settings panel |
| `Ctrl+Shift+P` | Command palette |
| `Ctrl+F` | Scrollback search |
| `Ctrl+[` | Vim copy mode |
| `Ctrl+Shift+C` / `V` | Copy / paste |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Font size up / down / reset |
| `Ctrl+Shift+H` | SSH Host Manager |
| `Ctrl+Shift+M` | Lua Macro Picker |
| `Ctrl+B Z` | Zoom focused pane |
| `Ctrl+Shift+ArrowUp/Down` | Jump between command blocks |

Full reference: [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md).

---

## nexterm-ctl

```bash
nexterm-ctl list                              # list sessions
nexterm-ctl new work                          # create session
nexterm-ctl attach work                       # show attach command
nexterm-ctl kill work                         # kill session

nexterm-ctl record start work output.log     # raw PTY recording
nexterm-ctl record start-cast work cast.cast # asciicast v2 recording

nexterm-ctl theme import ~/.iTerm2/colorscheme.itermcolors
nexterm-ctl plugin {list,load,unload,reload} # WASM plugin control
```

`NEXTERM_LANG=ja nexterm-ctl list` forces the UI locale. Supported: `en`, `fr`, `de`, `es`, `it`, `zh-CN`, `ja`, `ko`.

---

## Configuration

Config files are searched at:

| OS | Path |
|----|------|
| Linux / macOS | `~/.config/nexterm/config.toml` |
| Windows | `%APPDATA%\nexterm\config.toml` |

```toml
# Minimal example
scrollback_lines = 50000

[font]
family = "JetBrains Mono"
size = 14.0
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji"]

[colors]
scheme = "tokyonight"

[shell]
program = "/usr/bin/fish"

[window]
background_opacity = 0.95
```

A Lua override file (`nexterm.lua`) at the same location lets you change values at runtime; both are hot-reloaded on save.

Full reference:
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) — every TOML/Lua key
- [docs/src/config/snippets.md](docs/src/config/snippets.md) — copy-paste recipes
- [docs/src/config/lua-recipes.md](docs/src/config/lua-recipes.md) — Lua macros, hooks, status bar
- [docs/shell-integration.md](docs/shell-integration.md) — bash / zsh / fish snippets for OSC 133 command blocks

---

## Documentation map

| Document | Contents |
|----------|----------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, process model, render pipeline, IPC, BSP |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | IPC protocol spec (message types, framing, handshake) |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Full TOML / Lua configuration reference |
| [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) | Complete key binding reference |
| [docs/MIGRATION.md](docs/MIGRATION.md) | Upgrade notes between versions |
| [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | STRIDE threat model (9 trust boundaries) |
| [docs/SBOM.md](docs/SBOM.md) | Supply chain and SBOM policy |
| [docs/TESTING_STRATEGY.md](docs/TESTING_STRATEGY.md) | Test taxonomy and QA × ISO/IEC 25010 matrix |
| [docs/plugin-api.md](docs/plugin-api.md) | WASM Plugin API v2 |
| [docs/shell-integration.md](docs/shell-integration.md) | Command-block shell integration snippets |
| [docs/benchmarks.md](docs/benchmarks.md) | VT throughput / keystroke latency benchmarks |
| [docs/adr/](docs/adr/README.md) | Architecture Decision Records |
| [docs/src/](docs/src/README.md) | mdBook-rendered user guide (install / features / config / troubleshooting) |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Build instructions, coding conventions, PR guidelines |
| [SECURITY.md](SECURITY.md) | Security policy and reporting |

---

## License

MIT OR Apache-2.0
