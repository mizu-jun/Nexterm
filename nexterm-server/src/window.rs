//! ウィンドウ — ペインの集合（タブ相当）
//!
//! BSP（Binary Space Partition）ツリーでペインの分割レイアウトを管理する。
//! 各ペインの (col_offset, row_offset, cols, rows) はツリーの再帰計算で決まる。

use std::collections::HashMap;

use anyhow::Result;
use tokio::sync::broadcast;

use nexterm_proto::{PaneLayout, ServerToClient};

use crate::pane::Pane;
use crate::serial::SerialPane;
use crate::snapshot::{SplitDirSnapshot, SplitNodeSnapshot, WindowSnapshot};

// ---- レイアウトモード ----

/// ウィンドウのレイアウトモード
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum LayoutMode {
    /// BSP（バイナリ空間分割）— 手動分割・比率保持（デフォルト）
    #[default]
    Bsp,
    /// タイリング — ペインを均等グリッドに自動配置
    Tiling,
}

impl LayoutMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "tiling" => LayoutMode::Tiling,
            _ => LayoutMode::Bsp,
        }
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            LayoutMode::Bsp => "bsp",
            LayoutMode::Tiling => "tiling",
        }
    }
}

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

    /// BSP ツリー内の 2 つのペイン ID を入れ替える
    fn swap_ids(&mut self, id_a: u32, id_b: u32) -> bool {
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
    fn neighbor_id(&self, target_id: u32) -> Option<u32> {
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
    fn first_pane_id(&self) -> Option<u32> {
        match self {
            SplitNode::Pane { pane_id } => Some(*pane_id),
            SplitNode::Split { left, .. } => left.first_pane_id(),
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

/// フローティングペインの矩形情報
#[derive(Clone, Debug)]
pub struct FloatRect {
    pub col_off: u16,
    pub row_off: u16,
    pub cols: u16,
    pub rows: u16,
}

/// ウィンドウ（ペインのコンテナ）
pub struct Window {
    pub id: u32,
    pub name: String,
    /// PTY ペインの Map（ID → Pane）
    panes: HashMap<u32, Pane>,
    /// シリアルポートペインの Map（ID → SerialPane）
    serial_panes: HashMap<u32, SerialPane>,
    /// 現在フォーカス中のペイン ID
    focused_pane_id: u32,
    /// BSP 分割ツリー
    layout: SplitNode,
    /// ズーム中か（フォーカスペインがウィンドウ全体を占有する）
    zoomed: bool,
    /// レイアウトモード（Bsp / Tiling）
    pub layout_mode: LayoutMode,
    /// フローティングペイン（通常レイアウトの前面に重なるペイン）
    floating_panes: HashMap<u32, (Pane, FloatRect)>,
}

impl Window {
    /// 最初のペインを持つウィンドウを生成する
    pub fn new(
        id: u32,
        name: String,
        cols: u16,
        rows: u16,
        tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
    ) -> Result<Self> {
        let pane = Pane::spawn(cols, rows, tx, shell, args)?;
        let focused_pane_id = pane.id;
        let layout = SplitNode::Pane { pane_id: focused_pane_id };
        let mut panes = HashMap::new();
        panes.insert(pane.id, pane);

        Ok(Self {
            id,
            name,
            panes,
            serial_panes: HashMap::new(),
            focused_pane_id,
            layout,
            zoomed: false,
            layout_mode: LayoutMode::Bsp,
            floating_panes: HashMap::new(),
        })
    }

    /// 既存のペインを持つウィンドウを生成する（break-pane 用）
    pub fn new_with_pane(id: u32, name: String, pane: Pane) -> Result<Self> {
        let focused_pane_id = pane.id;
        let layout = SplitNode::Pane { pane_id: focused_pane_id };
        let mut panes = HashMap::new();
        panes.insert(pane.id, pane);
        Ok(Self {
            id,
            name,
            panes,
            serial_panes: HashMap::new(),
            focused_pane_id,
            layout,
            zoomed: false,
            layout_mode: LayoutMode::Bsp,
            floating_panes: HashMap::new(),
        })
    }

    /// フォーカス中のペイン ID を返す
    pub fn focused_pane_id(&self) -> u32 {
        self.focused_pane_id
    }

    /// ペイン一覧の ID を返す（PTY + シリアル）
    pub fn pane_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.panes.keys().copied().collect();
        ids.extend(self.serial_panes.keys().copied());
        ids
    }

    /// 新しいペインを BSP ツリーで分割して追加する
    ///
    /// `total_cols`/`total_rows` はウィンドウ全体のサイズ。
    /// 分割後の各ペインサイズを計算してから spawn する。
    pub fn add_pane(
        &mut self,
        total_cols: u16,
        total_rows: u16,
        tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
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
        let pane = Pane::spawn_with_id(new_id, new_rect.cols, new_rect.rows, tx, shell, args)?;
        self.panes.insert(new_id, pane);
        self.focused_pane_id = new_id;

        // 4. 既存ペインを新しいサイズにリサイズする
        for rect in &layouts {
            if rect.pane_id != new_id
                && let Some(p) = self.panes.get_mut(&rect.pane_id) {
                    let _ = p.resize_pty(rect.cols, rect.rows);
                }
        }

        Ok(new_id)
    }

    /// 全ペインのレイアウトを計算する
    pub fn compute_layouts(&self, cols: u16, rows: u16) -> Vec<PaneRect> {
        match self.layout_mode {
            LayoutMode::Bsp => {
                let mut out = Vec::new();
                self.layout.compute(0, 0, cols, rows, &mut out);
                out
            }
            LayoutMode::Tiling => {
                // BSP ツリーからペイン ID を挿入順（ソート済み）で収集する
                let mut pane_ids: Vec<u32> = self.panes.keys().copied().collect();
                pane_ids.extend(self.serial_panes.keys().copied());
                pane_ids.sort();
                compute_tiling_layouts(&pane_ids, cols, rows)
            }
        }
    }

    /// レイアウトモードを変更し、全ペインを新レイアウトにリサイズする
    pub fn set_layout_mode(&mut self, mode: LayoutMode, cols: u16, rows: u16) {
        self.layout_mode = mode;
        self.resize_all_panes(cols, rows);
    }

    // ---- フローティングペイン ----

    /// フローティングペインを生成してウィンドウ中央に配置する
    ///
    /// 返り値: (pane_id, FloatRect) — IPC が FloatingPaneOpened を送信するために使う
    pub fn open_floating_pane(
        &mut self,
        total_cols: u16,
        total_rows: u16,
        tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
    ) -> Result<(u32, FloatRect)> {
        // デフォルトサイズ: ウィンドウの 60%×70%、中央寄せ
        let fp_cols = (total_cols as f32 * 0.6) as u16;
        let fp_rows = (total_rows as f32 * 0.7) as u16;
        let col_off = (total_cols.saturating_sub(fp_cols)) / 2;
        let row_off = (total_rows.saturating_sub(fp_rows)) / 2;

        let pane = Pane::spawn(fp_cols.max(10), fp_rows.max(5), tx, shell, args)?;
        let pane_id = pane.id;
        let rect = FloatRect { col_off, row_off, cols: fp_cols.max(10), rows: fp_rows.max(5) };
        self.floating_panes.insert(pane_id, (pane, rect.clone()));
        Ok((pane_id, rect))
    }

    /// フローティングペインを閉じる
    pub fn close_floating_pane(&mut self, pane_id: u32) -> bool {
        self.floating_panes.remove(&pane_id).is_some()
    }

    /// フローティングペインを移動する
    pub fn move_floating_pane(&mut self, pane_id: u32, col_off: u16, row_off: u16) -> Option<FloatRect> {
        if let Some((_, rect)) = self.floating_panes.get_mut(&pane_id) {
            rect.col_off = col_off;
            rect.row_off = row_off;
            Some(rect.clone())
        } else {
            None
        }
    }

    /// フローティングペインをリサイズする
    pub fn resize_floating_pane(&mut self, pane_id: u32, cols: u16, rows: u16) -> Option<FloatRect> {
        if let Some((pane, rect)) = self.floating_panes.get_mut(&pane_id) {
            rect.cols = cols.max(10);
            rect.rows = rows.max(5);
            let _ = pane.resize_pty(rect.cols, rect.rows);
            Some(rect.clone())
        } else {
            None
        }
    }

    /// フローティングペインに入力を書き込む
    #[allow(dead_code)]
    pub fn write_to_floating(&self, pane_id: u32, data: &[u8]) -> Result<()> {
        if let Some((pane, _)) = self.floating_panes.get(&pane_id) {
            pane.write_input(data)?;
        }
        Ok(())
    }

    /// フローティングペインの一覧（表示用）
    #[allow(dead_code)]
    pub fn floating_pane_rects(&self) -> Vec<(u32, FloatRect)> {
        self.floating_panes
            .iter()
            .map(|(&id, (_, rect))| (id, rect.clone()))
            .collect()
    }

    /// フローティングペインが存在するか確認する
    #[allow(dead_code)]
    pub fn has_floating_pane(&self, pane_id: u32) -> bool {
        self.floating_panes.contains_key(&pane_id)
    }

    /// LayoutChanged メッセージを生成する（IPC 送信用）
    pub fn layout_changed_msg(&self, cols: u16, rows: u16) -> ServerToClient {
        // ズーム中はフォーカスペインのみをウィンドウ全体サイズで返す
        if self.zoomed {
            return ServerToClient::LayoutChanged {
                panes: vec![PaneLayout {
                    pane_id: self.focused_pane_id,
                    col_offset: 0,
                    row_offset: 0,
                    cols,
                    rows,
                    is_focused: true,
                }],
                focused_pane_id: self.focused_pane_id,
            };
        }
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

    /// フォーカスペインのズームをトグルする。
    /// ズーム中はフォーカスペインをウィンドウ全体に拡大し、他のペインは非表示になる。
    /// 戻り値: ズーム後の状態 (true = ズーム中)
    pub fn toggle_zoom(&mut self, cols: u16, rows: u16) -> bool {
        self.zoomed = !self.zoomed;
        // ズーム時はフォーカスペインをウィンドウサイズにリサイズする
        if self.zoomed {
            if let Some(pane) = self.panes.get_mut(&self.focused_pane_id) {
                let _ = pane.resize_pty(cols, rows);
            }
        } else {
            // アンズーム時は全ペインを正規レイアウトに戻す
            self.resize_all_panes(cols, rows);
        }
        self.zoomed
    }

    /// ズーム状態を返す
    #[allow(dead_code)]
    pub fn is_zoomed(&self) -> bool {
        self.zoomed
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

        // ペイン Map から削除する（PTY またはシリアルポート）
        self.panes.remove(&target_id);
        self.serial_panes.remove(&target_id);

        // 残ったペインにフォーカスを移す（ID が最も小さいものを選ぶ）
        let next_id = self
            .panes
            .keys()
            .copied()
            .min()
            .expect("panes は len > 1 ガード済みのため少なくとも1つ残っているはず");
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

    /// フォーカスペインと指定ペインを BSP ツリー内で入れ替える
    pub fn swap_focused_with(&mut self, target_pane_id: u32) {
        let focused = self.focused_pane_id;
        if focused != target_pane_id {
            self.layout.swap_ids(focused, target_pane_id);
        }
    }

    /// フォーカスペインと隣接ペイン（next 方向）を入れ替える
    #[allow(dead_code)]
    pub fn swap_with_next(&mut self) {
        let focused = self.focused_pane_id;
        let ids: Vec<u32> = {
            let mut v: Vec<u32> = self.panes.keys().copied().collect();
            v.sort();
            v
        };
        if let Some(pos) = ids.iter().position(|&id| id == focused) {
            let next_id = ids[(pos + 1) % ids.len()];
            self.layout.swap_ids(focused, next_id);
        }
    }

    /// フォーカスペインと隣接ペイン（prev 方向）を入れ替える
    #[allow(dead_code)]
    pub fn swap_with_prev(&mut self) {
        let focused = self.focused_pane_id;
        let ids: Vec<u32> = {
            let mut v: Vec<u32> = self.panes.keys().copied().collect();
            v.sort();
            v
        };
        if let Some(pos) = ids.iter().position(|&id| id == focused) {
            let prev_id = if pos == 0 { ids[ids.len() - 1] } else { ids[pos - 1] };
            self.layout.swap_ids(focused, prev_id);
        }
    }

    /// フォーカスペインを Pane Map から取り出す（break-pane / join-pane 用）
    ///
    /// ペインが1つしかない場合は `None` を返す（最後のペインは取り出せない）。
    /// BSP ツリーからは削除してフォーカスを隣接ペインに移動する。
    pub fn take_focused_pane(&mut self, cols: u16, rows: u16) -> Option<Pane> {
        if self.panes.len() <= 1 {
            return None;
        }
        let target_id = self.focused_pane_id;
        // BSP ツリーから削除する
        self.layout.remove(target_id);
        // シリアルペインは break-pane できない（PTY のみ対応）
        if self.serial_panes.contains_key(&target_id) {
            return None;
        }
        // ペイン Map から取り出す
        let pane = self.panes.remove(&target_id)?;
        // フォーカスを残ったペインの最小 ID に移す
        let next_id = self
            .panes
            .keys()
            .copied()
            .min()
            .expect("panes は len > 1 ガード済みのため少なくとも1つ残っているはず");
        self.focused_pane_id = next_id;
        self.zoomed = false;
        // 残ったペインをリサイズする
        let layouts = self.compute_layouts(cols, rows);
        for rect in &layouts {
            if let Some(p) = self.panes.get_mut(&rect.pane_id) {
                let _ = p.resize_pty(rect.cols, rect.rows);
            }
        }
        Some(pane)
    }

    /// 外部から持ち込まれたペインをフォーカスペインの後に追加する（join-pane 用）
    pub fn insert_pane(&mut self, pane: Pane, total_cols: u16, total_rows: u16, dir: SplitDir) {
        let new_id = pane.id;
        self.layout.insert_after(self.focused_pane_id, new_id, dir);
        self.panes.insert(new_id, pane);
        self.focused_pane_id = new_id;
        self.zoomed = false;
        // 全ペインをリサイズする
        let layouts = self.compute_layouts(total_cols, total_rows);
        for rect in &layouts {
            if let Some(p) = self.panes.get_mut(&rect.pane_id) {
                let _ = p.resize_pty(rect.cols, rect.rows);
            }
        }
    }

    /// ペイン数を返す（PTY + シリアル）
    pub fn pane_count(&self) -> usize {
        self.panes.len() + self.serial_panes.len()
    }

    /// フォーカス中のペインに入力データを書き込む（PTY またはシリアルポート）
    pub fn write_to_focused(&self, data: &[u8]) -> Result<()> {
        if let Some(pane) = self.panes.get(&self.focused_pane_id) {
            return pane.write_input(data);
        }
        if let Some(sp) = self.serial_panes.get(&self.focused_pane_id) {
            return sp.write_input(data);
        }
        Err(anyhow::anyhow!("フォーカスペインが見つかりません"))
    }

    /// フォーカスペインのブラケットペーストモードが有効かどうかを返す
    pub fn focused_bracketed_paste_mode(&self) -> bool {
        self.panes
            .get(&self.focused_pane_id)
            .map(|p| p.bracketed_paste.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// フォーカスペインのマウスレポーティングモードを返す（0=無効）
    pub fn focused_mouse_mode(&self) -> u8 {
        self.panes
            .get(&self.focused_pane_id)
            .map(|p| p.mouse_mode.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// フォーカス中のペインのみをリサイズする（後方互換・単一ペイン用）
    #[allow(dead_code)]
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
            } else if let Some(sp) = self.serial_panes.get_mut(&rect.pane_id) {
                let _ = sp.resize_pty(rect.cols, rect.rows);
            }
        }
    }

    /// シリアルポートペインを BSP ツリーに追加する
    #[allow(clippy::too_many_arguments)]
    pub fn add_serial_pane(
        &mut self,
        total_cols: u16,
        total_rows: u16,
        tx: broadcast::Sender<ServerToClient>,
        port_name: &str,
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: &str,
        dir: SplitDir,
    ) -> Result<u32> {
        let sp = SerialPane::spawn(port_name, baud_rate, data_bits, stop_bits, parity, total_cols, total_rows, tx)?;
        let new_id = sp.id;
        self.layout.insert_after(self.focused_pane_id, new_id, dir);
        self.serial_panes.insert(new_id, sp);
        self.focused_pane_id = new_id;
        Ok(new_id)
    }

    /// 全ペインの PTY 出力チャネルを差し替える — broadcast では再アタッチ時の差し替えは不要（no-op）
    #[allow(dead_code)]
    pub fn update_tx_for_all(&self, _tx: &broadcast::Sender<ServerToClient>) {
        // broadcast::Sender は共有されるため、クライアント再アタッチ時に差し替え不要
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

    /// フォーカスペインの asciicast 録画を開始する
    pub fn start_asciicast(&self, path: &str) -> Result<u32> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("フォーカスペインが見つかりません"))?;
        pane.start_asciicast(path)?;
        Ok(self.focused_pane_id)
    }

    /// フォーカスペインの asciicast 録画を停止する
    pub fn stop_asciicast(&self) -> Result<u32> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("フォーカスペインが見つかりません"))?;
        pane.stop_asciicast()?;
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
        tx: &broadcast::Sender<ServerToClient>,
        shell: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Self> {
        // BSP ツリーを再構築する
        let layout = SplitNode::from_snapshot(&snap.layout);

        // 各ペインのサイズを BSP 計算で求めてから PTY を起動する
        let mut size_map = Vec::new();
        compute_pane_sizes(&snap.layout, cols, rows, &mut size_map);

        let mut panes = HashMap::new();
        for (pane_id, pane_cols, pane_rows) in size_map {
            let cwd = find_cwd_in_snapshot(&snap.layout, pane_id);
            let pane = match cwd {
                Some(ref cwd_path) => {
                    Pane::spawn_with_cwd(pane_id, pane_cols, pane_rows, tx.clone(), shell, &[], cwd_path)?
                }
                None => Pane::spawn_with_id(pane_id, pane_cols, pane_rows, tx.clone(), shell, &[])?,
            };
            panes.insert(pane_id, pane);
        }

        Ok(Self {
            id: snap.id,
            name: snap.name.clone(),
            panes,
            serial_panes: HashMap::new(),
            focused_pane_id: snap.focused_pane_id,
            layout,
            zoomed: false,
            layout_mode: LayoutMode::Bsp,
            floating_panes: HashMap::new(),
        })
    }

    /// BSP スナップショット内の各ペインに作業ディレクトリを填入する
    fn fill_cwd_in_snapshot(&self, node: &mut SplitNodeSnapshot) {
        match node {
            SplitNodeSnapshot::Pane { pane_id, cwd } => {
                if let Some(pane) = self.panes.get(pane_id) {
                    *cwd = pane.working_dir();
                } else if let Some(sp) = self.serial_panes.get(pane_id) {
                    *cwd = sp.working_dir();
                }
            }
            SplitNodeSnapshot::Split { left, right, .. } => {
                self.fill_cwd_in_snapshot(left);
                self.fill_cwd_in_snapshot(right);
            }
        }
    }
}

/// タイリングレイアウトを計算する（ペインを均等グリッドに自動配置）
///
/// N ペインを ceil(sqrt(N)) 列に均等分配し、各列内でも均等な行高さを割り当てる。
/// 境界線は設けず全スペースをペインに割り当てる。
pub(crate) fn compute_tiling_layouts(pane_ids: &[u32], total_cols: u16, total_rows: u16) -> Vec<PaneRect> {
    let n = pane_ids.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![PaneRect {
            pane_id: pane_ids[0],
            col_off: 0,
            row_off: 0,
            cols: total_cols,
            rows: total_rows,
        }];
    }

    // 列数: ceil(sqrt(n))
    let ncols = ((n as f64).sqrt().ceil() as usize).max(1).min(n);
    // 各列のペイン数: 最初の (n % ncols) 列に 1 つ多く割り当てる
    let base = n / ncols;
    let extra = n % ncols;

    let mut result = Vec::with_capacity(n);
    let mut pane_idx = 0;

    for col_idx in 0..ncols {
        let count_in_col = base + if col_idx < extra { 1 } else { 0 };
        if count_in_col == 0 {
            continue;
        }

        // 列の X 範囲（整数除算で均等に分配）
        let col_off = (col_idx * total_cols as usize / ncols) as u16;
        let next_col_off = ((col_idx + 1) * total_cols as usize / ncols) as u16;
        let col_width = next_col_off.saturating_sub(col_off).max(1);

        // 列内の各行の Y 範囲
        for row_idx in 0..count_in_col {
            let row_off = (row_idx * total_rows as usize / count_in_col) as u16;
            let next_row_off = ((row_idx + 1) * total_rows as usize / count_in_col) as u16;
            let row_height = next_row_off.saturating_sub(row_off).max(1);

            result.push(PaneRect {
                pane_id: pane_ids[pane_idx],
                col_off,
                row_off,
                cols: col_width,
                rows: row_height,
            });
            pane_idx += 1;
            if pane_idx >= n {
                break;
            }
        }
    }

    result
}

/// BSP スナップショットから各ペインのサイズを計算する
fn compute_pane_sizes(
    node: &SplitNodeSnapshot,
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
                compute_pane_sizes(left, lc, rows, out);
                compute_pane_sizes(right, rc, rows, out);
            }
            SplitDirSnapshot::Horizontal => {
                let lr = ((rows as f32 * ratio) as u16).max(1).min(rows.saturating_sub(2));
                let rr = rows.saturating_sub(lr + 1).max(1);
                compute_pane_sizes(left, cols, lr, out);
                compute_pane_sizes(right, cols, rr, out);
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
    fn bsp_4分割のレイアウト計算() {
        let mut tree = SplitNode::Pane { pane_id: 1 };
        tree.insert_after(1, 2, SplitDir::Vertical);
        tree.insert_after(1, 3, SplitDir::Horizontal);
        tree.insert_after(2, 4, SplitDir::Horizontal);
        let mut out = Vec::new();
        tree.compute(0, 0, 80, 24, &mut out);
        assert_eq!(out.len(), 4, "4 ペインが計算されるべき");
        // 全ペインの col オフセットと row オフセットが有効範囲内
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
        // スナップショット → SplitNode → スナップショット の往復を確認する
        let node = SplitNode::from_snapshot(&snap_before);
        let snap_after = node.to_snapshot();
        let mut sizes_before = Vec::new();
        let mut sizes_after = Vec::new();
        compute_pane_sizes(&snap_before, 80, 24, &mut sizes_before);
        compute_pane_sizes(&snap_after, 80, 24, &mut sizes_after);
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
        // ncols=2 なので左右に並ぶ
        assert_eq!(rects[0].col_off, 0);
        assert!(rects[1].col_off > 0);
        assert_eq!(rects[0].rows, 24);
        assert_eq!(rects[1].rows, 24);
    }

    #[test]
    fn tiling_4ペインは2x2グリッド() {
        let rects = compute_tiling_layouts(&[1, 2, 3, 4], 80, 24);
        assert_eq!(rects.len(), 4);
        // ncols=2 なので各列に 2 ペイン
        for r in &rects {
            assert!(r.cols > 0);
            assert!(r.rows > 0);
            assert!(r.col_off < 80);
            assert!(r.row_off < 24);
        }
        // 左2ペインは同じ col_off
        assert_eq!(rects[0].col_off, rects[1].col_off);
        // 右2ペインは同じ col_off
        assert_eq!(rects[2].col_off, rects[3].col_off);
        // 左と右で col_off が異なる
        assert_ne!(rects[0].col_off, rects[2].col_off);
    }

    #[test]
    fn tiling_5ペインは3列グリッド() {
        let rects = compute_tiling_layouts(&[1, 2, 3, 4, 5], 80, 24);
        assert_eq!(rects.len(), 5, "5つのペインが配置されること");
        // ncols=3（ceil(sqrt(5))=3）
        for r in &rects {
            assert!(r.cols > 0, "列幅は0より大きいこと");
            assert!(r.rows > 0, "行高さは0より大きいこと");
        }
    }

    #[test]
    fn tiling_空リストは空を返す() {
        let rects = compute_tiling_layouts(&[], 80, 24);
        assert!(rects.is_empty());
    }

    #[test]
    fn layout_mode_from_str() {
        assert_eq!(LayoutMode::from_str("tiling"), LayoutMode::Tiling);
        assert_eq!(LayoutMode::from_str("bsp"), LayoutMode::Bsp);
        assert_eq!(LayoutMode::from_str("unknown"), LayoutMode::Bsp);
    }
}
