//! ウィンドウ — ペインの集合（タブ相当）
//!
//! BSP（Binary Space Partition）ツリーでペインの分割レイアウトを管理する。
//! 各ペインの (col_offset, row_offset, cols, rows) はツリーの再帰計算で決まる。

use std::collections::HashMap;

use anyhow::Result;
use tokio::sync::mpsc;

use nexterm_proto::{PaneLayout, ServerToClient};

use crate::pane::Pane;
use crate::snapshot::{SplitDirSnapshot, SplitNodeSnapshot, WindowSnapshot};

// ---- 分割方向 ----

/// ペイン分割方向
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SplitDir {
    /// 左右分割（垂直境界線）
    Vertical,
    /// 上下分割（水平境界線）
    Horizontal,
}

// ---- BSP ツリー ----

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
enum RemoveResult {
    /// 自分自身が削除対象（呼び出し元が兄弟に置換する）
    RemoveSelf,
    /// 子孫の削除が完了した
    Removed,
    /// ターゲットが見つからなかった
    NotFound,
}

/// BSP 分割ツリーのノード
#[derive(Clone, Debug)]
enum SplitNode {
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
    fn insert_after(&mut self, target_id: u32, new_id: u32, dir: SplitDir) -> bool {
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
    fn compute(&self, col_off: u16, row_off: u16, cols: u16, rows: u16, out: &mut Vec<PaneRect>) {
        match self {
            SplitNode::Pane { pane_id } => {
                out.push(PaneRect { pane_id: *pane_id, col_off, row_off, cols, rows });
            }
            SplitNode::Split { dir, ratio, left, right } => match dir {
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
    fn remove(&mut self, target_id: u32) -> RemoveResult {
        match self {
            SplitNode::Pane { pane_id } if *pane_id == target_id => RemoveResult::RemoveSelf,
            SplitNode::Pane { .. } => RemoveResult::NotFound,
            SplitNode::Split { left, right, .. } => {
                match left.remove(target_id) {
                    RemoveResult::RemoveSelf => {
                        // 左を削除 → 右を自分の場所に昇格させる
                        let sibling = std::mem::replace(right.as_mut(), SplitNode::Pane { pane_id: 0 });
                        *self = sibling;
                        RemoveResult::Removed
                    }
                    RemoveResult::Removed => RemoveResult::Removed,
                    RemoveResult::NotFound => match right.remove(target_id) {
                        RemoveResult::RemoveSelf => {
                            // 右を削除 → 左を自分の場所に昇格させる
                            let sibling = std::mem::replace(left.as_mut(), SplitNode::Pane { pane_id: 0 });
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
    fn adjust_ratio_for(&mut self, target_id: u32, delta: f32) -> bool {
        match self {
            SplitNode::Pane { .. } => false,
            SplitNode::Split { ratio, left, right, .. } => {
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
                    left.adjust_ratio_for(target_id, delta) || right.adjust_ratio_for(target_id, delta)
                }
            }
        }
    }

    /// 指定ペインがこのサブツリーに含まれるか確認する
    fn contains(&self, target_id: u32) -> bool {
        match self {
            SplitNode::Pane { pane_id } => *pane_id == target_id,
            SplitNode::Split { left, right, .. } => left.contains(target_id) || right.contains(target_id),
        }
    }

    /// BSP ツリーをスナップショットに変換する（CWD は Window::to_snapshot() で後から填入）
    fn to_snapshot(&self) -> SplitNodeSnapshot {
        match self {
            SplitNode::Pane { pane_id } => SplitNodeSnapshot::Pane {
                pane_id: *pane_id,
                cwd: None,
            },
            SplitNode::Split { dir, ratio, left, right } => SplitNodeSnapshot::Split {
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
    fn from_snapshot(snap: &SplitNodeSnapshot) -> Self {
        match snap {
            SplitNodeSnapshot::Pane { pane_id, .. } => SplitNode::Pane { pane_id: *pane_id },
            SplitNodeSnapshot::Split { dir, ratio, left, right } => SplitNode::Split {
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

// ---- ウィンドウ ----

/// ウィンドウ（ペインのコンテナ）
pub struct Window {
    pub id: u32,
    pub name: String,
    /// ペインの Map（ID → Pane）
    panes: HashMap<u32, Pane>,
    /// 現在フォーカス中のペイン ID
    focused_pane_id: u32,
    /// BSP 分割ツリー
    layout: SplitNode,
}

impl Window {
    /// 最初のペインを持つウィンドウを生成する
    pub fn new(
        id: u32,
        name: String,
        cols: u16,
        rows: u16,
        tx: mpsc::Sender<ServerToClient>,
        shell: &str,
    ) -> Result<Self> {
        let pane = Pane::spawn(cols, rows, tx, shell)?;
        let focused_pane_id = pane.id;
        let layout = SplitNode::Pane { pane_id: focused_pane_id };
        let mut panes = HashMap::new();
        panes.insert(pane.id, pane);

        Ok(Self { id, name, panes, focused_pane_id, layout })
    }

    /// フォーカス中のペイン ID を返す
    pub fn focused_pane_id(&self) -> u32 {
        self.focused_pane_id
    }

    /// ペイン一覧の ID を返す
    pub fn pane_ids(&self) -> Vec<u32> {
        self.panes.keys().copied().collect()
    }

    /// 新しいペインを BSP ツリーで分割して追加する
    ///
    /// `total_cols`/`total_rows` はウィンドウ全体のサイズ。
    /// 分割後の各ペインサイズを計算してから spawn する。
    pub fn add_pane(
        &mut self,
        total_cols: u16,
        total_rows: u16,
        tx: mpsc::Sender<ServerToClient>,
        shell: &str,
        dir: SplitDir,
    ) -> Result<u32> {
        // 1. 新 ID を事前発行してツリーに挿入する
        let new_id = crate::pane::new_pane_id();
        self.layout.insert_after(self.focused_pane_id, new_id, dir);

        // 2. 新レイアウトを計算して新ペインのサイズを取得する
        let layouts = self.compute_layouts(total_cols, total_rows);
        let new_rect = layouts
            .iter()
            .find(|r| r.pane_id == new_id)
            .cloned()
            .unwrap_or(PaneRect {
                pane_id: new_id,
                col_off: 0,
                row_off: 0,
                cols: total_cols,
                rows: total_rows,
            });

        // 3. 計算済みサイズで新ペインを spawn する
        let pane = Pane::spawn_with_id(new_id, new_rect.cols, new_rect.rows, tx, shell)?;
        self.panes.insert(new_id, pane);
        self.focused_pane_id = new_id;

        // 4. 既存ペインを新しいサイズにリサイズする
        for rect in &layouts {
            if rect.pane_id != new_id {
                if let Some(p) = self.panes.get_mut(&rect.pane_id) {
                    let _ = p.resize_pty(rect.cols, rect.rows);
                }
            }
        }

        Ok(new_id)
    }

    /// 全ペインのレイアウトを計算する
    pub fn compute_layouts(&self, cols: u16, rows: u16) -> Vec<PaneRect> {
        let mut out = Vec::new();
        self.layout.compute(0, 0, cols, rows, &mut out);
        out
    }

    /// LayoutChanged メッセージを生成する（IPC 送信用）
    pub fn layout_changed_msg(&self, cols: u16, rows: u16) -> ServerToClient {
        let rects = self.compute_layouts(cols, rows);
        ServerToClient::LayoutChanged {
            panes: rects
                .into_iter()
                .map(|r| PaneLayout {
                    pane_id: r.pane_id,
                    col_offset: r.col_off,
                    row_offset: r.row_off,
                    cols: r.cols,
                    rows: r.rows,
                    is_focused: r.pane_id == self.focused_pane_id,
                })
                .collect(),
            focused_pane_id: self.focused_pane_id,
        }
    }

    /// 指定ペインにフォーカスを移動する（クリック等）
    pub fn set_focused_pane(&mut self, pane_id: u32) {
        if self.panes.contains_key(&pane_id) {
            self.focused_pane_id = pane_id;
        }
    }

    /// 次のペインにフォーカスを移動する
    pub fn focus_next(&mut self) {
        let ids: Vec<u32> = {
            let mut v: Vec<u32> = self.panes.keys().copied().collect();
            v.sort();
            v
        };
        if let Some(pos) = ids.iter().position(|&id| id == self.focused_pane_id) {
            self.focused_pane_id = ids[(pos + 1) % ids.len()];
        }
    }

    /// 前のペインにフォーカスを移動する
    pub fn focus_prev(&mut self) {
        let ids: Vec<u32> = {
            let mut v: Vec<u32> = self.panes.keys().copied().collect();
            v.sort();
            v
        };
        if let Some(pos) = ids.iter().position(|&id| id == self.focused_pane_id) {
            let prev = if pos == 0 { ids.len() - 1 } else { pos - 1 };
            self.focused_pane_id = ids[prev];
        }
    }

    /// 指定ペインへの参照を返す
    pub fn pane(&self, id: u32) -> Option<&Pane> {
        self.panes.get(&id)
    }

    /// フォーカスペインを BSP ツリーから削除する。
    ///
    /// ペインが 1 つしかない場合は `Err` を返す（最後のペインは削除不可）。
    /// 成功した場合は削除されたペインの隣のペインにフォーカスが移る。
    pub fn remove_focused_pane(&mut self, cols: u16, rows: u16) -> Result<u32> {
        if self.panes.len() <= 1 {
            return Err(anyhow::anyhow!("最後のペインは削除できません"));
        }
        let target_id = self.focused_pane_id;

        // BSP ツリーから削除する
        self.layout.remove(target_id);

        // ペイン Map から削除する
        self.panes.remove(&target_id);

        // 残ったペインにフォーカスを移す（ID が最も小さいものを選ぶ）
        let next_id = self.panes.keys().copied().min().unwrap();
        self.focused_pane_id = next_id;

        // 残ったペインをリサイズする
        let layouts = self.compute_layouts(cols, rows);
        for rect in &layouts {
            if let Some(pane) = self.panes.get_mut(&rect.pane_id) {
                let _ = pane.resize_pty(rect.cols, rect.rows);
            }
        }

        Ok(target_id)
    }

    /// フォーカスペインに最も近い分割の比率を変更する。
    pub fn adjust_split_ratio(&mut self, delta: f32, cols: u16, rows: u16) {
        if self.layout.adjust_ratio_for(self.focused_pane_id, delta) {
            let layouts = self.compute_layouts(cols, rows);
            for rect in &layouts {
                if let Some(pane) = self.panes.get_mut(&rect.pane_id) {
                    let _ = pane.resize_pty(rect.cols, rect.rows);
                }
            }
        }
    }

    /// ペイン数を返す
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// フォーカス中のペインに入力データを書き込む
    pub fn write_to_focused(&self, data: &[u8]) -> Result<()> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("フォーカスペインが見つかりません"))?;
        pane.write_input(data)
    }

    /// フォーカス中のペインのみをリサイズする（後方互換・単一ペイン用）
    pub fn resize_focused(&mut self, cols: u16, rows: u16) -> Result<()> {
        let pane = self
            .panes
            .get_mut(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("フォーカスペインが見つかりません"))?;
        pane.resize_pty(cols, rows)
    }

    /// 全ペインを新しいトータルサイズに従ってリサイズする
    pub fn resize_all_panes(&mut self, cols: u16, rows: u16) {
        let layouts = self.compute_layouts(cols, rows);
        for rect in &layouts {
            if let Some(pane) = self.panes.get_mut(&rect.pane_id) {
                let _ = pane.resize_pty(rect.cols, rect.rows);
            }
        }
    }

    /// 全ペインの PTY 出力チャネルを差し替える（クライアント再アタッチ時）
    pub fn update_tx_for_all(&self, tx: &mpsc::Sender<ServerToClient>) {
        for pane in self.panes.values() {
            pane.update_tx(tx.clone());
        }
    }

    /// フォーカスペインの録音を開始する（Phase 5-A で完全実装）
    pub fn start_recording(&self, path: &str) -> Result<u32> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("フォーカスペインが見つかりません"))?;
        pane.start_recording(path)?;
        Ok(self.focused_pane_id)
    }

    /// フォーカスペインの録音を停止する（Phase 5-A で完全実装）
    pub fn stop_recording(&self) -> Result<u32> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("フォーカスペインが見つかりません"))?;
        pane.stop_recording()?;
        Ok(self.focused_pane_id)
    }

    // ---- スナップショット ----

    /// ウィンドウをスナップショットに変換する
    pub fn to_snapshot(&self) -> WindowSnapshot {
        let mut layout = self.layout.to_snapshot();
        // 各ペインの作業ディレクトリをスナップショットに填入する
        self.fill_cwd_in_snapshot(&mut layout);
        WindowSnapshot {
            id: self.id,
            name: self.name.clone(),
            focused_pane_id: self.focused_pane_id,
            layout,
        }
    }

    /// スナップショットからウィンドウを復元する
    ///
    /// 各ペインは保存されたシェル・作業ディレクトリで新規 PTY として起動する。
    pub fn restore_from_snapshot(
        snap: &WindowSnapshot,
        tx: &mpsc::Sender<ServerToClient>,
        shell: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Self> {
        // BSP ツリーを再構築する
        let layout = SplitNode::from_snapshot(&snap.layout);

        // 各ペインのサイズを BSP 計算で求めてから PTY を起動する
        let mut size_map = Vec::new();
        compute_pane_sizes(&snap.layout, 0, 0, cols, rows, &mut size_map);

        let mut panes = HashMap::new();
        for (pane_id, pane_cols, pane_rows) in size_map {
            let cwd = find_cwd_in_snapshot(&snap.layout, pane_id);
            let pane = match cwd {
                Some(ref cwd_path) => {
                    Pane::spawn_with_cwd(pane_id, pane_cols, pane_rows, tx.clone(), shell, cwd_path)?
                }
                None => Pane::spawn_with_id(pane_id, pane_cols, pane_rows, tx.clone(), shell)?,
            };
            panes.insert(pane_id, pane);
        }

        Ok(Self {
            id: snap.id,
            name: snap.name.clone(),
            panes,
            focused_pane_id: snap.focused_pane_id,
            layout,
        })
    }

    /// BSP スナップショット内の各ペインに作業ディレクトリを填入する
    fn fill_cwd_in_snapshot(&self, node: &mut SplitNodeSnapshot) {
        match node {
            SplitNodeSnapshot::Pane { pane_id, cwd } => {
                if let Some(pane) = self.panes.get(pane_id) {
                    *cwd = pane.working_dir();
                }
            }
            SplitNodeSnapshot::Split { left, right, .. } => {
                self.fill_cwd_in_snapshot(left);
                self.fill_cwd_in_snapshot(right);
            }
        }
    }
}

/// BSP スナップショットから各ペインのサイズを計算する
fn compute_pane_sizes(
    node: &SplitNodeSnapshot,
    col_off: u16,
    row_off: u16,
    cols: u16,
    rows: u16,
    out: &mut Vec<(u32, u16, u16)>,
) {
    match node {
        SplitNodeSnapshot::Pane { pane_id, .. } => {
            out.push((*pane_id, cols, rows));
        }
        SplitNodeSnapshot::Split { dir, ratio, left, right } => match dir {
            SplitDirSnapshot::Vertical => {
                let lc = ((cols as f32 * ratio) as u16).max(1).min(cols.saturating_sub(2));
                let rc = cols.saturating_sub(lc + 1).max(1);
                compute_pane_sizes(left, col_off, row_off, lc, rows, out);
                compute_pane_sizes(right, col_off + lc + 1, row_off, rc, rows, out);
            }
            SplitDirSnapshot::Horizontal => {
                let lr = ((rows as f32 * ratio) as u16).max(1).min(rows.saturating_sub(2));
                let rr = rows.saturating_sub(lr + 1).max(1);
                compute_pane_sizes(left, col_off, row_off, cols, lr, out);
                compute_pane_sizes(right, col_off, row_off + lr + 1, cols, rr, out);
            }
        },
    }
}

/// BSP スナップショット内の指定ペインの作業ディレクトリを返す
fn find_cwd_in_snapshot(
    node: &SplitNodeSnapshot,
    target_id: u32,
) -> Option<std::path::PathBuf> {
    match node {
        SplitNodeSnapshot::Pane { pane_id, cwd } if *pane_id == target_id => cwd.clone(),
        SplitNodeSnapshot::Pane { .. } => None,
        SplitNodeSnapshot::Split { left, right, .. } => {
            find_cwd_in_snapshot(left, target_id)
                .or_else(|| find_cwd_in_snapshot(right, target_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bsp_垂直分割のレイアウト計算() {
        let mut tree = SplitNode::Pane { pane_id: 1 };
        tree.insert_after(1, 2, SplitDir::Vertical);
        let mut out = Vec::new();
        tree.compute(0, 0, 80, 24, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].pane_id, 1);
        assert_eq!(out[0].col_off, 0);
        assert_eq!(out[1].pane_id, 2);
        assert!(out[1].col_off > 0);
        // 合計列数 + 境界1 = 80
        assert_eq!(out[0].cols + 1 + out[1].cols, 80);
    }

    #[test]
    fn bsp_水平分割のレイアウト計算() {
        let mut tree = SplitNode::Pane { pane_id: 1 };
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
        let mut tree = SplitNode::Pane { pane_id: 1 };
        tree.insert_after(1, 2, SplitDir::Vertical);
        tree.insert_after(2, 3, SplitDir::Horizontal);
        let mut out = Vec::new();
        tree.compute(0, 0, 80, 24, &mut out);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn フォーカス移動の境界値() {
        let ids = vec![10u32, 20, 30];
        let pos = ids.iter().position(|&id| id == 30).unwrap();
        let next = ids[(pos + 1) % ids.len()];
        assert_eq!(next, 10);
        let pos = ids.iter().position(|&id| id == 10).unwrap();
        let prev = if pos == 0 { ids.len() - 1 } else { pos - 1 };
        assert_eq!(ids[prev], 30);
    }

    #[test]
    fn スナップショット変換の往復整合性() {
        let snap_before = SplitNodeSnapshot::Split {
            dir: SplitDirSnapshot::Vertical,
            ratio: 0.5,
            left: Box::new(SplitNodeSnapshot::Pane { pane_id: 1, cwd: None }),
            right: Box::new(SplitNodeSnapshot::Pane { pane_id: 2, cwd: None }),
        };
        // スナップショット → SplitNode → スナップショット の往復を確認する
        let node = SplitNode::from_snapshot(&snap_before);
        let snap_after = node.to_snapshot();
        let mut sizes_before = Vec::new();
        let mut sizes_after = Vec::new();
        compute_pane_sizes(&snap_before, 0, 0, 80, 24, &mut sizes_before);
        compute_pane_sizes(&snap_after, 0, 0, 80, 24, &mut sizes_after);
        assert_eq!(sizes_before, sizes_after);
    }
}
