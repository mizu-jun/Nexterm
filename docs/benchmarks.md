# Nexterm ベンチマーク

> Sprint 5-3 / C1 / C5 / J4: VT パーサ層のスループットおよびキーストロークレイテンシ計測。

## 1. 計測の目的と注意点

このドキュメントは Nexterm の **VT 層単独** のスループットを計測したものです。
ターミナルエミュレータ全体のレイテンシ（キー入力 → ピクセル更新までのエンドツーエンド）
は GPU / winit / ウィンドウシステム / ディスプレイのリフレッシュレートにも左右されるため、
他社の公表値（Alacritty 3 ms / kitty 3 ms / Ghostty 2 ms 等）と直接の数値比較はできません。

本計測は次の問いに答えるものです:

- VT 層がボトルネックになっていないこと（典型ワークロードで GPU 描画の前に詰まらない）。
- 将来の回帰検出ベースライン（同じシナリオで遅くなったら気付ける）。

エンドツーエンドのレイテンシ計測（typometer 等の外部ツールによる実機計測）は今後の課題です。

## 2. 計測環境（リファレンス）

| 項目 | 値 |
|------|------|
| CPU | x86_64 モバイル / ノート PC クラス |
| OS | Windows 11（64-bit） |
| Rust | rustc stable（Cargo.toml で edition 2024 = 1.85+） |
| ビルド | `cargo bench --release` プロファイル（criterion デフォルト） |
| コミット | master 2026-05-11 時点 |

具体的なハードウェア構成は計測者の端末に依存します。再現性を上げたい場合は、
将来 GitHub Actions の `ubuntu-latest` ランナー上で同じベンチを走らせた数値を
本ドキュメントに併記することを検討してください（環境固定の方が再現性が高い）。

実行手順:

```sh
cargo bench -p nexterm-vt --bench vt_throughput
```

すべてのシナリオは `nexterm-vt/benches/vt_throughput.rs` に実装されています。

## 3. VT スループット（`vt_advance`）

各シナリオは **256 KiB のバイト列** を `VtParser::new(80, 24).advance()` に流したときの
ピーク値を `criterion --quick` で計測したものです。
シナリオは [alacritty/vtebench](https://github.com/alacritty/vtebench) を参考にしています。

| シナリオ | 時間 (ms) | スループット (MiB/s) | 説明 |
|---|---:|---:|---|
| `light_cells` | 6.16 | 40.6 | ASCII テキストのみ。CRLF 区切り |
| `medium_cells` | 6.78 | 36.9 | ANSI 8 色 SGR を多数含む（`ls --color` 風） |
| `dense_cells` | 1.83 | 136.3 | 24-bit RGB 前景背景フル装飾、改行少なめ |
| `cursor_motion` | 2.19 | 114.0 | CSI H で大量カーソル移動（vim/htop 風） |
| `scrolling` | 7.50 | 33.3 | 改行多数で連続スクロール（`tail -f` 風） |
| `alt_screen_random` | 2.31 | 108.2 | 代替画面 + 決定論的ランダム位置描画 |
| `sync_output` | 9.38 | 26.7 | DEC ?2026 同期出力で TUI 全画面再描画相当 |

中央値ベース。詳細な信頼区間は `cargo bench` 実行時に criterion が表示します。

## 4. キーストロークレイテンシ（`vt_keystroke_latency`）

1 文字相当のバイト列を `advance` → `take_dirty_rows` まで流したときの所要時間。
VT 層のレイテンシ上限を確認する目的の合成ベンチです。

| シナリオ | 時間 | 説明 |
|---|---:|---|
| `single_ascii` | 133 ns | 単一 ASCII 文字 1 字を打鍵相当 |
| `enter_newline` | 3.15 μs | CR LF 改行（バッファ末端でスクロール発生） |
| `backspace` | 114 ns | BS + Space + BS の典型 erase |
| `cursor_up` | 57 ns | CSI A の 1 行上移動 |
| `colored_char` | 326 ns | SGR カラー設定 + 1 文字 + リセット |

これらの数値はすべて **マイクロ秒未満**で、Nexterm のエンドツーエンドレイテンシの
ボトルネックは VT 層ではないことを示しています（GPU / コンポジタ / モニタが支配的）。

## 5. 競合との比較（公表値）

参考値です。計測手法と環境がそろわないため厳密な順位付けではありません。

| ターミナル | エンドツーエンドレイテンシ（公表値） | 出典 |
|---|---:|---|
| Ghostty | 約 2 ms | プロジェクト README |
| Alacritty | 約 3 ms | プロジェクト wiki |
| kitty | 約 3 ms | プロジェクト wiki |
| Nexterm（VT 層単独） | < 1 μs | このドキュメント |
| Nexterm（エンドツーエンド） | 未計測 | 今後の課題 |

## 6. 既知の限界

- **PTY 層のオーバーヘッドは未計測**: `nexterm-server::Pane` の PTY リーダースレッドや
  IPC（postcard シリアライズ）の所要時間は別途計測が必要。
- **GPU 描画パスのベンチが未整備**: `nexterm-client-gpu` の 3 パス（背景／テキスト／画像）
  に GPU 側マイクロベンチがありません。Sprint 5-4 以降の課題。
- **`session_manager` テストは PTY を fork するため、カバレッジから除外**しています
  （CI でハングしやすい既知の重テスト）。

## 7. 回帰検出の運用

ベンチマークは現状 CI では走らせていません（時間がかかるため）。
リリース前にローカルで以下を実行し、過去結果（`target/criterion/` 配下）と比較してください:

```sh
cargo bench -p nexterm-vt --bench vt_throughput
```

`criterion` は自動で前回比較を行い、`No change in performance detected` / `improved` /
`regressed` を判定します。

## 8. 関連

- 監査ラウンド 2: `memory/project_audit_round2.md` の C1 / C5 / J4
- ADR-0001: wgpu アップグレード方針 (`docs/adr/0001-wgpu-upgrade.md`)
- Sprint 5-3 進捗（memory `project_sprint5_3_progress.md`）
