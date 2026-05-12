# Troubleshooting

This page lists common issues and their resolutions when running Nexterm.

If your issue is not listed, please open a GitHub issue with:
- Nexterm version (`nexterm-ctl --version`)
- OS / GPU driver version
- The exact command you ran
- The stderr output captured with `NEXTERM_LOG=debug`

## Build / Install

### `cargo build` fails with missing `libx11-dev` (Linux)

The GPU client links against several X11/Wayland/audio system libraries.
On Ubuntu / Debian, install:

```bash
sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev
```

### `candle.exe` returns `CNDL0289` during MSI build (Windows)

The WiX preprocessor variable syntax for `candle.exe` must use `-dName=Value` (no space).
When invoked from PowerShell, write the argument as one token:

```powershell
& candle.exe "-dVersion=$version" ...
```

See `CLAUDE.md` for the canonical build invocation.

## Runtime

### Server does not start / `nexterm` exits with `IPC ソケットの作成に失敗`

Check whether another `nexterm-server` instance owns the socket:

- Linux/macOS: `ls $XDG_RUNTIME_DIR/nexterm.sock`
- Windows: `Get-ChildItem \\.\pipe\nexterm-*`

Remove the stale socket file (Linux only — Windows pipes vanish automatically) and retry.

### Sessions are gone after upgrade to v2.0.0

`SNAPSHOT_VERSION_MIN` will be raised from 1 to 2 at v2.0.0 (see [ADR-0007](https://github.com/mizu-jun/Nexterm/blob/master/docs/adr/0007-snapshot-v1-deprecation.md)).
Run any v1.x release once before upgrading to v2.0.0 so the snapshot is migrated to v2 automatically.

### Plugins emit `deprecation warning: API v1 → v2`

Your plugin exports no `nexterm_api_version()` function, so the host treats it as v1.
Add the v2 ABI declaration (see `examples/plugins/README.md`).
v1 will be removed in v2.0.0 — see [ADR-0003](https://github.com/mizu-jun/Nexterm/blob/master/docs/adr/0003-plugin-api-v2.md).

### GPU initialization fails on Linux laptop with hybrid graphics

Force the discrete GPU before launching:

```bash
DRI_PRIME=1 nexterm
# or for NVIDIA Optimus:
__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia nexterm
```

If the discrete GPU is still skipped, set `[gpu] backend = "vulkan"` in `config.toml`.

### High input latency on Windows 11

Enable `mailbox` present mode in `config.toml`:

```toml
[gpu]
present_mode = "mailbox"
```

`auto` defaults to `fifo` on most drivers. `mailbox` trims one display frame at the cost of
occasional tearing under heavy load.

### `nexterm-ctl wsl import-profiles` does not detect distros

Confirm WSL is installed and `wsl.exe --list --verbose` lists at least one distro.
Then re-run with `--dry-run` to preview:

```bash
nexterm-ctl wsl import-profiles --dry-run
```

## SSH

### `known_hosts: ホスト鍵が変更されています` blocks connection

This indicates a host key mismatch — possibly a MITM attack or a legitimate server rotation.
If you trust the new key, remove the offending line from `~/.ssh/known_hosts` and reconnect.
**Never disable host key verification unconditionally.**

### Agent authentication on Windows fails

SSH agent authentication is currently Unix-only.
On Windows, use private key files (`auth_type = "key"` in the host entry) until agent support
lands. Track issue: [ADR/Roadmap](https://github.com/mizu-jun/Nexterm).

## Web Terminal

### Browser shows `401 Unauthorized` even with valid token

Tokens are sourced from `WEB_AUTH_TOKEN` env var first, then `config.toml`.
If both differ, the env var wins. Verify with:

```bash
env | grep WEB_AUTH_TOKEN
```

### OAuth callback returns `redirect_uri_mismatch`

The redirect URI registered in your OAuth provider must exactly match the
`redirect_url` field in `[web.oauth]`. Trailing slashes count.

## Performance

### Frame rate drops below 60 fps when scrolling

Check:
1. `present_mode = "mailbox"` ([above](#high-input-latency-on-windows-11))
2. GPU driver up to date
3. `[font]` size larger than 24pt — glyph atlas thrashing increases. Lower `glyph_atlas_size` if
   memory is tight, or raise it (default 1024) for fewer evictions.

Run `cargo bench -p nexterm-vt` to verify the VT parser itself isn't the bottleneck.

## Where to get help

- [GitHub Discussions](https://github.com/mizu-jun/Nexterm/discussions)
- [GitHub Issues](https://github.com/mizu-jun/Nexterm/issues)
- `NEXTERM_LOG=trace nexterm-server 2> trace.log` for deep diagnostics
