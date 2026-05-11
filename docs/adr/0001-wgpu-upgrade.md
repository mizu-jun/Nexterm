# ADR-0001: wgpu アップグレード戦略

## ステータス

調査済み（2026-05-11、Sprint 5-3 / C2）。実コード変更は将来 Sprint に分離。

## コンテキスト

Nexterm v1.1.0 時点で `wgpu = "22"` (workspace dep) を使用。
監査ラウンド 2 の C2 タスクで、より新しい wgpu への移行可否を評価した。

主な動機は次のとおり:

1. **RUSTSEC-2024-0436 (paste) の解消**: 当初 wgpu 22 → 23 で解消する見込みだったが、
   `cargo tree -p nexterm-client-gpu -i paste` で確認したところ依存元は
   `wasmi 0.38 → wasmi_core → paste 1.0.15` であり、wgpu アップグレードでは解消しない。
   この目的なら wasmi のメジャー版を上げるか paste を置き換える別タスクが必要。
2. wgpu 自体の脆弱性・パフォーマンス改善・最新 GPU API の追従。
3. cosmic-text / winit との将来的整合（cosmic-text 0.18 はまだ wgpu 22 系を許容）。

## 検討した選択肢

### A. wgpu 22 のまま据え置く

- 利点: 工数 0、即座のリスク 0、cosmic-text 0.18 と完全互換。
- 欠点: 数バージョン遅れている。中長期で追従コストが増える。

### B. wgpu 23 へ最小限のアップグレード

- 利点: `Instance::new` のシグネチャは 22 と概ね互換。
  本コードベースで使用する API のうち、ほぼ変更が要らない見込み。
- 欠点: 23 はすでに 24/25/26 が出ている過渡期版。再度近い将来に上げ直し。

### C. wgpu 26（現在の安定最新）へ一気にアップグレード

- 利点: 最新の改善（バリデーション・PollType・MemoryHints の選択肢）を取得。
- 欠点: 複数の breaking change を一度に対応する必要がある。

## wgpu 22 → 26 の breaking change（本コードベースに関連するもの）

`context7` で確認した範囲。バージョン別の正確な分岐は CHANGELOG 参照。

| 影響 | 22 | 26 | 本コード内の該当 |
|------|----|----|------------------|
| `wgpu::ImageCopyTexture` | あり | **削除** → `TexelCopyTextureInfo` | `renderer/mod.rs:1227`, `glyph_atlas.rs:215, 287` |
| `wgpu::ImageDataLayout` | あり | **削除** → `TexelCopyBufferLayout` | `renderer/mod.rs:1234`, `glyph_atlas.rs:226, 298` |
| `wgpu::ImageCopyBuffer` | あり | **削除** → `TexelCopyBufferInfo` | 未使用 |
| `Instance::new(desc)` | `Instance` を直接返す | 互換シグネチャは維持しつつ `new_with_display_handle` / `new_without_display_handle` が標準化 | `renderer/mod.rs:146` |
| `request_device(&desc, trace_path)` | 第 2 引数 `Option<&Path>` | 第 2 引数廃止。`DeviceDescriptor::trace: wgpu::Trace` に統合。`experimental_features` / 新しい `memory_hints` 形式も追加 | `renderer/mod.rs:163-173` |
| `device.poll(Maintain)` | `Maintain` 列挙 | **`PollType`** に rename | 本コードで未使用（要再確認） |
| `PresentMode::AutoVsync` | 既存 | 既存（変更なし見込み） | `renderer/mod.rs:186` |

### 影響箇所サマリ

- `nexterm-client-gpu/src/renderer/mod.rs`: 5 〜 7 箇所
- `nexterm-client-gpu/src/glyph_atlas.rs`: 4 箇所
- 合計 **約 10 箇所**の機械的置換 + `request_device` のフィールド調整

加えて Cargo.toml の `wgpu = "22"` → `wgpu = "26"` の更新、Cargo.lock の再生成、
依存クレート（特に cosmic-text、winit）との互換確認が必要。

## 結論

**Option B を後送り、現時点では Option A（据え置き）を維持する。**

理由:

1. RUSTSEC-2024-0436 の解消が wgpu アップグレードで達成できないことが判明した
   （別タスクで wasmi/paste 経路の対処が必要）。
   wgpu アップグレード自体の優先度は低下した。
2. cosmic-text 0.18 → 0.x（wgpu 26 対応版）との同時アップグレードが必要になる可能性が高く、
   API 影響範囲が広がる。Sprint 5-3 のスコープ（性能・ベンチマーク）外。
3. 本作業は **Sprint 5-4 以降**、cosmic-text アップグレードと併せて実施するのが効率的。

## 将来 Sprint で実施する手順（メモ）

1. **準備**:
   - `cosmic-text` の最新版を確認、wgpu 互換性表を参照。
   - `winit 0.30.x` の最新リリースノートを確認（ApplicationHandler 周りに breaking change がないこと）。
2. **依存更新**: workspace `Cargo.toml` の `wgpu = "26"` / `cosmic-text = "<対応版>"` を更新。
3. **rename 対応**（機械的）:
   ```bash
   # 本コードベース内のリネーム対象
   grep -rln "ImageCopyTexture" nexterm-client-gpu/src/ \
     | xargs sed -i 's/ImageCopyTexture/TexelCopyTextureInfo/g'
   grep -rln "ImageDataLayout" nexterm-client-gpu/src/ \
     | xargs sed -i 's/ImageDataLayout/TexelCopyBufferLayout/g'
   ```
4. **`request_device` 更新**: `DeviceDescriptor` に `experimental_features` / `trace`
   を追加、第 2 引数 `None` を削除。
5. **動作確認**:
   - `cargo build -p nexterm-client-gpu`
   - `cargo clippy -p nexterm-client-gpu -- -D warnings`
   - GUI 起動して描画崩れ・パフォーマンス低下がないこと
6. **ベンチマーク回帰確認**: Sprint 5-3 で導入した `vt_throughput` は VT 層のみで
   wgpu に依存しないが、参考までに前後比較。
   GPU 系のマイクロベンチがあればそちらも比較（現状未整備）。

## 関連

- 監査ラウンド 2: `memory/project_audit_round2.md` C2
- Sprint 5-3 進捗（本 ADR の出処）
- 関連タスク（別件として残す）:
  - wasmi 0.38 → 最新 + paste 経路解消（RUSTSEC-2024-0436）
  - cosmic-text 0.18 → 最新（C7）
