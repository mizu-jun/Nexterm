# ADR-0002: PROTOCOL_VERSION の u32 採用と minor バンプ方針

## ステータス

採用 (2026-05-12、Sprint 5-1〜5-2 の決定を遡及記録)

## コンテキスト

Nexterm の IPC（クライアント↔サーバー間 Unix ソケット / 名前付きパイプ）は `Hello` ハンドシェイクで
プロトコルバージョンを交換する。バージョンの粒度・型・bump 方針が初期は曖昧だった。

監査ラウンド 2（2026-05-10）の A7 で「`PROTOCOL_VERSION` を u32 → (major, minor) タプル化 + ADR 化」が
MEDIUM 優先で挙がっていたが、Sprint 5-1〜5-2 で実装した現状を踏まえて改めて方針を整理する。

### Sprint 5-1〜5-2 で起きたこと

- **Sprint 5-1 (commit 35b9c5b)**: bincode → postcard 移行に伴い `PROTOCOL_VERSION` を 1 → 2 → 3 にバンプ
- **Sprint 5-2 (commit 829d55b)**: OSC 7 (CwdChanged) 追加で `PROTOCOL_VERSION` を 3 → 4 にバンプ
- 旧クライアントが新サーバーに接続した場合（または逆）は `HelloAck` 時点で拒否する

### 制約

- 設計時に「セマンティックバージョニング（major.minor）」と単純な u32 で迷ったが、
  実装の簡潔さを優先して u32 で始めた
- 既に v4 まで進んでおり、(major, minor) タプル化は破壊的変更

## 決定

`PROTOCOL_VERSION: u32` を維持し、**任意の互換性破壊を伴う変更で +1 する**。

minor / major の区別は導入しない。代わりに、サーバー側で「最低互換バージョン」（最低受理する
バージョン）を保持し、クライアント側もハンドシェイクで `HelloAck.server_proto_version` を読んで
互換性をチェックする。

## 影響

### ポジティブ

- 実装が単純（フィールド 1 個・ロジック分岐少）
- 既存コードベースをそのまま使える（v1〜v4 まで導入済み）
- バージョン番号が大きくなることに対する許容（u32 = 約 42 億まで）

### ネガティブ

- 「minor bump で後方互換」のような細やかなマイグレーション戦略は使えない
- すべての破壊的変更が一様に「+1」になるため、変更の規模が見えにくい
- 互換性チェックの粒度が「完全一致」となり、後方互換のフィールド追加でも bump が必要

## 代替案

- **代替案 A: (major, minor) タプル化**: より柔軟だが、既に v4 まで進んでおり破壊的。
  実装時のメンテナンスコスト増 vs 得られる柔軟性の比は低いと判断。
- **代替案 B: SemVer 文字列 ("1.4.0" 等)**: 過剰な抽象化。ハンドシェイク時のパースが必要。
- **代替案 C: 各メッセージごとに version フィールド**: 過剰な複雑さ。1 セッション内で
  バージョンが変わることは想定しない。

## 参照

- Sprint 5-1 進捗: `memory/project_sprint5_1_progress.md`（PROTOCOL_VERSION 3 への bump 経緯）
- Sprint 5-2 進捗: `memory/project_sprint5_2_progress.md`（PROTOCOL_VERSION 4 への bump）
- commit 35b9c5b: bincode → postcard + PROTOCOL_VERSION 3
- commit 829d55b: OSC 7 CwdChanged + PROTOCOL_VERSION 4
- 監査ラウンド 2 タスク A7（再評価により u32 維持）
