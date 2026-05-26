# Nexterm Benchmarks

> Sprint 5-3 / C1 / C5 / J4: throughput and keystroke-latency measurements at the VT-parser layer.

## 1. Goals and caveats

This document measures the throughput of Nexterm's **VT layer in isolation**.
End-to-end terminal latency (keypress → pixels on screen) is also influenced by the
GPU, winit, the windowing system, and the display refresh rate, so the numbers here
cannot be compared directly with the figures published by other terminals
(Alacritty 3 ms, kitty 3 ms, Ghostty 2 ms, etc.).

These benchmarks answer the following questions:

- Confirm that the VT layer is not a bottleneck (the typical workload does not stall
  before GPU rendering).
- Provide a regression baseline so we notice if the same scenario gets slower in the
  future.

End-to-end latency measurement (via external tools such as typometer, on real
hardware) remains future work.

## 2. Reference environment

| Item | Value |
|------|-------|
| CPU | x86_64 mobile-class laptop |
| OS | Windows 11 (64-bit) |
| Rust | rustc stable (edition 2024 in `Cargo.toml` → 1.85+) |
| Build | `cargo bench --release` profile (criterion defaults) |
| Commit | master as of 2026-05-11 |

The exact hardware depends on the operator. To improve reproducibility, consider
also running the same benches on the GitHub Actions `ubuntu-latest` runner and
recording those numbers alongside the local results (a fixed environment is more
reproducible).

Run the benches with:

```sh
cargo bench -p nexterm-vt --bench vt_throughput
```

All scenarios live in `nexterm-vt/benches/vt_throughput.rs`.

## 3. VT throughput (`vt_advance`)

Each scenario feeds a **256 KiB byte stream** into `VtParser::new(80, 24).advance()`
and reports the peak value with `criterion --quick`.
The scenarios are inspired by
[alacritty/vtebench](https://github.com/alacritty/vtebench).

| Scenario | Time (ms) | Throughput (MiB/s) | Description |
|---|---:|---:|---|
| `light_cells` | 6.16 | 40.6 | ASCII text only, CRLF-delimited |
| `medium_cells` | 6.78 | 36.9 | Lots of ANSI 8-colour SGR (like `ls --color`) |
| `dense_cells` | 1.83 | 136.3 | 24-bit RGB foreground/background, heavily decorated, few newlines |
| `cursor_motion` | 2.19 | 114.0 | Heavy CSI H cursor moves (vim/htop-style) |
| `scrolling` | 7.50 | 33.3 | Continuous scroll via many newlines (`tail -f`-style) |
| `alt_screen_random` | 2.31 | 108.2 | Alt screen + deterministic random-position draws |
| `sync_output` | 9.38 | 26.7 | DEC ?2026 synchronized output — equivalent to a full TUI redraw |

Median values. Confidence intervals are reported by criterion when you run
`cargo bench`.

## 4. Keystroke latency (`vt_keystroke_latency`)

Time taken to push the byte sequence for a single keypress through
`advance` → `take_dirty_rows`. This is a synthetic micro-benchmark that gives
us an upper bound on the VT layer's latency.

| Scenario | Time | Description |
|---|---:|---|
| `single_ascii` | 133 ns | A single ASCII character |
| `enter_newline` | 3.15 μs | CR LF (scroll triggered at the bottom of the buffer) |
| `backspace` | 114 ns | Typical erase sequence: BS + space + BS |
| `cursor_up` | 57 ns | One-row CSI A cursor up |
| `colored_char` | 326 ns | SGR colour + character + reset |

All values are **sub-microsecond**, which shows that the VT layer is not the
bottleneck in Nexterm's end-to-end latency (the GPU, the compositor, and the
monitor dominate).

## 5. Comparison with other terminals (published values)

These are reference values only. Methodology and environments differ, so this is
not a strict ranking.

| Terminal | End-to-end latency (published) | Source |
|---|---:|---|
| Ghostty | ~2 ms | Project README |
| Alacritty | ~3 ms | Project wiki |
| kitty | ~3 ms | Project wiki |
| Nexterm (VT layer only) | < 1 μs | This document |
| Nexterm (end-to-end) | not measured | Future work |

## 6. Known limitations

- **PTY-layer overhead is unmeasured**: the PTY reader thread inside
  `nexterm-server::Pane` and the IPC serialization (postcard) cost needs to be
  measured separately.
- **No GPU benches yet**: the `nexterm-client-gpu` three-pass renderer (background,
  text, image) has no GPU-side micro-bench. Tracked for Sprint 5-4 and later.
- **`session_manager` tests are excluded from coverage** because they fork a real
  PTY and tend to hang on CI (known heavy tests).

## 7. Regression-detection workflow

We do not run benches in CI today (they take too long). Before a release, run
locally and compare with previous results stored under `target/criterion/`:

```sh
cargo bench -p nexterm-vt --bench vt_throughput
```

`criterion` automatically diffs the new run against the previous one and reports
`No change in performance detected`, `improved`, or `regressed`.

## 8. Related material

- Audit round 2: items C1 / C5 / J4 in `memory/project_audit_round2.md`
- ADR-0001: wgpu upgrade plan (`docs/adr/0001-wgpu-upgrade.md`)
- Sprint 5-3 progress notes (memory `project_sprint5_3_progress.md`)
