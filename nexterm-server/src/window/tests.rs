use super::tiling::compute_tiling_layouts;
use super::*;
use crate::snapshot::*;

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
        left: Box::new(SplitNodeSnapshot::Pane { pane_id: 1, cwd: None }),
        right: Box::new(SplitNodeSnapshot::Pane { pane_id: 2, cwd: None }),
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
