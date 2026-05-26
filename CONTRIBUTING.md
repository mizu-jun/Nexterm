# Contributing to nexterm

> **日本語:** [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md)

## Documentation Language

Nexterm is open source and distributed worldwide, so **English is the canonical language** for everything in this repository.

- Source code comments (`//`, `///`, doc-comments, `expect("...")` messages) must be written in English.
- Repository documentation (`README.md`, files under `docs/`, `CHANGELOG.md`, ADRs, etc.) is authored in English. A Japanese translation, when useful, sits alongside the English file as `*.ja.md` (e.g. `README.md` + `README.ja.md`). The English file is authoritative; the Japanese companion is a best-effort translation.
- User-facing strings in the application are managed by `nexterm-i18n`. Add new strings to all 8 locale files under `nexterm-i18n/locales/` and reference them through the `fl!` macro — do not hard-code natural language in the renderer.
- Commit messages, PR titles, and GitHub Release notes should be in English. Japanese supplements are welcome but should not be the only version.

When you add a new document, default to English. Create a `*.ja.md` companion only when Japanese readability matters for that specific document; treat it as a translation that may lag behind the English source.

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.80+ | Compilation |
| cargo | (bundled with Rust) | Build & test |

### OS-specific requirements

**Windows**
- Visual Studio Build Tools (C++ components)

**Linux**
```bash
sudo apt install libx11-dev libxkbcommon-dev libwayland-dev
```

**macOS**
- Xcode Command Line Tools (`xcode-select --install`)

---

## Build

```bash
# Build all crates
cargo build

# Release build
cargo build --release

# Specific crates
cargo build -p nexterm-server
cargo build -p nexterm-client-gpu
cargo build -p nexterm-ctl
cargo build -p nexterm-i18n
```

---

## Test

```bash
# Run all tests
cargo test

# Specific crate
cargo test -p nexterm-vt
cargo test -p nexterm-server
cargo test -p nexterm-ctl
cargo test -p nexterm-i18n

# Filter by test name
cargo test bsp_split
```

---

## Lint / Formatting

```bash
# Clippy (warnings as errors)
cargo clippy -- -D warnings

# Check formatting
cargo fmt --check

# Apply formatting
cargo fmt
```

PRs must pass `cargo clippy` and `cargo fmt --check`.

---

## Crate structure

```
nexterm/
├── nexterm-proto        # IPC message types and serialization (shared)
├── nexterm-vt           # VT100 parser, virtual screen, image decode
├── nexterm-server       # PTY server (IPC + session management)
├── nexterm-config       # Config loader (TOML + Lua) + StatusBarEvaluator
├── nexterm-client-tui   # TUI client (ratatui + crossterm)
├── nexterm-client-gpu   # GPU client (wgpu + winit)
├── nexterm-ctl          # Session management CLI (list / new / attach / kill)
└── nexterm-i18n         # Localization (8 languages, embedded JSON)
```

When adding features, consult the dependency graph in `docs/ARCHITECTURE.md` to decide which crate owns the change.
Changes to `nexterm-proto` affect all crates — handle with care.

---

## Coding conventions

### General

- Add doc comments to functions, types, and fields (English or Japanese)
- Variable and function names: English `snake_case` / `CamelCase`
- No `unwrap()` — use `?` or `expect("reason")`
- Propagate errors with `anyhow::Result`

### Async code

- Spawn tasks with `tokio::spawn`
- Blocking operations: use `tokio::task::spawn_blocking`
- `Arc<Mutex<T>>`: use tokio's `Mutex` for IPC, `std::sync::Mutex` for PTY read threads

### Localization

User-visible strings must use the `fl!` macro from `nexterm-i18n`:

```rust
use nexterm_i18n::fl;

println!("{}", fl!("ctl-no-sessions"));
println!("{}", fl!("ctl-session-created", name = name));
```

Add new keys to all 8 locale files under `nexterm-i18n/locales/`.

### Tests

- New features must include unit tests
- All tests must pass (`cargo test`) before submitting a PR

---

## Branch strategy

| Branch | Purpose |
|--------|---------|
| `main` | Stable. No direct push. |
| `feature/<name>` | New features |
| `fix/<name>` | Bug fixes |

---

## PR guidelines

1. Open a PR from a **feature branch** to `main`
2. Title format: `<type>: <description>` (e.g. `feat: add mouse click focus`)
3. `cargo test` and `cargo clippy` must pass
4. Update relevant docs in `docs/`

### Commit message format

```
<type>: <description>

<body (optional)>
```

| type | Purpose |
|------|---------|
| `feat` | New feature |
| `fix` | Bug fix |
| `refactor` | Refactoring |
| `test` | Add / update tests |
| `docs` | Documentation |
| `chore` | Build / dependency changes |
| `perf` | Performance improvement |

---

## Debugging

### Enable logging

```bash
# Server
NEXTERM_LOG=debug nexterm-server

# GPU client
NEXTERM_LOG=debug nexterm-client-gpu

# Windows
set NEXTERM_LOG=debug && nexterm-server.exe
```

Log levels: `error` / `warn` / `info` / `debug` / `trace`

### IPC message debugging

`NEXTERM_LOG=trace` prints all IPC messages (very verbose — development only).

---

## Key dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio` | 1 | Async runtime |
| `postcard` | 1 (use-std) | IPC serialization (Sprint 5-1 / ADR-0006 で bincode から移行) |
| `serde` | 1 | Serialization |
| `anyhow` | 1 | Error handling |
| `tracing` | 0.1 | Logging |
| `portable-pty` | 0.8 | PTY management |
| `vte` | 0.13 | VT sequence parser |
| `wgpu` | 22 | GPU rendering |
| `winit` | 0.30 | Window management |
| `cosmic-text` | 0.12 | Font rendering |
| `ratatui` | 0.29 | TUI rendering |
| `crossterm` | 0.28 | TUI I/O |
| `mlua` | 0.10 | Lua embedding |
| `toml` | 0.8 | TOML parser |
| `notify` | 6 | File watching |
| `arboard` | 3 | Clipboard |
| `clap` | 4 | CLI argument parser |
| `serde_json` | 1 | Locale JSON parsing |
| `sys-locale` | 0.3 | OS locale detection |
