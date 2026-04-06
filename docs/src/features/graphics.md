# Graphics — Sixel & Kitty Protocol

Nexterm supports both **Sixel** and the **Kitty Graphics Protocol**, enabling inline image
display directly in the terminal.

## Sixel

[Sixel](https://en.wikipedia.org/wiki/Sixel) is an image format developed by DEC.
Many CLI tools support Sixel output.

### Verification

```bash
# Install img2sixel (libsixel)
brew install libsixel   # macOS
apt install libsixel-bin # Ubuntu

# Display an image
img2sixel ~/Pictures/photo.jpg

# viu (Rust) also works
cargo install viu
viu ~/Pictures/photo.jpg
```

### Supported Escape Sequence

```
DCS P1;P2;P3 q <sixel_data> ST
```

- `P1`: aspect ratio (usually 0 or omitted)
- `P3`: background handling (0 = transparent background, 1 = preserve background)

---

## Kitty Graphics Protocol

The [Kitty Graphics Protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) is a
more capable image protocol that transmits Base64-encoded RGBA/RGB data.

### Verification

```bash
# Kitty-protocol-aware tools
pip install ranger-fm  # file manager
cargo install termpdf  # PDF viewer

# Manual test in Python
python3 - <<'EOF'
import base64, sys

def show_image(path):
    with open(path, 'rb') as f:
        data = f.read()
    encoded = base64.standard_b64encode(data).decode()
    # Send via Kitty protocol (f=32 RGBA, s=width, v=height)
    sys.stdout.buffer.write(
        b'\x1b_Ga=T,f=32,s=100,v=100;' + encoded.encode() + b'\x1b\\'
    )
    sys.stdout.flush()
EOF
```

### Supported Parameters

| Parameter | Description |
|-----------|-------------|
| `a=T` | Transmit action |
| `f=32` | RGBA 8-bit format |
| `f=24` | RGB 8-bit format (automatically converted to RGBA) |
| `s=<width>` | Image width in pixels |
| `v=<height>` | Image height in pixels |
| `m=1` | Chunked transfer (multiple chunks allowed) |

---

## Performance

- Images are cached in a GPU texture (keyed by `image_id`).
- Re-rendering the same image ID requires no texture regeneration.
- Very large images (4K+) may take additional time to transfer.

---

## Known Limitations

- Sixel HLS color model uses a simplified conversion (white approximation).
- Animated Sixel is not supported.
- Kitty `a=p` (display only, no transfer) is not supported.
