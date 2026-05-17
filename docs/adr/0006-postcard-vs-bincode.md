# ADR-0006: IPC シリアライザ: bincode 1.x → postcard 移行

## ステータス

採用 (2026-05-12、Sprint 5-1 / G3 の決定を遡及記録)

## コンテキスト

Nexterm の IPC（クライアント↔サーバー間 Unix ソケット / Windows 名前付きパイプ）は
`ClientToServer` / `ServerToClient` メッセージを serde 経由で送受信する。

### 初期実装

`bincode 1.x` を採用。Rust エコシステムで広く使われ、serde 互換で簡潔。

### 問題発生（2025）

- **RUSTSEC-2025-0141**: bincode 1.x にメンテナンス停止リスクのアドバイザリが出た
- `cargo audit` で `.cargo/audit.toml` / `deny.toml` に ignore を入れる対応をしていたが、
  根本的解決ではない（依存上の脆弱性が将来 CVE 化する可能性）

### 代替候補

- **bincode 2.x**: API が再設計されており、移行コストはあるが同系統
- **postcard**: no_std 対応・embedded 向け設計だが Nexterm でも適合する。サイズが小さく、
  メンテナンスが活発。RUSTSEC アドバイザリ無し
- **rmp-serde (MessagePack)**: 互換性高いが、サイズと速度で postcard に劣る

### 監査ラウンド 2 タスク G3

CRITICAL 優先度で「bincode 1.x → postcard 移行」が挙がっていた（Sprint 5+ の最優先課題の 1 つ）。

## 決定

**postcard 1.x に移行する。**

### Sprint 5-1 で実施した変更

1. **`nexterm-proto/Cargo.toml`** — bincode を依存から外し、postcard を追加
2. **IPC エンドポイントすべてのシリアライザを postcard に書き換え**:
   - `nexterm-server/src/ipc/`
   - `nexterm-client-gpu/src/connection.rs`
   - `nexterm-client-tui/`
   - `nexterm-ctl/src/ipc.rs`
   - `nexterm-launcher/`（v1.4.0 で削除済み）
3. **`PROTOCOL_VERSION` 1 → 2 → 3 に bump**（移行段階を識別するため）
   - v1: bincode
   - v2: postcard + 旧フィールドレイアウト
   - v3: postcard + Sprint 5-1 で整理したメッセージレイアウト
4. **postcard ラウンドトリップテストを追加**（nexterm-ctl/src/main.rs の `#[cfg(test)]` mod）
5. **`cargo audit` の bincode ignore を削除**

### 移行のポイント

- メッセージ長プレフィックス（4 バイト LE）は維持
- バイト数は若干減（postcard の方が varint 圧縮で効率的）
- ベンチマーク（Sprint 5-3 で計測）: パース速度は同等またはわずかに高速

## 影響

### ポジティブ

- RUSTSEC アドバイザリの解消
- `cargo audit` の ignore リストから 1 件削除
- バイナリサイズが若干小さい（postcard の varint）
- メンテナンスが活発なライブラリへの依存

### ネガティブ

- 移行作業の工数 L（多数のクレートに散らばっていたため）
- postcard の API は bincode と微妙に違う（学習コスト）
- 旧 v1 クライアントとの非互換（クライアント・サーバーを同時更新する必要）

## 代替案

- **代替案 A: bincode 1.x のまま ignore で運用**: 将来の CVE 化リスクを抱え続ける
- **代替案 B: bincode 2.x へ更新**: API 再設計で移行コストは同等、しかし将来も似た問題が再発する可能性
- **代替案 C: rmp-serde (MessagePack)**: サイズ・速度で postcard に劣る
- **代替案 D: 独自フォーマット**: 過剰な複雑さ

## 参照

- Sprint 5-1 進捗: `memory/project_sprint5_1_progress.md`
- 監査ラウンド 2 タスク G3
- RUSTSEC-2025-0141: bincode 1.x advisory
- postcard 公式: https://github.com/jamesmunns/postcard
- Sprint 5-3 ベンチマーク: `docs/benchmarks.md`（VT 層のパース速度に postcard の影響は無視できるレベル）
