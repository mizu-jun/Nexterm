//! UI アニメーション基盤（Sprint 5-7 / Phase 3-2）
//!
//! タブ切替・ペイン追加等のイベント時刻を記録し、レンダラー側からの
//! `progress(now)` 問い合わせで [0.0, 1.0] の進捗値を返す。
//!
//! 設計上の特徴:
//! - 時刻計算は [`AnimationManager`] が保持し、レンダラーは純粋に問い合わせるだけ
//! - イベント記録は冪等（同じ ID を 2 回登録すると最新時刻で上書き）
//! - duration はコンフィグから取得した実効値（`AnimationsConfig::scaled_duration_ms`）
//!   を渡す。0 ms の場合は常に進捗 1.0 を返す（= 即時反映）
//! - easing 関数は純関数として独立。テスト容易性のため

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// イーズアウト（3 次曲線）。`t ∈ [0, 1]` → `[0, 1]`。
///
/// 開始時に速く動き、終盤で減速する自然な動き。
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

/// 線形（恒等関数）。
///
/// 現状未使用だが将来のアニメーション（カーソルブリンク等）で使うため公開。
#[allow(dead_code)]
pub fn linear(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// 開始時刻 → 経過時間 → 進捗 `[0, 1]`。
///
/// - `duration_ms == 0` の場合は常に `1.0`（アニメーション無効として扱う）
/// - 経過時間 ≥ duration の場合は `1.0`
/// - それ以外は `elapsed / duration`
pub fn compute_progress(start: Instant, now: Instant, duration_ms: u32) -> f32 {
    if duration_ms == 0 {
        return 1.0;
    }
    let elapsed_ms = now.saturating_duration_since(start).as_millis() as f32;
    (elapsed_ms / duration_ms as f32).clamp(0.0, 1.0)
}

/// 全アニメーションを管理する構造体。
///
/// イベント発生時に `record_*` を呼ぶと内部にタイムスタンプを保存し、
/// レンダラーは `tab_switch_progress(now)` / `pane_fade_in_progress(id, now)`
/// で進捗を問い合わせる。
#[derive(Debug, Default)]
pub struct AnimationManager {
    /// 直近のタブ切替アニメーションの開始時刻 + 切替先 pane_id
    tab_switch: Option<TabSwitchState>,
    /// 新規ペイン追加アニメーション（pane_id → 開始時刻）
    pane_fade_ins: HashMap<u32, Instant>,
}

#[derive(Debug, Clone, Copy)]
struct TabSwitchState {
    /// 切替先の pane_id
    to_pane: u32,
    /// 切替が始まった時刻
    started_at: Instant,
}

impl AnimationManager {
    /// 新しい [`AnimationManager`] を返す（全て空）。
    pub fn new() -> Self {
        Self::default()
    }

    /// タブ切替イベントを記録する（フォーカスペインが変わった瞬間に呼ぶ）。
    ///
    /// 同じ pane_id を 2 回連続で記録するとリセットされる（最新時刻で上書き）。
    pub fn record_tab_switch(&mut self, to_pane: u32, now: Instant) {
        self.tab_switch = Some(TabSwitchState {
            to_pane,
            started_at: now,
        });
    }

    /// ペイン追加イベントを記録する。
    pub fn record_pane_added(&mut self, pane_id: u32, now: Instant) {
        self.pane_fade_ins.insert(pane_id, now);
    }

    /// ペインが削除された場合に対応するアニメーション状態を破棄する。
    pub fn record_pane_removed(&mut self, pane_id: u32) {
        self.pane_fade_ins.remove(&pane_id);
        if let Some(ref s) = self.tab_switch
            && s.to_pane == pane_id
        {
            self.tab_switch = None;
        }
    }

    /// 期限切れのアニメーション状態を掃除する（メモリ漏れ防止）。
    ///
    /// 呼び出し頻度: フレーム末尾やアイドル時に呼ぶ。`duration_ms` を超えた状態を削除。
    /// 現状は明示的なクリーンアップフックがないため未使用だが、進捗値で 1.0 を返す
    /// ロジックに依存しているのでメモリ漏れにはならない。将来 record_* の回数が
    /// 増える局面で有効化する。
    #[allow(dead_code)]
    pub fn cleanup_expired(&mut self, now: Instant, duration_ms: u32) {
        if duration_ms == 0 {
            self.tab_switch = None;
            self.pane_fade_ins.clear();
            return;
        }
        let dur = Duration::from_millis(duration_ms as u64);
        if let Some(ref s) = self.tab_switch
            && now.saturating_duration_since(s.started_at) > dur
        {
            self.tab_switch = None;
        }
        self.pane_fade_ins
            .retain(|_, started_at| now.saturating_duration_since(*started_at) <= dur);
    }

    /// タブ切替アニメーションの進捗を返す（[0, 1]）。
    ///
    /// 切替イベントがない場合は `1.0`（完全に表示完了 = アニメーションなし扱い）。
    pub fn tab_switch_progress(&self, now: Instant, duration_ms: u32) -> f32 {
        match self.tab_switch {
            Some(s) => compute_progress(s.started_at, now, duration_ms),
            None => 1.0,
        }
    }

    /// 指定 pane の新規追加フェードイン進捗を返す（[0, 1]）。
    ///
    /// 記録がない場合は `1.0`（完全に表示完了）。
    pub fn pane_fade_in_progress(&self, pane_id: u32, now: Instant, duration_ms: u32) -> f32 {
        match self.pane_fade_ins.get(&pane_id) {
            Some(started_at) => compute_progress(*started_at, now, duration_ms),
            None => 1.0,
        }
    }

    /// 直近のタブ切替先 pane_id を返す（アクティブ時のみ）。
    pub fn current_tab_switch_target(&self) -> Option<u32> {
        self.tab_switch.as_ref().map(|s| s.to_pane)
    }

    /// 何らかのアニメーションが進行中かを返す（再描画要求の判定に使う）。
    ///
    /// FPS 制限と組み合わせる場合に有用。現状は毎フレーム再描画しているため未使用だが、
    /// アイドル時の再描画スキップ最適化で活用予定。
    #[allow(dead_code)]
    pub fn has_active_animation(&self, now: Instant, duration_ms: u32) -> bool {
        if duration_ms == 0 {
            return false;
        }
        if self.tab_switch_progress(now, duration_ms) < 1.0 {
            return true;
        }
        self.pane_fade_ins
            .values()
            .any(|started_at| compute_progress(*started_at, now, duration_ms) < 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    // ---- easing 関数 ----

    #[test]
    fn ease_out_cubic_は端点で_0_と_1() {
        assert!(approx(ease_out_cubic(0.0), 0.0));
        assert!(approx(ease_out_cubic(1.0), 1.0));
    }

    #[test]
    fn ease_out_cubic_は単調増加() {
        let v00 = ease_out_cubic(0.0);
        let v25 = ease_out_cubic(0.25);
        let v50 = ease_out_cubic(0.5);
        let v75 = ease_out_cubic(0.75);
        let v100 = ease_out_cubic(1.0);
        assert!(v00 < v25 && v25 < v50 && v50 < v75 && v75 < v100);
    }

    #[test]
    fn ease_out_cubic_は中央付近で線形より大きい() {
        // ease-out は前半が速い → t=0.3 で linear より進んでいる
        assert!(ease_out_cubic(0.3) > linear(0.3));
    }

    #[test]
    fn ease_out_cubic_は範囲外をクランプする() {
        assert!(approx(ease_out_cubic(-1.0), 0.0));
        assert!(approx(ease_out_cubic(2.0), 1.0));
    }

    #[test]
    fn linear_は恒等関数() {
        assert!(approx(linear(0.0), 0.0));
        assert!(approx(linear(0.5), 0.5));
        assert!(approx(linear(1.0), 1.0));
        assert!(approx(linear(-0.5), 0.0));
        assert!(approx(linear(1.5), 1.0));
    }

    // ---- compute_progress ----

    #[test]
    fn compute_progress_duration_0_は常に_1() {
        let now = Instant::now();
        assert!(approx(compute_progress(now, now, 0), 1.0));
        let later = now + Duration::from_secs(60);
        assert!(approx(compute_progress(now, later, 0), 1.0));
    }

    #[test]
    fn compute_progress_経過_0_は_0() {
        let now = Instant::now();
        assert!(approx(compute_progress(now, now, 200), 0.0));
    }

    #[test]
    fn compute_progress_経過_半分は_0_5() {
        let start = Instant::now();
        let now = start + Duration::from_millis(100);
        assert!(approx(compute_progress(start, now, 200), 0.5));
    }

    #[test]
    fn compute_progress_経過_超過は_1にクランプ() {
        let start = Instant::now();
        let now = start + Duration::from_millis(500);
        assert!(approx(compute_progress(start, now, 200), 1.0));
    }

    // ---- AnimationManager ----

    #[test]
    fn 新規マネージャーは何も持たない() {
        let mgr = AnimationManager::new();
        let now = Instant::now();
        // どのペインも完了扱い
        assert!(approx(mgr.tab_switch_progress(now, 200), 1.0));
        assert!(approx(mgr.pane_fade_in_progress(1, now, 200), 1.0));
        assert!(!mgr.has_active_animation(now, 200));
    }

    #[test]
    fn record_tab_switch_直後の進捗は_0() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(approx(mgr.tab_switch_progress(now, 200), 0.0));
        assert_eq!(mgr.current_tab_switch_target(), Some(7));
    }

    #[test]
    fn record_pane_added_直後の進捗は_0() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_pane_added(42, now);
        assert!(approx(mgr.pane_fade_in_progress(42, now, 200), 0.0));
        // 他のペインには影響しない
        assert!(approx(mgr.pane_fade_in_progress(43, now, 200), 1.0));
    }

    #[test]
    fn record_pane_removed_はフェードインを削除する() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_pane_added(42, now);
        mgr.record_pane_removed(42);
        // 削除後は記録がなく 1.0 になる
        assert!(approx(mgr.pane_fade_in_progress(42, now, 200), 1.0));
    }

    #[test]
    fn record_pane_removed_は対応するタブ切替もクリアする() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        mgr.record_pane_removed(7);
        assert_eq!(mgr.current_tab_switch_target(), None);
        // 別 pane の切替は影響を受けない
        mgr.record_tab_switch(8, now);
        mgr.record_pane_removed(7);
        assert_eq!(mgr.current_tab_switch_target(), Some(8));
    }

    #[test]
    fn cleanup_expired_で期限切れが消える() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        mgr.record_pane_added(42, now);

        // duration の倍を経過した時刻でクリーンアップ
        let later = now + Duration::from_millis(500);
        mgr.cleanup_expired(later, 200);

        assert_eq!(mgr.current_tab_switch_target(), None);
        assert!(approx(mgr.pane_fade_in_progress(42, later, 200), 1.0));
    }

    #[test]
    fn cleanup_expired_duration_0_は全消去() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        mgr.record_pane_added(42, now);
        mgr.cleanup_expired(now, 0);
        assert_eq!(mgr.current_tab_switch_target(), None);
        assert!(mgr.pane_fade_ins.is_empty());
    }

    #[test]
    fn has_active_animation_の判定() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(mgr.has_active_animation(now, 200));

        // duration 経過後は false
        let later = now + Duration::from_millis(300);
        assert!(!mgr.has_active_animation(later, 200));

        // duration_ms = 0 では常に false
        assert!(!mgr.has_active_animation(now, 0));
    }

    #[test]
    fn record_pane_added_は同じ_id_を上書きする() {
        let mut mgr = AnimationManager::new();
        let t0 = Instant::now();
        mgr.record_pane_added(42, t0);
        // 100ms 後に再登録 → 新しい開始時刻に置き換わる
        let t1 = t0 + Duration::from_millis(100);
        mgr.record_pane_added(42, t1);
        // t1 時点ではまた 0.0
        assert!(approx(mgr.pane_fade_in_progress(42, t1, 200), 0.0));
    }
}
