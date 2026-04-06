# Performance Tuning

## GPU Renderer Optimization

### FPS Cap

The default is 60 FPS. Adjust to match your monitor's refresh rate:

```toml
[gpu]
fps_limit = 144   # 144 Hz monitor
fps_limit = 0     # uncapped (vsync only)
```

### Glyph Atlas Size

The glyph atlas caches font glyphs in a GPU texture.
The default 2048×2048 is sufficient for typical use, but 4096 is recommended
for large font sizes or a large number of Unicode characters:

```toml
[gpu]
atlas_size = 4096  # for high-DPI or large font sizes
```

---

## Startup Time

### Server / Client Separation

Because Nexterm uses a server/client architecture, the first launch includes a brief
server-startup wait (typically < 200 ms).

**Subsequent launches**: if the server is already running, the client connects instantly.
The launcher's detection polling uses exponential backoff (10 ms → 100 ms) for efficiency.

### Session Persistence

Keep the server running to connect instantly on client restart without any wait:

```bash
# Start server in the background and leave it running
nexterm-server &

# Clients can start and exit freely; sessions remain alive
```

---

## Memory Usage

### Scrollback Line Count

Scrollback memory usage scales with: `pane count × line count × bytes per line`.

```toml
# Default: 50,000 lines (~200 MB @ 80 cols)
scrollback_lines = 10000  # conserve memory
scrollback_lines = 100000 # retain more log history
```

---

## Scroll Performance

The GPU renderer does not redraw the entire scrollback every frame.
Only rows marked dirty are reprocessed.
No GPU buffer reallocation occurs during scrolling.

---

## Profiling

If you encounter performance issues, enable debug logging with an environment variable:

```bash
NEXTERM_LOG=debug nexterm
```

For detailed GPU-side profiling, use [RenderDoc](https://renderdoc.org/)
(wgpu supports RenderDoc integration).
