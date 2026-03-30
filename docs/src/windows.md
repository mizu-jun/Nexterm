# Windows Quick Start

See the [project README](https://github.com/mizu-jun/Nexterm#windows) for full Windows setup instructions.

## Requirements

- Windows 10 version 1809 or later (ConPTY support required)
- DirectX 11-capable GPU
- PowerShell 7 recommended (falls back to `powershell.exe`)

## Installation

Download `nexterm-v0.4.0-windows-x86_64.msi` from the [releases page](https://github.com/mizu-jun/Nexterm/releases) and run the installer. It will:

1. Install all binaries to `%ProgramFiles%\Nexterm\`
2. Add the install directory to `PATH`
3. Register `nexterm-server` as a Windows Service (auto-start)

## Running

After installation, open a new terminal and run:

```powershell
nexterm
```

The launcher will connect to the running service (or start one on first launch) and open the GPU client window.
