//! Sprint 5-3 / C5: VT パーサのスループットベンチマーク
//!
//! alacritty/vtebench のシナリオを参考に、`VtParser::advance` の
//! 単位時間あたりのバイト処理速度を計測する。
//!
//! 実行: `cargo bench -p nexterm-vt --bench vt_throughput`
//!
//! 各シナリオ:
//! - light_cells: ASCII のみ（最も基本的なスループット）
//! - medium_cells: ANSI 8 色 + ASCII（`ls --color` 風）
//! - dense_cells: 24-bit カラー前景背景フル（フル装飾）
//! - cursor_motion: CSI H で大量カーソル移動（vim / htop 風）
//! - scrolling: 改行で連続スクロール（`tail -f` 風）
//! - alt_screen_random: 代替画面 + ランダム配置（TUI 全画面再描画風）
//! - sync_output: DEC ?2026 で同期描画
//!
//! 表示される時間は 1 シナリオ分のバイト列を 1 回処理する時間。
//! スループット (MB/s) は criterion の `throughput` 機能で自動表示される。

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nexterm_vt::VtParser;
use std::hint::black_box;

/// ASCII テキストのみのシナリオ。
///
/// 1 行 80 桁 × 24 行を CR LF 区切りで繰り返す。
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

/// ANSI 8 色 + ASCII のシナリオ。
///
/// `ls --color` や色付きログ出力の典型。SGR シーケンスを多数含む。
fn build_medium_cells(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    // 各カラム 8 種類の前景色を交互に変える
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

/// 24-bit カラー (truecolor) を前景背景に毎セル設定するシナリオ。
///
/// `bat` や `cmatrix` のような重い装飾出力の典型。
fn build_dense_cells(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    let mut col = 0u32;
    while buf.len() < bytes_target {
        let r = (col & 0xFF) as u8;
        let g = ((col >> 3) & 0xFF) as u8;
        let b = ((col >> 5) & 0xFF) as u8;
        // CSI 38;2;r;g;b;48;2;R;G;B m + 1 文字
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

/// CSI H でカーソルを大量に移動するシナリオ。
///
/// vim / htop / tmux のような TUI が画面更新する際の典型。
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

/// 改行で連続スクロールするシナリオ。
///
/// `tail -f` や長いビルドログの典型。
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

/// 代替画面 + ランダム位置への描画のシナリオ。
///
/// TUI が画面全体を再描画する際の典型（vim refresh 等）。
fn build_alt_screen_random(bytes_target: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(bytes_target);
    // 代替画面に切り替え
    buf.extend_from_slice(b"\x1b[?1049h");
    let mut seed: u32 = 0x1234_5678;
    while buf.len() < bytes_target {
        // 簡易 xorshift で row, col を決定論的に生成
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

/// DEC ?2026 同期出力で大量のテキストを送るシナリオ。
///
/// Sprint 5-2 / B5 で完全対応した経路の計測。
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

/// 1 シナリオ分のベンチを行うヘルパ。
///
/// criterion の `throughput` を `Bytes(len)` に設定することで MB/s を自動算出させる。
fn bench_scenario(c: &mut Criterion, name: &str, data: &[u8]) {
    let mut group = c.benchmark_group("vt_advance");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_with_input(BenchmarkId::new(name, data.len()), data, |b, data| {
        // 各イテレーションで Parser を作り直す（warm cache の影響を抑える）
        b.iter(|| {
            let mut parser = VtParser::new(80, 24);
            parser.advance(black_box(data));
            // grid() を黒箱に渡してデッドコード除去を防ぐ
            black_box(parser.screen().grid().get(0, 0));
        });
    });
    group.finish();
}

/// すべてのシナリオを 256 KiB / 1 シナリオで実行する。
fn vt_throughput(c: &mut Criterion) {
    // 1 シナリオあたりのターゲットバイト数。
    // ローカル実行で十分な精度が出るサイズ。CI のジョブ時間制限内でも完走可能。
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

/// Sprint 5-3 / C1: 入力レイテンシ（per-keystroke）ベンチマーク
///
/// `Ghostty 2 ms / Alacritty 3 ms / kitty 3 ms` といったエンドツーエンドの
/// レイテンシは GPU + winit + コンポジタが絡むため正確な比較は困難。
/// ここでは「タイピング 1 文字相当のバイト列を VT に流して dirty を取り出す」
/// 経路に絞った時間を計測する。VT 層のオーバーヘッドの上限を可視化する目的。
fn vt_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("vt_keystroke_latency");
    // throughput を Elements(1) にすることで「1 キーストローク」あたりの ns を表示する。
    group.throughput(Throughput::Elements(1));

    // 1) 単一 ASCII 文字 (b"a") を打鍵 → dirty 抽出
    group.bench_function("single_ascii", |b| {
        let mut parser = VtParser::new(80, 24);
        b.iter(|| {
            parser.advance(black_box(b"a"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    // 2) Enter (CR LF) で改行 → dirty 抽出
    group.bench_function("enter_newline", |b| {
        let mut parser = VtParser::new(80, 24);
        b.iter(|| {
            parser.advance(black_box(b"\r\n"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    // 3) Backspace 相当（BS + Space + BS）→ dirty 抽出
    group.bench_function("backspace", |b| {
        let mut parser = VtParser::new(80, 24);
        // 事前に何か入力してから BS
        parser.advance(b"abc");
        parser.screen_mut().take_dirty_rows();
        b.iter(|| {
            parser.advance(black_box(b"\x08 \x08"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    // 4) カーソル移動 (CSI A) → dirty 抽出
    group.bench_function("cursor_up", |b| {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[10;10H"); // 初期位置
        parser.screen_mut().take_dirty_rows();
        b.iter(|| {
            parser.advance(black_box(b"\x1b[A"));
            black_box(parser.screen_mut().take_dirty_rows());
        });
    });

    // 5) SGR カラー変更を伴う 1 文字 (色付き打鍵)
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
