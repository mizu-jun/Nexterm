//! タブドラッグのドロップ先判定（Sprint 5-8 Phase 4-2）
//!
//! タブドラッグのドロップ位置（screen 座標、ピクセル）と全 OS Window の bounds から、
//! ドロップ先（同一 Window / 別 Window タブバー / 新規 Window）を決定する純関数。
//!
//! - `compute_drop_target` 自体は副作用なしの純関数で、winit には直接依存しない
//!   ジェネリック設計（`Id: Copy + Eq`）。テストで `u32` を直接渡せるようにする
//!   ため。実運用では `winit::window::WindowId` を `Id` として渡す
//! - `OsWindowBounds` は呼び出し側（`event_handler/mouse.rs`）が各
//!   `ClientWindow.window.outer_position()` / `outer_size()` から構築する
//! - Phase 4-4 で OtherWindowTabBar 経路を `MovePaneToWindow` IPC に接続予定

/// OS Window の外接矩形（screen 座標、ピクセル）
///
/// `position` は `winit::Window::outer_position()` から取得した左上スクリーン座標。
/// `size` は `winit::Window::outer_size()` から取得した外接サイズ。
/// `tab_bar_y_range` は **ウィンドウクライアント領域内のローカル座標**（先頭〜末尾、
/// ピクセル）。タブバー無効時は `(0.0, 0.0)` を渡せばタブバー hit 判定が無効になる。
///
/// Sprint 5-8 Phase 4-2 ではドロップ判定の純関数だけを先行整備するため
/// `#[allow(dead_code)]` を付ける。実利用は Step 2.5（`on_mouse_left_released` 配線）から。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsWindowBounds<Id> {
    pub window_id: Id,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub tab_bar_y_range: (f32, f32),
}

/// ドロップ先判定の結果
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropTarget<Id> {
    /// 同一 OS Window 内にドロップされた（既存の `ReorderPanes` 経路で処理）
    SameWindow { window_id: Id },
    /// 別 OS Window のタブバー上にドロップされた
    /// （Phase 4-4 で `MovePaneToWindow` を発火、現状はログのみ）
    OtherWindowTabBar { window_id: Id },
    /// どの OS Window の bounds にも入らない位置にドロップされた
    /// → 新規 OS Window 生成（`spawn_os_window`）
    NewWindow,
}

/// ドロップ位置から **どの OS Window** にドロップされたかを判定する純関数。
///
/// 判定ルール（先頭から順に評価）:
/// 1. screen pos が **source_window_id** の Window の **タブバー領域** に hit
///    → `SameWindow`（呼び出し側で既存の `ReorderPanes` を発火）
/// 2. screen pos が **他の** Window の **タブバー領域** に hit
///    → `OtherWindowTabBar`（Phase 4-4 で `MovePaneToWindow` を発火）
/// 3. screen pos が **source_window_id** の Window 内だが **タブバー外**
///    → `SameWindow`（呼び出し側で何もしない: ペイン領域へのドロップは reorder 不発）
/// 4. screen pos が **他の** Window 内だが **タブバー外**
///    → `OtherWindowTabBar` 扱い（明示的タブバー領域以外への merge は不発、
///    Phase 4-4 で要件確定）
/// 5. どの Window の bounds にも hit しない
///    → `NewWindow`（新規 OS Window 生成）
///
/// **設計判断**: 別 Window のペイン領域へのドロップを `NewWindow` ではなく
/// `OtherWindowTabBar` にしているのは、「画面上に表示されている別 Window 内に
/// ドロップしたユーザー意図 = 何らかの統合」と解釈する方が自然なため。
/// Phase 4-4 でこのケースを「タブバーへのドロップのみを merge とみなして無視」
/// に絞るか「ペイン分割としての merge」を追加するかは別途検討する。
#[allow(dead_code)]
pub fn compute_drop_target<Id>(
    drop_pos: (i32, i32),
    source_window_id: Id,
    os_windows: &[OsWindowBounds<Id>],
) -> DropTarget<Id>
where
    Id: Copy + Eq,
{
    let (px, py) = drop_pos;
    for w in os_windows {
        let (wx, wy) = w.position;
        let (ww, wh) = w.size;
        let in_bounds = px >= wx && px < wx + ww as i32 && py >= wy && py < wy + wh as i32;
        if !in_bounds {
            continue;
        }
        let local_y = (py - wy) as f32;
        let (tab_y0, tab_y1) = w.tab_bar_y_range;
        let tab_bar_enabled = tab_y1 > tab_y0;
        let on_tab_bar = tab_bar_enabled && local_y >= tab_y0 && local_y < tab_y1;

        if w.window_id == source_window_id {
            return DropTarget::SameWindow {
                window_id: w.window_id,
            };
        } else if on_tab_bar {
            return DropTarget::OtherWindowTabBar {
                window_id: w.window_id,
            };
        } else {
            // 別 Window 内だがタブバー外: Phase 4-4 で要件確定（merge 不発の暫定扱い）
            return DropTarget::OtherWindowTabBar {
                window_id: w.window_id,
            };
        }
    }
    DropTarget::NewWindow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(id: u32, x: i32, y: i32, w: u32, h: u32) -> OsWindowBounds<u32> {
        OsWindowBounds {
            window_id: id,
            position: (x, y),
            size: (w, h),
            tab_bar_y_range: (0.0, 32.0),
        }
    }

    #[test]
    fn 同一window_タブバー上にドロップ() {
        // タブバーは local_y 0..32 = screen y 100..132
        let windows = [bounds(1, 100, 100, 800, 600)];
        let result = compute_drop_target((200, 110), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }

    #[test]
    fn 同一window_ペイン領域にドロップ() {
        let windows = [bounds(1, 100, 100, 800, 600)];
        // y=200 はタブバー外 → 同一 Window のペイン領域扱い
        let result = compute_drop_target((200, 200), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }

    #[test]
    fn 別window_タブバー上にドロップ() {
        let windows = [
            bounds(1, 100, 100, 800, 600),
            bounds(2, 1000, 100, 800, 600),
        ];
        let result = compute_drop_target((1200, 110), 1, &windows);
        assert_eq!(result, DropTarget::OtherWindowTabBar { window_id: 2 });
    }

    #[test]
    fn どのwindowにもhitしない場合は新規window() {
        let windows = [bounds(1, 100, 100, 800, 600)];
        let result = compute_drop_target((2000, 2000), 1, &windows);
        assert_eq!(result, DropTarget::NewWindow);
    }

    #[test]
    fn os_windows空のときは新規window() {
        let windows: [OsWindowBounds<u32>; 0] = [];
        let result = compute_drop_target((100, 100), 1, &windows);
        assert_eq!(result, DropTarget::NewWindow);
    }

    #[test]
    fn 左上隅は内側判定() {
        let windows = [bounds(1, 100, 100, 800, 600)];
        // (100, 100) は左上隅、px >= wx && py >= wy なので in_bounds
        let result = compute_drop_target((100, 100), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }

    #[test]
    fn 右下隅は外側判定() {
        let windows = [bounds(1, 100, 100, 800, 600)];
        // (900, 700) は外接 right=100+800=900, bottom=100+600=700、px < wx+ww (900<900) false
        let result = compute_drop_target((900, 700), 1, &windows);
        assert_eq!(result, DropTarget::NewWindow);
    }

    #[test]
    fn タブバー無効時はペイン領域扱い() {
        let windows = [OsWindowBounds {
            window_id: 1u32,
            position: (100, 100),
            size: (800, 600),
            tab_bar_y_range: (0.0, 0.0), // タブバー無効
        }];
        // タブバー無効でも in_bounds なら同一 Window 扱い（reorder 不発、no-op）
        let result = compute_drop_target((200, 110), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }

    #[test]
    fn 別window_ペイン領域は暫定other_window_tab_bar扱い() {
        // Phase 4-4 で merge 仕様確定時に見直す
        let windows = [
            bounds(1, 100, 100, 800, 600),
            bounds(2, 1000, 100, 800, 600),
        ];
        let result = compute_drop_target((1200, 200), 1, &windows);
        assert_eq!(result, DropTarget::OtherWindowTabBar { window_id: 2 });
    }

    #[test]
    fn 重なるwindowは先頭が勝つ() {
        // Vec の先頭から評価するため、重複領域は先頭の Window が hit する
        let windows = [
            bounds(1, 100, 100, 800, 600),
            bounds(2, 100, 100, 800, 600), // 完全に重なる
        ];
        let result = compute_drop_target((200, 110), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }
}
