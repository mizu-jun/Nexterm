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
use std::time::Instant;

use tracing::{info, warn};

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
mod gpu_buffers;
mod image;
mod render_frame;
mod shader_reload;
mod wgpu_init;

pub use app::NextermApp;
pub use event_handler::EventHandler;

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
struct WgpuState {
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
