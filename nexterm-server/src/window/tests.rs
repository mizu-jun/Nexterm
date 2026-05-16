use super::tiling::compute_tiling_layouts;
use super::*;
use crate::snapshot::*;

use proptest::prelude::*;

// ---- BSP レイアウト テスト ----

#[test]
fn bsp_垂直分割のレイアウト計算() {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    tree.insert_after(1, 2, SplitDir::Vertical);
    let mut out = Vec::new();
    tree.compute(0, 0, 80, 24, &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].pane_id, 1);
    assert_eq!(out[0].col_off, 0);
    assert!(out[1].col_off > 0);
    assert_eq!(out[0].cols + 1 + out[1].cols, 80);
}

#[test]
fn bsp_水平分割のレイアウト計算() {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    tree.insert_after(1, 2, SplitDir::Horizontal);
    let mut out = Vec::new();
    tree.compute(0, 0, 80, 24, &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].row_off, 0);
    assert!(out[1].row_off > 0);
    assert_eq!(out[0].rows + 1 + out[1].rows, 24);
}

#[test]
fn bsp_3分割のレイアウト計算() {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    tree.insert_after(1, 2, SplitDir::Vertical);
    tree.insert_after(2, 3, SplitDir::Horizontal);
    let mut out = Vec::new();
    tree.compute(0, 0, 80, 24, &mut out);
    assert_eq!(out.len(), 3);
}

#[test]
fn フォーカス移動の境界値() {
    let ids = [10u32, 20, 30];
    let pos = ids.iter().position(|&id| id == 30).unwrap();
    let next = ids[(pos + 1) % ids.len()];
    assert_eq!(next, 10);
    let pos = ids.iter().position(|&id| id == 10).unwrap();
    let prev = if pos == 0 { ids.len() - 1 } else { pos - 1 };
    assert_eq!(ids[prev], 30);
}

#[test]
fn bsp_4分割のレイアウト計算() {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    tree.insert_after(1, 2, SplitDir::Vertical);
    tree.insert_after(1, 3, SplitDir::Horizontal);
    tree.insert_after(2, 4, SplitDir::Horizontal);
    let mut out = Vec::new();
    tree.compute(0, 0, 80, 24, &mut out);
    assert_eq!(out.len(), 4);
    for p in &out {
        assert!(p.col_off < 80);
        assert!(p.row_off < 24);
        assert!(p.cols > 0);
        assert!(p.rows > 0);
    }
}

#[test]
fn スナップショット変換の往復整合性() {
    let snap_before = SplitNodeSnapshot::Split {
        dir: SplitDirSnapshot::Vertical,
        ratio: 0.5,
        left: Box::new(SplitNodeSnapshot::Pane {
            pane_id: 1,
            cwd: None,
        }),
        right: Box::new(SplitNodeSnapshot::Pane {
            pane_id: 2,
            cwd: None,
        }),
    };
    let node = bsp::SplitNode::from_snapshot(&snap_before);
    let snap_after = node.to_snapshot();
    let mut sizes_before = Vec::new();
    let mut sizes_after = Vec::new();
    tiling::compute_pane_sizes(&snap_before, 80, 24, &mut sizes_before);
    tiling::compute_pane_sizes(&snap_after, 80, 24, &mut sizes_after);
    assert_eq!(sizes_before, sizes_after);
}

// ---- タイリングレイアウト テスト ----

#[test]
fn tiling_1ペインは全画面() {
    let rects = compute_tiling_layouts(&[1], 80, 24);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].cols, 80);
    assert_eq!(rects[0].rows, 24);
    assert_eq!(rects[0].col_off, 0);
    assert_eq!(rects[0].row_off, 0);
}

#[test]
fn tiling_2ペインは横2分割() {
    let rects = compute_tiling_layouts(&[1, 2], 80, 24);
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0].col_off, 0);
    assert!(rects[1].col_off > 0);
    assert_eq!(rects[0].rows, 24);
    assert_eq!(rects[1].rows, 24);
}

#[test]
fn tiling_4ペインは2x2グリッド() {
    let rects = compute_tiling_layouts(&[1, 2, 3, 4], 80, 24);
    assert_eq!(rects.len(), 4);
    for r in &rects {
        assert!(r.cols > 0);
        assert!(r.rows > 0);
        assert!(r.col_off < 80);
        assert!(r.row_off < 24);
    }
    assert_eq!(rects[0].col_off, rects[1].col_off);
    assert_eq!(rects[2].col_off, rects[3].col_off);
    assert_ne!(rects[0].col_off, rects[2].col_off);
}

#[test]
fn tiling_5ペインは3列グリッド() {
    let rects = compute_tiling_layouts(&[1, 2, 3, 4, 5], 80, 24);
    assert_eq!(rects.len(), 5);
    for r in &rects {
        assert!(r.cols > 0);
        assert!(r.rows > 0);
    }
}

#[test]
fn tiling_空リストは空を返す() {
    let rects = compute_tiling_layouts(&[], 80, 24);
    assert!(rects.is_empty());
}

// ---- LayoutMode テスト ----

#[test]
fn layout_mode_from_str() {
    assert_eq!(LayoutMode::from_str("tiling"), LayoutMode::Tiling);
    assert_eq!(LayoutMode::from_str("bsp"), LayoutMode::Bsp);
    assert_eq!(LayoutMode::from_str("unknown"), LayoutMode::Bsp);
}

#[test]
fn layout_mode_default_is_bsp() {
    let mode: LayoutMode = Default::default();
    assert_eq!(mode, LayoutMode::Bsp);
}

#[test]
fn layout_mode_as_str() {
    assert_eq!(LayoutMode::Bsp.as_str(), "bsp");
    assert_eq!(LayoutMode::Tiling.as_str(), "tiling");
}

// ---- BSP / Tiling プロパティテスト（Sprint 4-4） ----

/// BSP 操作シーケンスを生成するための擬似 Op
#[derive(Clone, Debug)]
enum BspOp {
    /// 既存ペインを選択して新ペインで分割
    Insert {
        /// 現在のペインリストへのインデックス（剰余で正規化）
        target_idx: usize,
        /// 分割方向
        vertical: bool,
    },
    /// 既存ペインを削除（最後の 1 つは削除しない）
    Remove {
        /// 現在のペインリストへのインデックス
        target_idx: usize,
    },
}

fn arb_bsp_op() -> impl Strategy<Value = BspOp> {
    prop_oneof![
        (any::<usize>(), any::<bool>()).prop_map(|(target_idx, vertical)| BspOp::Insert {
            target_idx,
            vertical,
        }),
        any::<usize>().prop_map(|target_idx| BspOp::Remove { target_idx }),
    ]
}

/// 操作シーケンスを実行して (BSP ツリー, 全ペイン ID) を返す
fn run_bsp_ops(ops: &[BspOp]) -> (bsp::SplitNode, Vec<u32>) {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    let mut active_ids: Vec<u32> = vec![1];
    let mut next_id: u32 = 2;
    for op in ops {
        match op {
            BspOp::Insert {
                target_idx,
                vertical,
            } => {
                if active_ids.is_empty() {
                    continue;
                }
                let target = active_ids[*target_idx % active_ids.len()];
                let new_id = next_id;
                next_id += 1;
                let dir = if *vertical {
                    SplitDir::Vertical
                } else {
                    SplitDir::Horizontal
                };
                if tree.insert_after(target, new_id, dir) {
                    active_ids.push(new_id);
                }
            }
            BspOp::Remove { target_idx } => {
                // 最後の 1 ペインは削除しない（BSP ツリーが空にならない不変）
                if active_ids.len() <= 1 {
                    continue;
                }
                let idx = *target_idx % active_ids.len();
                let target = active_ids[idx];
                // ルート単独で RemoveSelf になるケースは active_ids.len() <= 1 ガードで弾けている
                if matches!(tree.remove(target), bsp::RemoveResult::Removed) {
                    active_ids.remove(idx);
                }
            }
        }
    }
    (tree, active_ids)
}

/// 矩形 a と b が重なっているかを判定する
fn rects_overlap(a: &PaneRect, b: &PaneRect) -> bool {
    let a_right = a.col_off + a.cols;
    let a_bottom = a.row_off + a.rows;
    let b_right = b.col_off + b.cols;
    let b_bottom = b.row_off + b.rows;
    !(a_right <= b.col_off
        || b_right <= a.col_off
        || a_bottom <= b.row_off
        || b_bottom <= a.row_off)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_local_rejects: 1024,
        ..ProptestConfig::default()
    })]

    /// 任意の BSP 操作シーケンスで `compute()` がパニックしないこと
    /// （ウィンドウサイズが極端に小さくゼロ寸法矩形が生まれても OK）
    ///
    /// 実装メモ: 現行 BSP は `rows < ペイン数` のような極小ウィンドウで
    /// `rows = 0` の矩形を生成する。レンダリング側は最小サイズを保証するが、
    /// パニックを起こさないことだけは厳格に検証する。
    #[test]
    fn bsp_compute_never_panics(
        ops in proptest::collection::vec(arb_bsp_op(), 0..40),
        cols in 1u16..=300,
        rows in 1u16..=120,
    ) {
        let (tree, _) = run_bsp_ops(&ops);
        let mut layout = Vec::new();
        // パニックなく完了すること
        tree.compute(0, 0, cols, rows, &mut layout);
    }

    /// 十分な領域がある場合、BSP の意味的不変条件が保たれること
    /// - 計算された pane_id 集合が active_ids と一致
    /// - 各矩形が画面内に収まる
    /// - cols > 0 && rows > 0
    /// - pane_id が一意
    ///
    /// ## ウィンドウサイズの選定根拠
    ///
    /// 現行 BSP は分割比 0.5 で再帰的に半分ずつにするため、深さ `d` の
    /// 右偏りツリーは各方向に `2^d` 以上のセル数を要求する。
    /// `target_idx=0` が常に root を指す proptest 入力（shrinking 後の
    /// 縮約形）は深さ `ops.len()` の右偏りを生むため、`2^ops.len()` 以上の
    /// サイズを確保する必要がある。
    ///
    /// 操作上限を 6 に絞れば 2^6 = 64 セル、+ 安全マージン込みで 1024 を採用する。
    /// これで現行実装の契約内で property を検証できる。
    /// （実装側が「最小サイズ保証」を入れれば、この上限は緩められる。
    ///   proptest 側は実装契約の境界を pin する役割を担う。）
    #[test]
    fn bsp_invariants_with_sufficient_space(
        ops in proptest::collection::vec(arb_bsp_op(), 0..=6),
    ) {
        let (tree, active_ids) = run_bsp_ops(&ops);
        let cols: u16 = 1024;
        let rows: u16 = 1024;

        let mut layout = Vec::new();
        tree.compute(0, 0, cols, rows, &mut layout);

        // ペイン数の整合性
        prop_assert_eq!(layout.len(), active_ids.len(),
            "compute() のペイン数 {} != active_ids 数 {}",
            layout.len(), active_ids.len());

        // 各矩形が有効
        for r in &layout {
            prop_assert!(r.cols > 0, "cols = 0 は不正: {:?}", r);
            prop_assert!(r.rows > 0, "rows = 0 は不正: {:?}", r);
            prop_assert!(r.col_off + r.cols <= cols,
                "矩形が右端を超える: col_off={} + cols={} > {}",
                r.col_off, r.cols, cols);
            prop_assert!(r.row_off + r.rows <= rows,
                "矩形が下端を超える: row_off={} + rows={} > {}",
                r.row_off, r.rows, rows);
        }

        // pane_id の一意性
        let mut ids: Vec<u32> = layout.iter().map(|r| r.pane_id).collect();
        ids.sort();
        let dup_count = ids.windows(2).filter(|w| w[0] == w[1]).count();
        prop_assert_eq!(dup_count, 0, "重複した pane_id: {:?}", ids);

        // active_ids と layout の ID 集合が一致
        let mut expected = active_ids.clone();
        expected.sort();
        prop_assert_eq!(ids, expected);
    }

    /// 十分な領域がある場合、BSP 矩形は互いに重ならないこと
    /// （Separator 行/列で常に離れている）
    /// ウィンドウサイズの選定根拠は `bsp_invariants_with_sufficient_space` を参照。
    #[test]
    fn bsp_rects_do_not_overlap(
        ops in proptest::collection::vec(arb_bsp_op(), 0..=6),
    ) {
        let (tree, _active_ids) = run_bsp_ops(&ops);
        let cols: u16 = 1024;
        let rows: u16 = 1024;

        let mut layout = Vec::new();
        tree.compute(0, 0, cols, rows, &mut layout);

        for i in 0..layout.len() {
            for j in (i + 1)..layout.len() {
                prop_assert!(!rects_overlap(&layout[i], &layout[j]),
                    "矩形 {:?} と {:?} が重なっている", layout[i], layout[j]);
            }
        }
    }

    /// BSP ツリーのスナップショット往復が pane ID と矩形を保存すること
    /// ウィンドウサイズの選定根拠は `bsp_invariants_with_sufficient_space` を参照。
    #[test]
    fn bsp_snapshot_roundtrip(
        ops in proptest::collection::vec(arb_bsp_op(), 0..=6),
    ) {
        let (tree, _active_ids) = run_bsp_ops(&ops);
        let cols: u16 = 1024;
        let rows: u16 = 1024;
        let snap = tree.to_snapshot();
        let rebuilt = bsp::SplitNode::from_snapshot(&snap);

        let mut layout_a = Vec::new();
        let mut layout_b = Vec::new();
        tree.compute(0, 0, cols, rows, &mut layout_a);
        rebuilt.compute(0, 0, cols, rows, &mut layout_b);

        prop_assert_eq!(layout_a.len(), layout_b.len());
        // pane_id でソートして比較（順序差は許容）
        let mut a: Vec<(u32, u16, u16, u16, u16)> = layout_a.iter()
            .map(|r| (r.pane_id, r.col_off, r.row_off, r.cols, r.rows)).collect();
        let mut b: Vec<(u32, u16, u16, u16, u16)> = layout_b.iter()
            .map(|r| (r.pane_id, r.col_off, r.row_off, r.cols, r.rows)).collect();
        a.sort();
        b.sort();
        prop_assert_eq!(a, b);
    }

    /// タイリングレイアウトの不変条件
    /// - ペイン数 = 入力 ID 数
    /// - 全矩形が画面内
    /// - 各矩形の cols > 0, rows > 0（十分な領域がある場合）
    /// - pane_id が入力と一致
    #[test]
    fn tiling_invariants_hold(
        ids in proptest::collection::vec(1u32..=u32::MAX, 1..12),
    ) {
        // 入力 ID から重複を除く（compute_tiling_layouts は ID 重複を想定しない）
        let mut unique_ids = ids.clone();
        unique_ids.sort();
        unique_ids.dedup();

        let n = unique_ids.len() as u16;
        // タイリングは sqrt(N) × sqrt(N) のグリッド配置のため、
        // 各方向に最小 sqrt(N) * 2 の領域が必要
        let cols = (n * 4).max(20);
        let rows = (n * 4).max(20);

        let layout = compute_tiling_layouts(&unique_ids, cols, rows);
        prop_assert_eq!(layout.len(), unique_ids.len());

        for r in &layout {
            prop_assert!(r.cols > 0, "cols = 0 は不正: {:?}", r);
            prop_assert!(r.rows > 0, "rows = 0 は不正: {:?}", r);
            prop_assert!(r.col_off + r.cols <= cols,
                "tiling 右端超過: {:?} window={}", r, cols);
            prop_assert!(r.row_off + r.rows <= rows,
                "tiling 下端超過: {:?} window={}", r, rows);
        }

        // 入力 ID 集合と一致
        let mut layout_ids: Vec<u32> = layout.iter().map(|r| r.pane_id).collect();
        layout_ids.sort();
        prop_assert_eq!(layout_ids, unique_ids);
    }

    /// `compute_tiling_layouts` がパニックしないこと（任意の cols/rows でも）
    #[test]
    fn tiling_compute_never_panics(
        ids in proptest::collection::vec(1u32..=u32::MAX, 0..16),
        cols in 0u16..=200,
        rows in 0u16..=80,
    ) {
        let _ = compute_tiling_layouts(&ids, cols, rows);
    }
}

// ---- compute_reordered（Sprint 5-7 / Phase 2-3）テスト ----

use super::compute_reordered;
use std::collections::HashSet;

fn known_set(ids: &[u32]) -> HashSet<u32> {
    ids.iter().copied().collect()
}

#[test]
fn reorder_完全な順列を反映する() {
    let current = vec![1, 2, 3, 4];
    let known = known_set(&current);
    let requested = vec![3, 1, 4, 2];
    let next = compute_reordered(&current, &requested, &known);
    assert_eq!(next, vec![3, 1, 4, 2]);
}

#[test]
fn reorder_未指定idは元の相対順で末尾補完される() {
    let current = vec![10, 20, 30, 40];
    let known = known_set(&current);
    // 30 と 10 だけ指定。残り 20, 40 は元の相対順で末尾に
    let requested = vec![30, 10];
    let next = compute_reordered(&current, &requested, &known);
    assert_eq!(next, vec![30, 10, 20, 40]);
}

#[test]
fn reorder_未知idは無視される() {
    let current = vec![1, 2, 3];
    let known = known_set(&current);
    // 99 は未知 → 無視。1, 3 だけ採用、2 が末尾補完
    let requested = vec![99, 3, 99, 1];
    let next = compute_reordered(&current, &requested, &known);
    assert_eq!(next, vec![3, 1, 2]);
}

#[test]
fn reorder_重複指定は最初の出現のみ採用() {
    let current = vec![1, 2, 3];
    let known = known_set(&current);
    let requested = vec![2, 2, 1, 3, 1];
    let next = compute_reordered(&current, &requested, &known);
    assert_eq!(next, vec![2, 1, 3]);
}

#[test]
fn reorder_空指定は元の順序を保つ() {
    let current = vec![5, 6, 7];
    let known = known_set(&current);
    let next = compute_reordered(&current, &[], &known);
    assert_eq!(next, vec![5, 6, 7]);
}

#[test]
fn reorder_currentにない既知idは末尾に昇順で追加() {
    // 何らかのバグで pane_order に登録漏れの既知ペインがあったケース
    let current = vec![1, 3];
    let mut known = known_set(&current);
    known.insert(5);
    known.insert(2);
    let requested = vec![3, 1];
    let next = compute_reordered(&current, &requested, &known);
    // 3, 1 → current から漏れていた 5, 2 を昇順で末尾追加
    assert_eq!(next, vec![3, 1, 2, 5]);
}
