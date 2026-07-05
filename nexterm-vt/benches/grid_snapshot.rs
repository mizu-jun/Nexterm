//! Audit round 3 (P1/P2): measure the cost the PTY reader pays per output burst
//! to keep its `latest_grid` snapshot fresh.
//!
//! Run with: `cargo bench -p nexterm-vt --bench grid_snapshot`.
//!
//! Two comparisons:
//! - `snapshot_refresh`: the old hot path cloned the whole parser grid on every
//!   burst (`full_grid_clone`) vs. the new one that applies only the changed
//!   rows onto a persistent snapshot (`apply_dirty_rows`).
//! - `take_dirty_rows`: draining dirty rows after a full-screen repaint, to show
//!   the `Vec::with_capacity` change carries no regression.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use nexterm_proto::Grid;
use nexterm_vt::VtParser;
use std::hint::black_box;

/// Fill a parser with one full screen of styled content and drain the initial
/// dirty rows so subsequent measurements reflect steady-state updates.
fn seeded_parser(cols: u16, rows: u16) -> VtParser {
    let mut parser = VtParser::new(cols, rows);
    let mut buf = Vec::new();
    for _ in 0..rows {
        buf.extend_from_slice(b"\x1b[32mThe quick brown fox 0123456789 !@#$%^&*()\x1b[0m\r\n");
    }
    parser.advance(&buf);
    parser.screen_mut().take_dirty_rows();
    parser
}

/// Compare full-grid clone vs. incremental row application for one burst that
/// touches a handful of rows (typical steady-state output).
fn snapshot_refresh(c: &mut Criterion) {
    const COLS: u16 = 200;
    const ROWS: u16 = 200;

    let mut group = c.benchmark_group("snapshot_refresh");
    group.throughput(Throughput::Elements(1));

    // A single-row, in-place update that does NOT scroll (cursor to row 5, then
    // overwrite it). This is the common partial-update case (progress bars,
    // status lines, cursor moves) where only one row is dirty — exactly where
    // the old full-grid clone did the most wasted work.
    const UPDATE: &[u8] = b"\x1b[5;1Hupdated status line payload 0123456789";

    // Old hot path: clone the entire parser grid on every burst.
    group.bench_function("full_grid_clone_200x200", |b| {
        let mut parser = seeded_parser(COLS, ROWS);
        let mut snapshot = Grid::new(COLS, ROWS);
        b.iter(|| {
            parser.advance(black_box(UPDATE));
            let _ = parser.screen_mut().take_dirty_rows();
            snapshot = parser.screen().full_refresh_grid();
            black_box(&snapshot);
        });
    });

    // New hot path: apply only the dirty rows onto a persistent snapshot.
    group.bench_function("apply_dirty_rows_200x200", |b| {
        let mut parser = seeded_parser(COLS, ROWS);
        let mut snapshot = Grid::new(COLS, ROWS);
        b.iter(|| {
            parser.advance(black_box(UPDATE));
            let dirty = parser.screen_mut().take_dirty_rows();
            for d in &dirty {
                snapshot.apply_dirty_row(d);
            }
            let (cc, cr) = parser.screen().cursor();
            snapshot.cursor_col = cc;
            snapshot.cursor_row = cr;
            black_box(&snapshot);
        });
    });

    group.finish();
}

/// Draining dirty rows after a full-screen repaint (worst case for the Vec size).
fn take_dirty_rows_full_screen(c: &mut Criterion) {
    const COLS: u16 = 200;
    const ROWS: u16 = 200;

    let mut group = c.benchmark_group("take_dirty_rows_full_screen");
    group.throughput(Throughput::Elements(ROWS as u64));
    group.bench_function("repaint_200_rows", |b| {
        let mut parser = seeded_parser(COLS, ROWS);
        // A clear + home marks the whole screen dirty on the next repaint.
        b.iter(|| {
            parser.advance(black_box(b"\x1b[2J\x1b[H"));
            for _ in 0..ROWS {
                parser.advance(black_box(b"row payload with several fields\r\n"));
            }
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });
    group.finish();
}

criterion_group!(benches, snapshot_refresh, take_dirty_rows_full_screen);
criterion_main!(benches);
