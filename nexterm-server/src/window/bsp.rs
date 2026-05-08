//! BSP（バイナリ空間分割）ツリー実装
//!
//! ウィンドウ内のペイン分割レイアウトを管理するBSPツリーの実装。

use crate::snapshot::{SplitDirSnapshot, SplitNodeSnapshot};

/// ペイン分割方向
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SplitDir {
    /// 左右分割（垂直境界線）
    Vertical,
    /// 上下分割（水平境界線）
    Horizontal,
}

/// サーバー内部でのペイン矩形（グリッド座標）
#[derive(Clone, Debug)]
pub struct PaneRect {
    pub pane_id: u32,
    pub col_off: u16,
    pub row_off: u16,
    pub cols: u16,
    pub rows: u16,
}

/// remove() の結果
pub(super) enum RemoveResult {
    /// 自分自身が削除対象（呼び出し元が兄弟に置換する）
    RemoveSelf,
    /// 子孫の削除が完了した
    Removed,
    /// ターゲットが見つからなかった
    NotFound,
}

/// BSP 分割ツリーのノード
#[derive(Clone, Debug)]
pub(super) enum SplitNode {
    /// 末端ノード（単一ペイン）
    Pane { pane_id: u32 },
    /// 分割ノード（左/上 と 右/下 の子を持つ）
    Split {
        dir: SplitDir,
        /// 左/上の占有割合（0.0〜1.0）
        ratio: f32,
        left: Box<SplitNode>,
        right: Box<SplitNode>,
    },
}

impl SplitNode {
    /// 指定ペインを分割して新ペインを右/下に挿入する
    pub(super) fn insert_after(&mut self, target_id: u32, new_id: u32, dir: SplitDir) -> bool {
        match self {
            SplitNode::Pane { pane_id } if *pane_id == target_id => {
                let old = std::mem::replace(self, SplitNode::Pane { pane_id: 0 });
                *self = SplitNode::Split {
                    dir,
                    ratio: 0.5,
                    left: Box::new(old),
                    right: Box::new(SplitNode::Pane { pane_id: new_id }),
                };
                true
            }
            SplitNode::Pane { .. } => false,
            SplitNode::Split { left, right, .. } => {
                left.insert_after(target_id, new_id, dir.clone())
                    || right.insert_after(target_id, new_id, dir)
            }
        }
    }

    /// 矩形 (col_off, row_off, cols, rows) を再帰的に計算して out に追加する
    pub(super) fn compute(
        &self,
        col_off: u16,
        row_off: u16,
        cols: u16,
        rows: u16,
        out: &mut Vec<PaneRect>,
    ) {
        match self {
            SplitNode::Pane { pane_id } => {
                out.push(PaneRect {
                    pane_id: *pane_id,
                    col_off,
                    row_off,
                    cols,
                    rows,
                });
            }
            SplitNode::Split {
                dir,
                ratio,
                left,
                right,
            } => match dir {
                SplitDir::Vertical => {
                    // 左右分割（境界線1列分を差し引く）
                    let left_cols = ((cols as f32 * ratio) as u16)
                        .max(1)
                        .min(cols.saturating_sub(2));
                    let right_cols = cols.saturating_sub(left_cols + 1).max(1);
                    left.compute(col_off, row_off, left_cols, rows, out);
                    right.compute(col_off + left_cols + 1, row_off, right_cols, rows, out);
                }
                SplitDir::Horizontal => {
                    // 上下分割（境界線1行分を差し引く）
                    let top_rows = ((rows as f32 * ratio) as u16)
                        .max(1)
                        .min(rows.saturating_sub(2));
                    let bot_rows = rows.saturating_sub(top_rows + 1).max(1);
                    left.compute(col_off, row_off, cols, top_rows, out);
                    right.compute(col_off, row_off + top_rows + 1, cols, bot_rows, out);
                }
            },
        }
    }

    /// 指定ペインを BSP ツリーから削除し、兄弟ノードを親に昇格させる。
    /// 削除に成功した場合は `Some(self_after_removal)` を返す。
    /// `None` は「自分自身が削除対象だった」ことを示す（呼び出し元で兄弟に置換する）。
    pub(super) fn remove(&mut self, target_id: u32) -> RemoveResult {
        match self {
            SplitNode::Pane { pane_id } if *pane_id == target_id => RemoveResult::RemoveSelf,
            SplitNode::Pane { .. } => RemoveResult::NotFound,
            SplitNode::Split { left, right, .. } => {
                match left.remove(target_id) {
                    RemoveResult::RemoveSelf => {
                        // 左を削除 → 右を自分の場所に昇格させる
                        let sibling =
                            std::mem::replace(right.as_mut(), SplitNode::Pane { pane_id: 0 });
                        *self = sibling;
                        RemoveResult::Removed
                    }
                    RemoveResult::Removed => RemoveResult::Removed,
                    RemoveResult::NotFound => match right.remove(target_id) {
                        RemoveResult::RemoveSelf => {
                            // 右を削除 → 左を自分の場所に昇格させる
                            let sibling =
                                std::mem::replace(left.as_mut(), SplitNode::Pane { pane_id: 0 });
                            *self = sibling;
                            RemoveResult::Removed
                        }
                        other => other,
                    },
                }
            }
        }
    }

    /// フォーカスペインに最も近い Split ノードの ratio を delta だけ変更する。
    /// delta > 0 でフォーカスペインを広げ、delta < 0 で縮める。
    pub(super) fn adjust_ratio_for(&mut self, target_id: u32, delta: f32) -> bool {
        match self {
            SplitNode::Pane { .. } => false,
            SplitNode::Split {
                ratio, left, right, ..
            } => {
                let in_left = left.contains(target_id);
                let in_right = right.contains(target_id);
                if in_left || in_right {
                    let new_ratio = if in_left {
                        (*ratio + delta).clamp(0.1, 0.9)
                    } else {
                        (*ratio - delta).clamp(0.1, 0.9)
                    };
                    *ratio = new_ratio;
                    true
                } else {
                    left.adjust_ratio_for(target_id, delta)
                        || right.adjust_ratio_for(target_id, delta)
                }
            }
        }
    }

    /// BSP ツリー内の 2 つのペイン ID を入れ替える
    pub(super) fn swap_ids(&mut self, id_a: u32, id_b: u32) -> bool {
        match self {
            SplitNode::Pane { pane_id } => {
                if *pane_id == id_a {
                    *pane_id = id_b;
                    true
                } else if *pane_id == id_b {
                    *pane_id = id_a;
                    true
                } else {
                    false
                }
            }
            SplitNode::Split { left, right, .. } => {
                left.swap_ids(id_a, id_b) | right.swap_ids(id_a, id_b)
            }
        }
    }

    /// 隣接するペイン ID を返す（フォーカスペインの兄弟ノード）
    #[allow(dead_code)]
    pub(super) fn neighbor_id(&self, target_id: u32) -> Option<u32> {
        match self {
            SplitNode::Pane { .. } => None,
            SplitNode::Split { left, right, .. } => {
                if left.contains(target_id) {
                    right.first_pane_id()
                } else if right.contains(target_id) {
                    left.first_pane_id()
                } else {
                    left.neighbor_id(target_id)
                        .or_else(|| right.neighbor_id(target_id))
                }
            }
        }
    }

    /// サブツリー内の最初のペイン ID を返す
    #[allow(dead_code)]
    pub(super) fn first_pane_id(&self) -> Option<u32> {
        match self {
            SplitNode::Pane { pane_id } => Some(*pane_id),
            SplitNode::Split { left, .. } => left.first_pane_id(),
        }
    }

    /// 指定ペインがこのサブツリーに含まれるか確認する
    pub(super) fn contains(&self, target_id: u32) -> bool {
        match self {
            SplitNode::Pane { pane_id } => *pane_id == target_id,
            SplitNode::Split { left, right, .. } => {
                left.contains(target_id) || right.contains(target_id)
            }
        }
    }

    /// BSP ツリーをスナップショットに変換する（CWD は Window::to_snapshot() で後から填入）
    pub(super) fn to_snapshot(&self) -> SplitNodeSnapshot {
        match self {
            SplitNode::Pane { pane_id } => SplitNodeSnapshot::Pane {
                pane_id: *pane_id,
                cwd: None,
            },
            SplitNode::Split {
                dir,
                ratio,
                left,
                right,
            } => SplitNodeSnapshot::Split {
                dir: match dir {
                    SplitDir::Vertical => SplitDirSnapshot::Vertical,
                    SplitDir::Horizontal => SplitDirSnapshot::Horizontal,
                },
                ratio: *ratio,
                left: Box::new(left.to_snapshot()),
                right: Box::new(right.to_snapshot()),
            },
        }
    }

    /// スナップショットから BSP ツリーを再構築する
    pub(super) fn from_snapshot(snap: &SplitNodeSnapshot) -> Self {
        match snap {
            SplitNodeSnapshot::Pane { pane_id, .. } => SplitNode::Pane { pane_id: *pane_id },
            SplitNodeSnapshot::Split {
                dir,
                ratio,
                left,
                right,
            } => SplitNode::Split {
                dir: match dir {
                    SplitDirSnapshot::Vertical => SplitDir::Vertical,
                    SplitDirSnapshot::Horizontal => SplitDir::Horizontal,
                },
                ratio: *ratio,
                left: Box::new(SplitNode::from_snapshot(left)),
                right: Box::new(SplitNode::from_snapshot(right)),
            },
        }
    }
}
