# ADR-0007: Snapshot v1 削除タイミング

## ステータス

採用 (2026-05-12、Sprint 5-5 / A9 対応)

## コンテキスト

`nexterm-server` のセッション永続化スナップショットは v1 / v2 の二バージョンを並行サポートしている。

```rust
// nexterm-server/src/snapshot.rs
pub const SNAPSHOT_VERSION: u32 = 2;
pub const SNAPSHOT_VERSION_MIN: u32 = 1;
```

### 経緯

- **v1**: 初期スキーマ（`shell_args` は後から追加し `#[serde(default)]` で互換維持）
- **v2**: `session_title` フィールドを追加。Sprint 5-1 以降で `persist::load_snapshot` が v1 を v2 に自動マイグレート

### 監査ラウンド 2 タスク A9

「Snapshot v1 削除タイミングを明示すべき」と指摘されていた
（`snapshot.rs:34` の `SNAPSHOT_VERSION_MIN = 1` を v2.0 で 2 に上げる予定だが未文書化）。

ADR-0003（Plugin API v2 削除タイミング）と同じ方針で、本 ADR で確定する。

## 決定

1. **v2 を標準スキーマとする**: 新規スナップショットはすべて v2 で保存される（既に実装済）
2. **v1 読み込みサポートは v2.0.0 リリース時に削除予定**
3. **v1 読み込み時に migration 警告をログ出力する**（既存実装の維持）
4. **v2.0.0 リリース時に `SNAPSHOT_VERSION_MIN` を 2 に引き上げる**
5. **v1 スナップショットを保有するユーザー向け移行手順**:
   - v1.x のうちに 1 度サーバーを起動すれば自動的に v2 に書き換わる
   - v2.0.0 以降に直接アップグレードすると v1 スナップショットはロード不能になる旨を CHANGELOG に明記する

### 削除タイミングの根拠

- v2.0.0 = メジャーバージョン bump 時に破壊的変更を集約する慣例（ADR-0003 と整合）
- v1.x の間に自動マイグレーションが完了する設計のため、ユーザー操作不要
- ロード時のフォールバック分岐コードを削減でき、`persist.rs` の保守性が向上

## 影響

### ポジティブ

- 削除タイミングが明確（v2.0.0）になり、ユーザーは計画的にアップグレードできる
- `persist::load_snapshot` の v1 マイグレーションコードを v2.0.0 で削除可能
- `SNAPSHOT_VERSION_MIN` を bump することでセキュリティ的な互換境界を明示できる

### ネガティブ

- v2.0.0 リリースまでマイグレーションコードを保守する必要がある（既存コードのため追加コストは小さい）
- v1.x の最終バージョンで起動せずに v2.0.0 に飛んだユーザーはセッションを失う
  → CHANGELOG / README で告知する

## 代替案

- **代替案 A: v1 サポート無期限**: マイグレーション分岐コードが残り続け、`SNAPSHOT_VERSION_MIN` の意味が薄れる
- **代替案 B: 即時 v1 削除**: 既に v1 スナップショットを持つユーザー（特に長期セッションを保持する利用者）が影響を受ける
- **代替案 C: 自動バックアップ + v1 削除**: 実装コストが大きい。v1→v2 自動マイグレートで実質的に同等の保護を提供できる

## 参照

- `nexterm-server/src/snapshot.rs` の `SNAPSHOT_VERSION` / `SNAPSHOT_VERSION_MIN` 定義
- `nexterm-server/src/persist.rs` の `load_snapshot` マイグレーションロジック
- ADR-0003: Plugin API v1 → v2 移行と削除タイミング（同じ方針）
- 監査ラウンド 2 タスク A9
