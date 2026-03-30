# SignPath Foundation — Open Source Application

## Project Information

**Project name:** Nexterm

**Short description:**
Nexterm is a GPU-accelerated terminal multiplexer written in Rust. It provides session persistence (like tmux/zellij), a built-in SSH client with SFTP transfer and port forwarding, serial port connectivity, Lua automation scripting, and an 8-language UI. The GPU client renders via wgpu (DirectX 11 / Metal / Vulkan). Windows users receive a full MSI installer with optional Windows Service registration.

**GitHub repository URL:** https://github.com/mizu-jun/Nexterm

**Homepage / documentation:** https://mizu-jun.github.io/Nexterm/

**License:** MIT OR Apache-2.0
(Both LICENSE-MIT and LICENSE-APACHE are present in the repository root.)

---

## Open Source Confirmation

Nexterm is free and open-source software. The full source code is publicly available at the GitHub URL above under the MIT and Apache 2.0 dual-license terms. There are no proprietary components, no obfuscated code, and no closed-source dependencies in the distributed binaries. All dependencies are published Rust crates with OSI-approved licenses.

---

## Build Pipeline

Nexterm uses GitHub Actions for all CI and release builds.

**Relevant workflow files:**
- `.github/workflows/ci.yml` — runs `cargo test` and `cargo clippy` on every push and pull request
- `.github/workflows/release.yml` — triggered on version tags (`v*`); builds release binaries for Linux x86_64, macOS arm64, and Windows x86_64, then publishes them as GitHub Release assets along with the MSI installer

**Build tool chain:**
- Rust stable toolchain (pinned via `rust-toolchain.toml`)
- `cargo build --release` for all targets
- WiX Toolset 4.x for the Windows MSI package (invoked via `cargo wix` in the `wix/` directory)
- No build scripts that download or execute pre-built third-party binaries

**Reproducibility:**
Cargo.lock is committed to the repository, ensuring dependency versions are pinned. All dependencies are resolved from crates.io at build time with no network access to arbitrary sources.

---

## Security Practices

**Memory safety:**
The entire codebase is written in Rust with no `unsafe` blocks outside of well-audited FFI boundaries (wgpu platform bindings). Rust's ownership model eliminates the class of memory corruption vulnerabilities that code-signing is intended to protect against.

**IPC security:**
The server accepts connections only over a local Unix domain socket (Linux/macOS) or a named pipe with a restrictive ACL (Windows). Peer credentials are verified on connection using `SO_PEERCRED` (Linux) or pipe security descriptors (Windows). Remote connections are not accepted by default.

**Credential handling:**
SSH passwords and passphrases are stored in the OS keychain (libsecret on Linux, Keychain on macOS, Windows Credential Manager on Windows) and are never written to disk in plaintext. In-memory credential buffers use the `zeroize` crate to clear secrets when dropped.

**Dependency auditing:**
The CI pipeline runs `cargo audit` on every push to check all dependencies against the RustSec advisory database.

---

## Maintainer / Contact

**GitHub username:** mizu-jun

**Repository:** https://github.com/mizu-jun/Nexterm

**Contact email:** _[maintainer's email address — fill in before submitting]_

**Country of residence:** _[fill in before submitting]_

---

## Additional Notes for SignPath Foundation Reviewers

- The Windows MSI is built entirely from source using WiX Toolset; no pre-built binary blobs are bundled.
- The release workflow does not use self-hosted runners; all builds run on GitHub-hosted `ubuntu-latest` and `windows-latest` images.
- Signing is needed for `nexterm-server.exe`, `nexterm-client-gpu.exe`, `nexterm-client-tui.exe`, `nexterm-ctl.exe`, `nexterm.exe`, and the `nexterm-v*.msi` installer.
- The project does not monetize its users and does not include telemetry or analytics.
