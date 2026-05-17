# Installation

See [README.md](https://github.com/mizu-jun/Nexterm#build) for full build and install instructions.

## Linux

Download the latest release tarball from the [releases page](https://github.com/mizu-jun/Nexterm/releases) and extract the binaries to a directory on your `PATH`, or install via Homebrew once the tap is published.

## macOS

Download the tarball matching your CPU from the [releases page](https://github.com/mizu-jun/Nexterm/releases):

| CPU | Asset |
|-----|-------|
| Apple Silicon (M1/M2/M3) | `nexterm-vX.Y.Z-macos-arm64.tar.gz` |
| Intel | `nexterm-vX.Y.Z-macos-x86_64.tar.gz` |

macOS Gatekeeper blocks unsigned binaries downloaded from the internet. Remove the quarantine attribute before running:

```sh
xattr -dr com.apple.quarantine nexterm-vX.Y.Z-macos-arm64.tar.gz
tar xzf nexterm-vX.Y.Z-macos-arm64.tar.gz
sudo mv nexterm* /usr/local/bin/
nexterm
```

Alternatively, allow the binary via **System Settings → Privacy & Security → Allow Anyway**.

## Windows

Download the MSI installer from the [releases page](https://github.com/mizu-jun/Nexterm/releases). The installer registers `nexterm-server` as a Windows Service and adds all binaries to `PATH`. See [Windows Quick Start](windows.md) for details.

## Building from Source

Requires Rust 1.85+ (workspace `edition = "2024"` requires 1.85+) and the platform graphics SDK (DirectX on Windows, Metal on macOS, Vulkan headers on Linux).

```sh
git clone https://github.com/mizu-jun/Nexterm
cd Nexterm
cargo build --release
```
