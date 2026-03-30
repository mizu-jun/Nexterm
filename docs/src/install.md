# Installation

See [README.md](https://github.com/mizu-jun/Nexterm#build) for full build and install instructions.

## Linux / macOS

Download the latest release tarball from the [releases page](https://github.com/mizu-jun/Nexterm/releases) and extract the binaries to a directory on your `PATH`, or install via Homebrew once the tap is published.

## Windows

Download the MSI installer from the [releases page](https://github.com/mizu-jun/Nexterm/releases). The installer registers `nexterm-server` as a Windows Service and adds all binaries to `PATH`. See [Windows Quick Start](windows.md) for details.

## Building from Source

Requires Rust 1.78+ and the platform graphics SDK (DirectX on Windows, Metal on macOS, Vulkan headers on Linux).

```sh
git clone https://github.com/mizu-jun/Nexterm
cd Nexterm
cargo build --release
```
