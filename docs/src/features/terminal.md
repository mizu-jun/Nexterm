# Terminal Emulation

See the [project README](https://github.com/mizu-jun/Nexterm) for a full feature overview.

Nexterm implements a VT100/VT220/xterm-compatible terminal emulator with GPU-accelerated rendering via wgpu, supporting DirectX 11 on Windows, Metal on macOS, and Vulkan on Linux.

## Supported Features

- Full xterm-256color and true-color (24-bit) support
- Alternate screen buffer (SMCUP/RMCUP) for apps like `vim`, `less`, `htop`
- OSC 0/1/2 window title updates
- OSC 9 desktop notifications
- CJK double-width character handling
- IME (Input Method Editor) support for East Asian input
- Scrollback buffer with configurable line count (default 50,000)
- In-buffer search via `Ctrl+Shift+F`
- Quick Select mode for URLs, file paths, IP addresses, and hashes
