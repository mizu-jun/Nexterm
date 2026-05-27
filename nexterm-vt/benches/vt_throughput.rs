//! Sprint 5-3 / C5: throughput benchmarks for the VT parser.
//!
//! Modeled after the `alacritty/vtebench` scenarios, this benchmark measures the
//! number of bytes per second that `VtParser::advance` can process.
//!
//! Run with: `cargo bench -p nexterm-vt --bench vt_throughput`.
//!
//! Scenarios:
//! - `light_cells`: ASCII only (the most basic throughput).
//! - `medium_cells`: ANSI 8-color + ASCII (`ls --color`-style output).
//! - `dense_cells`: 24-bit color on both foreground and background (full styling).
//! - `cursor_motion`: heavy cursor movement via CSI H (vim / htop-style).
//! - `scrolling`: continuous scrolling via newlines (`tail -f`-style).
//! - `alt_screen_random`: alternate screen with random placement
//!   (full-screen TUI repaints).
//! - `sync_output`: synchronized drawing via DEC ?2026.
//!
//! The displayed time is for processing one scenario's byte stream once.
//! Throughput (MB/s) is shown automatically by criterion's `throughput` feature.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nexterm_vt::VtParser;
use std::hint::black_box;

/// ASCII-only scenario.
///
/// Repeats a 24-row × 80-column line separated by CR LF.
fn build_light_cells(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    let line: &[u8] = b"The quick brown fox jumps over the lazy dog. 0123456789 !@#$%^&*()_+-=  ";
    while buf.len() < bytes_target {
        buf.extend_from_slice(line);
        buf.extend_from_slice(b"\r\n");
    }
    buf.truncate(bytes_target);
    buf
}

/// ANSI 8-color + ASCII scenario.
///
/// Typical of `ls --color` and colored log output; contains many SGR sequences.
fn build_medium_cells(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    // Alternate through 8 foreground colors across columns.
    let colors: [u8; 8] = [30, 31, 32, 33, 34, 35, 36, 37];
    let mut row = 0u32;
    while buf.len() < bytes_target {
        for &c in &colors {
            buf.extend_from_slice(b"\x1b[");
            buf.extend_from_slice(c.to_string().as_bytes());
            buf.extend_from_slice(b"mHello ");
        }
        buf.extend_from_slice(b"\x1b[0m\r\n");
        row = row.wrapping_add(1);
    }
    buf.truncate(bytes_target);
    buf
}

/// Per-cell 24-bit color (truecolor) on both foreground and background.
///
/// Typical of heavily styled output such as `bat` or `cmatrix`.
fn build_dense_cells(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    let mut col = 0u32;
    while buf.len() < bytes_target {
        let r = (col & 0xFF) as u8;
        let g = ((col >> 3) & 0xFF) as u8;
        let b = ((col >> 5) & 0xFF) as u8;
        // CSI 38;2;r;g;b;48;2;R;G;B m + 1 character.
        buf.extend_from_slice(b"\x1b[38;2;");
        buf.extend_from_slice(r.to_string().as_bytes());
        buf.push(b';');
        buf.extend_from_slice(g.to_string().as_bytes());
        buf.push(b';');
        buf.extend_from_slice(b.to_string().as_bytes());
        buf.extend_from_slice(b";48;2;");
        buf.extend_from_slice((255 - r).to_string().as_bytes());
        buf.push(b';');
        buf.extend_from_slice((255 - g).to_string().as_bytes());
        buf.push(b';');
        buf.extend_from_slice((255 - b).to_string().as_bytes());
        buf.extend_from_slice(b"m#");
        col = col.wrapping_add(1);
        if col.is_multiple_of(80) {
            buf.extend_from_slice(b"\x1b[0m\r\n");
        }
    }
    buf.truncate(bytes_target);
    buf
}

/// Heavy cursor motion via CSI H.
///
/// Typical of TUI updates from vim / htop / tmux.
fn build_cursor_motion(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    let mut row = 1u16;
    let mut col = 1u16;
    while buf.len() < bytes_target {
        buf.extend_from_slice(b"\x1b[");
        buf.extend_from_slice(row.to_string().as_bytes());
        buf.push(b';');
        buf.extend_from_slice(col.to_string().as_bytes());
        buf.extend_from_slice(b"H*");
        col = if col >= 79 { 1 } else { col + 1 };
        if col == 1 {
            row = if row >= 23 { 1 } else { row + 1 };
        }
    }
    buf.truncate(bytes_target);
    buf
}

/// Continuous scrolling driven by newlines.
///
/// Typical of `tail -f` or a long build log.
fn build_scrolling(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    let mut n = 0u64;
    while buf.len() < bytes_target {
        buf.extend_from_slice(b"[INFO] line ");
        buf.extend_from_slice(n.to_string().as_bytes());
        buf.extend_from_slice(b" processing batch with several payload fields\r\n");
        n = n.wrapping_add(1);
    }
    buf.truncate(bytes_target);
    buf
}

/// Alternate screen with paints at random positions.
///
/// Typical of a TUI that redraws the whole screen (e.g. a `vim` refresh).
fn build_alt_screen_random(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    // Switch to the alternate screen.
    buf.extend_from_slice(b"\x1b[?1049h");
    let mut seed: u32 = 0x1234_5678;
    while buf.len() < bytes_target {
        // Generate row/col deterministically with a small xorshift.
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let row = (seed % 24) + 1;
        let col = ((seed >> 8) % 80) + 1;
        buf.extend_from_slice(b"\x1b[");
        buf.extend_from_slice(row.to_string().as_bytes());
        buf.push(b';');
        buf.extend_from_slice(col.to_string().as_bytes());
        buf.extend_from_slice(b"HX");
    }
    buf.extend_from_slice(b"\x1b[?1049l");
    buf.truncate(bytes_target);
    buf
}

/// Large amount of text sent through DEC ?2026 synchronized output.
///
/// Measures the path that was completed in Sprint 5-2 / B5.
fn build_sync_output(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    while buf.len() < bytes_target {
        buf.extend_from_slice(b"\x1b[?2026h");
        for _ in 0..24 {
            buf.extend_from_slice(b"\x1b[2K\rRedraw line with payload data 0123456789\r\n");
        }
        buf.extend_from_slice(b"\x1b[?2026l");
    }
    buf.truncate(bytes_target);
    buf
}

/// Helper for benchmarking one scenario.
///
/// Setting criterion's `throughput` to `Bytes(len)` makes MB/s appear automatically.
fn bench_scenario(c: &mut Criterion, name: &str, data: &[u8]) {
    let mut group = c.benchmark_group("vt_advance");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_with_input(BenchmarkId::new(name, data.len()), data, |b, data| {
        // Recreate the parser each iteration to minimize warm-cache effects.
        b.iter(|| {
            let mut parser = VtParser::new(80, 24);
            parser.advance(black_box(data));
            // Pipe `grid()` through black_box so dead-code elimination does not strip it.
            black_box(parser.screen().grid().get(0, 0));
        });
    });
    group.finish();
}

/// Run every scenario at 256 KiB per scenario.
fn vt_throughput(c: &mut Criterion) {
    // Target bytes per scenario. Large enough for stable numbers locally and
    // still completes within the CI job timeout.
    const TARGET_BYTES: usize = 256 * 1024;

    bench_scenario(c, "light_cells", &build_light_cells(TARGET_BYTES));
    bench_scenario(c, "medium_cells", &build_medium_cells(TARGET_BYTES));
    bench_scenario(c, "dense_cells", &build_dense_cells(TARGET_BYTES));
    bench_scenario(c, "cursor_motion", &build_cursor_motion(TARGET_BYTES));
    bench_scenario(c, "scrolling", &build_scrolling(TARGET_BYTES));
    bench_scenario(
        c,
        "alt_screen_random",
        &build_alt_screen_random(TARGET_BYTES),
    );
    bench_scenario(c, "sync_output", &build_sync_output(TARGET_BYTES));
}

/// Sprint 5-3 / C1: per-keystroke input-latency benchmark.
///
/// End-to-end latency numbers like `Ghostty 2 ms / Alacritty 3 ms / kitty 3 ms`
/// involve the GPU, winit, and the compositor, so a precise comparison is
/// difficult. Here we measure only the path of "feeding a single keystroke
/// through the VT and pulling the dirty rows back out". The goal is to expose
/// the upper bound on the VT layer's overhead.
fn vt_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("vt_keystroke_latency");
    // Setting throughput to Elements(1) displays nanoseconds per keystroke.
    group.throughput(Throughput::Elements(1));

    // 1) A single ASCII character (`b"a"`) → take dirty rows.
    group.bench_function("single_ascii", |b| {
        let mut parser = VtParser::new(80, 24);
        b.iter(|| {
            parser.advance(black_box(b"a"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    // 2) Enter (CR LF) → take dirty rows.
    group.bench_function("enter_newline", |b| {
        let mut parser = VtParser::new(80, 24);
        b.iter(|| {
            parser.advance(black_box(b"\r\n"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    // 3) Backspace equivalent (BS + Space + BS) → take dirty rows.
    group.bench_function("backspace", |b| {
        let mut parser = VtParser::new(80, 24);
        // Type something first, then backspace it.
        parser.advance(b"abc");
        parser.screen_mut().take_dirty_rows();
        b.iter(|| {
            parser.advance(black_box(b"\x08 \x08"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    // 4) Cursor motion (CSI A) → take dirty rows.
    group.bench_function("cursor_up", |b| {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[10;10H"); // initial position
        parser.screen_mut().take_dirty_rows();
        b.iter(|| {
            parser.advance(black_box(b"\x1b[A"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    // 5) A single character with SGR color change (a colored keystroke).
    group.bench_function("colored_char", |b| {
        let mut parser = VtParser::new(80, 24);
        b.iter(|| {
            parser.advance(black_box(b"\x1b[31mr\x1b[0m"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    group.finish();
}

criterion_group!(benches, vt_throughput, vt_latency);
criterion_main!(benches);
