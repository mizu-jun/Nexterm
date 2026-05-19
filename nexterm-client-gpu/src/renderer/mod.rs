//! wgpu + winit レンダラー
//!
//! 描画パイプライン:
//!   1. ターミナルセルの背景色を頂点バッファで描画（カラーパス）
//!   2. cosmic-text でグリフをラスタライズし、グリフアトラスに書き込む
//!   3. グリフアトラスからサンプリングしてテキストを描画（テキストパス）
//!
//! 頂点ビルダーサブモジュール:
//! - `grid_verts` — グリッド / スクロールバック / 境界線
//! - `overlay` — タブバー / ステータス / 検索バー / オーバーレイ各種
//! - `ui_verts` — コンテキストメニュー / 同意ダイアログ / 更新バナー
//!
//! ランタイムサブモジュール:
//! - `app` — `NextermApp`
//! - `event_handler` — winit `ApplicationHandler`
//! - `input_handler` — キー入力ディスパッチ
//!
//! wgpu 内部サブモジュール（Sprint 5-6 で分割）:
//! - `wgpu_init` — `WgpuState::new` / `resize` / `select_present_mode`
//! - `render_frame` — `WgpuState::render`
//! - `gpu_buffers` — 背景・テキスト頂点バッファのアップロード
//! - `image` — 画像テクスチャと頂点構築
//! - `shader_reload` — カスタムシェーダーのホットリロード

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use nexterm_proto::PaneLayout;
use tracing::{info, warn};

use crate::state::{ContextMenu, CopyModeState, SearchState};

// ---- 頂点ビルダーサブモジュール（Sprint 2-1 Phase A）----
// Sprint 5-4 / A2: overlay_verts.rs (1,958 行) を overlay/ サブディレクトリに再分割
mod grid_verts;
mod overlay;
mod ui_verts;

// ---- ランタイムサブモジュール（Sprint 2-1 Phase B/C）----
mod app;
mod event_handler;
mod input_handler;

// ---- wgpu 内部サブモジュール（Sprint 5-6 でファイル分割）----
mod background_pass;
mod gpu_buffers;
mod image;
mod render_frame;
mod shader_reload;
mod wgpu_init;

pub use app::NextermApp;
pub use event_handler::{EventHandler, UserEvent};

use background_pass::BackgroundTexture;
use image::ImageEntry;

// ---- シェーダーファイル監視 ----

/// カスタムシェーダーファイルを監視するウォッチャーを起動する。
///
/// 設定にシェーダーパスがある場合のみ監視を開始する。
/// ファイルが変更されると `()` を受信チャネルに送信する。
pub(super) fn start_shader_watcher(
    gpu_cfg: &nexterm_config::GpuConfig,
) -> (
    Option<tokio::sync::mpsc::Receiver<()>>,
    Option<notify::RecommendedWatcher>,
) {
    use notify::{Event, RecursiveMode, Watcher};

    let paths: Vec<std::path::PathBuf> = [
        gpu_cfg.custom_bg_shader.as_deref(),
        gpu_cfg.custom_text_shader.as_deref(),
    ]
    .iter()
    .flatten()
    .map(|p| std::path::PathBuf::from(shellexpand::tilde(p).as_ref()))
    .filter(|p| p.exists())
    .collect();

    if paths.is_empty() {
        return (None, None);
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<()>(1);

    let mut watcher = match notify::recommended_watcher(move |result: notify::Result<Event>| {
        if let Ok(event) = result {
            use notify::EventKind::*;
            if matches!(event.kind, Modify(_) | Create(_)) {
                info!("シェーダーファイルの変更を検知しました。パイプラインを再構築します。");
                let _ = tx.blocking_send(());
            }
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            warn!("シェーダーウォッチャーの起動に失敗しました: {}", e);
            return (None, None);
        }
    };

    for path in &paths {
        if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
            warn!(
                "シェーダーファイルの監視に失敗しました: {}: {}",
                path.display(),
                e
            );
        } else {
            info!("シェーダーファイルを監視中: {}", path.display());
        }
    }

    (Some(rx), Some(watcher))
}

// ---- wgpu コアステート ----

/// wgpu の初期化済み状態
///
/// 全フィールドは renderer サブモジュール（wgpu_init / render_frame / gpu_buffers /
/// image / shader_reload）から直接アクセスする。
///
/// 可視性 `pub(super)` は Sprint 5-8 Phase 4-1 Step 1.2 で `ClientWindow.wgpu` の
/// 公開可視性に揃えるため。EventHandler 等の親モジュールからも参照可能。
pub(super) struct WgpuState {
    device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    bg_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    text_bind_group_layout: wgpu::BindGroupLayout,
    /// 画像レンダリングパイプライン
    image_pipeline: wgpu::RenderPipeline,
    /// 画像用サンプラー
    image_sampler: wgpu::Sampler,
    /// 画像テクスチャキャッシュ（image_id → ImageEntry）
    image_textures: HashMap<u32, ImageEntry>,
    /// 背景画像（Sprint 5-7 / Phase 3-1）。`WindowConfig.background_image` 設定時のみロード
    background: Option<BackgroundTexture>,
    // ---- フレーム間再利用バッファ（毎フレームの GPU アロケーションを回避）----
    /// 背景頂点バッファ（VERTEX | COPY_DST、容量超過時は再確保）
    buf_bg_v: wgpu::Buffer,
    /// 背景インデックスバッファ
    buf_bg_i: wgpu::Buffer,
    /// テキスト頂点バッファ
    buf_txt_v: wgpu::Buffer,
    /// テキストインデックスバッファ
    buf_txt_i: wgpu::Buffer,
    /// 背景頂点バッファの現在容量（BgVertex 単位）
    bg_v_cap: u64,
    /// 背景インデックスバッファの現在容量（u16 単位）
    bg_i_cap: u64,
    /// テキスト頂点バッファの現在容量（TextVertex 単位）
    txt_v_cap: u64,
    /// テキストインデックスバッファの現在容量（u16 単位）
    txt_i_cap: u64,
    /// 最後にフレームを描画した時刻（FPS 制限用）
    last_frame_at: Instant,
}

// ---- 複数 OS Window 対応スケルトン（Sprint 5-8 Phase 4-1 Step 1.2）----

/// 各 OS Window 固有の表示状態を集約する型（Sprint 5-8 Phase 4-1 Step 1.3）。
///
/// 現在 `ClientState` 内に格納されている **per-OS-Window 化候補フィールド** を
/// 構造体として並行定義する。実機能配線（イベントハンドラの引数化・`ClientState`
/// からの移行）は Step 1.4 以降で段階的に行うため、本構造体は現状インスタンス化
/// されてもどこからも参照されない（`dead_code` allow 維持）。
///
/// 並行定義の理由は計画書（[[project_sprint5_7_phase4_plan]] Sprint 5-8 セクション）
/// に従い、`ClientState` 責務分割の波及をコンパイル不能期間ゼロで進めるため。
///
/// 含まれるフィールド（Step 1.4 以降で `ClientState` から段階移行）:
/// - `focused_server_window_id`: この OS Window がフォーカス中のサーバー Window ID
/// - `pane_layouts`: 表示中のペインレイアウト情報（per-window 描画用に複製）
/// - `copy_mode`: コピーモード（Vim 風テキスト選択）状態
/// - `search`: インクリメンタル検索状態
/// - `context_menu`: 右クリックで開いたコンテキストメニュー
/// - `hovered_tab_id`: タブバーでホバー中のタブ ID
#[allow(dead_code)]
pub(super) struct PerWindowViewState {
    pub(super) focused_server_window_id: u32,
    pub(super) pane_layouts: HashMap<u32, PaneLayout>,
    pub(super) copy_mode: CopyModeState,
    pub(super) search: SearchState,
    pub(super) context_menu: Option<ContextMenu>,
    pub(super) hovered_tab_id: Option<u32>,
}

impl Default for PerWindowViewState {
    fn default() -> Self {
        Self {
            focused_server_window_id: 0,
            pane_layouts: HashMap::new(),
            copy_mode: CopyModeState::new(),
            search: SearchState::new(),
            context_menu: None,
            hovered_tab_id: None,
        }
    }
}

/// 1 個の OS Window に紐付くペア型（Sprint 5-8 Phase 4-1 Step 1.2 スケルトン）。
///
/// 現状は単一 Window のみだが、Phase 4-2 以降で
/// `EventHandler.windows: HashMap<WindowId, ClientWindow>` として複数 OS Window を保持する。
///
/// 移行期間中（Step 1.2〜1.3）は既存の `EventHandler.window` / `EventHandler.wgpu_state`
/// フィールドと並行して保持され、Step 1.3 以降で段階的に統合していく。
///
/// Sprint 5-11-2 Step 2-3: 各 OS Window が独自の AccessKit Adapter を保持する。
/// プラットフォーム a11y アダプタは Window 単位で管理されるため、追加 Window では
/// 主 Window と独立したノードツリーが必要になる（現状の Step 2-3 では主 Window 用
/// `EventHandler::accesskit_adapter` を維持しつつ、追加 Window 用に本フィールドを用意）。
#[allow(dead_code)]
pub(super) struct ClientWindow {
    /// winit ネイティブウィンドウ
    pub(super) window: Arc<winit::window::Window>,
    /// wgpu 描画ステート
    pub(super) wgpu: WgpuState,
    /// per-OS-Window 表示状態（Step 1.3 で詳細フィールド追加予定）
    pub(super) view_state: PerWindowViewState,
    /// AccessKit プラットフォームアダプタ（Sprint 5-11-2 Step 2-3）。
    ///
    /// 各 OS Window ごとに独立した Adapter を保持。スクリーンリーダーは Window ごとに
    /// 別ツリーを扱えるため、追加 Window でも `InitialTreeRequested` を受信して
    /// `build_tree_from_state(&self.app.state)` を返す。
    pub(super) accesskit_adapter: accesskit_winit::Adapter,
}

#[cfg(test)]
mod client_window_tests {
    use super::*;

    #[test]
    fn per_window_view_state_default() {
        // Step 1.3 で `PerWindowViewState` を unit struct から本構造体に拡張した。
        // Default 実装が ClientState から per-OS-Window 化する候補フィールドを
        // 既存ロジックと一致する初期値で生成することを検証する。
        let view = PerWindowViewState::default();
        assert_eq!(view.focused_server_window_id, 0);
        assert!(view.pane_layouts.is_empty());
        assert!(view.context_menu.is_none());
        assert!(view.hovered_tab_id.is_none());
        // `copy_mode` / `search` 自身の初期状態の不変条件は各モジュールのテストで担保。
    }
}
