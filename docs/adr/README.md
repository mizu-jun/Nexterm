# Architecture Decision Records (ADR)

このディレクトリは Nexterm の重要なアーキテクチャ決定を記録する場所です。

## ADR とは

ADR（Architecture Decision Record）は、プロジェクトで重要な技術選択をした際の **「なぜそうしたか」** を残す軽量ドキュメントです。
コードを読めば *何を* やっているかは分かりますが、 *なぜその選択をしたか* は時間が経つと失われるため、ADR にまとめます。

詳細: [Michael Nygard "Documenting Architecture Decisions"](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)

## いつ ADR を書くか

以下のような **後で「なぜこうなった？」と聞かれそう** な決定を記録する:

- フレームワーク・ライブラリ・プロトコルの選定（例: bincode → postcard）
- 後方互換性・バージョン管理方針（例: PROTOCOL_VERSION / Plugin v1→v2）
- データ構造の中核設計（例: BSP ツリーによるペイン分割）
- セキュリティトレードオフ（例: TLS フォールバックの可否）
- パフォーマンスとシンプルさのトレードオフ（例: present_mode のデフォルト）

迷ったら書く方が安全（書いても得られる情報量が多い）。

## ADR インデックス

| ID | タイトル | ステータス | 日付 |
|----|---------|----------|------|
| [0001](0001-wgpu-upgrade.md) | wgpu 22 → 26 アップグレード方針 | 採用 | 2026-05-12 |
| [0002](0002-protocol-versioning.md) | PROTOCOL_VERSION の u32 採用と minor バンプ方針 | 採用 | 2026-05-12 (遡及) |
| [0003](0003-plugin-api-v2.md) | Plugin API v1 → v2 移行と削除タイミング | 採用 | 2026-05-12 (遡及) |
| [0004](0004-toml-lua-config.md) | TOML + Lua のハイブリッド設定方式 | 採用 | 2026-05-12 (遡及) |
| [0005](0005-bsp-pane-layout.md) | BSP ツリーによる pane 分割設計 | 採用 | 2026-05-12 (遡及) |
| [0006](0006-postcard-vs-bincode.md) | IPC シリアライザ: bincode 1.x → postcard 移行 | 採用 | 2026-05-12 (遡及) |
| [0007](0007-snapshot-v1-deprecation.md) | Snapshot v1 削除タイミング (v2.0.0 で `SNAPSHOT_VERSION_MIN` を 2 に bump) | 採用 | 2026-05-12 |

## 新規 ADR の追加手順

1. `template.md` をコピーして連番（例: `0007-xxx.md`）でファイルを作成
2. ステータスを「提案中」で開始し、関係者レビューを経て「採用」に変更
3. 本 `README.md` のインデックステーブルに行を追加
4. 採用後は **本文を書き換えない**（修正したい場合は新規 ADR を起票して「代替済み」とリンクする）

## ステータスの意味

- **提案中**: 議論中、まだ実装には反映されていない
- **採用**: 決定済み、コードベースに反映済み
- **廃止**: もう適用されない（後継 ADR が無い場合）
- **代替済み (ADR-NNNN)**: 後続の ADR で置き換えられた

## 関連ドキュメント

- [docs/benchmarks.md](../benchmarks.md) — 性能計測リファレンス
- [CLAUDE.md](../../CLAUDE.md) — リポジトリ概要・コーディング規約
- [memory/](../../memory/) — Sprint 進捗・監査結果（claude memory）
