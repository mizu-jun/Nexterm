# Nexterm

**GPU-accelerated terminal multiplexer** written in Rust.

Inspired by tmux/zellij, featuring:
- **wgpu GPU rendering** — DirectX 12 / Metal / Vulkan
- **Session persistence** — sessions survive client disconnects
- **Built-in SSH** with SFTP transfer, serial port, ProxyJump, SOCKS5
- **Web Terminal** with TLS / OAuth / TOTP authentication
- **Lua automation** — macros, event hooks, status bar
- **WASM plugin runtime** with sandboxed fuel / memory limits (API v2)
- **8-language UI** — EN / JA / KO / ZH / DE / FR / ES / IT
- **Supply chain hardening** — minisign + SLSA + CycloneDX SBOM + STRIDE
- **Windows MSI installer** with Service registration

## Quick Links

- [Installation](install.md)
- [Quick Start](quickstart.md)
- [Configuration Reference](config/toml.md)
- [Troubleshooting](troubleshooting.md)
- [Architecture Decision Records](adr-index.md)
- [GitHub](https://github.com/mizu-jun/Nexterm)

## Latest Release: v1.1.0

See the [CHANGELOG](https://github.com/mizu-jun/Nexterm/blob/master/CHANGELOG.md) for release notes and migration tips.
