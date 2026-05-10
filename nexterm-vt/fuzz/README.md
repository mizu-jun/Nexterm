# nexterm-vt fuzzing

Sprint 3-5 で導入された [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) ベースの
ファジング基盤。VT パーサ・Sixel/Kitty デコーダ・OSC ハンドラに対する任意バイト入力で
パニック・メモリ枯渇・無限ループを起こさないことを継続的に検証する。

## ターゲット一覧

| ターゲット | 対象 API | 想定攻撃 |
|-----------|---------|---------|
| `vt_parser_input` | `VtParser::advance()` | 不正 CSI/OSC/DCS/APC、巨大パラメータ |
| `sixel_decode` | `image::decode_sixel()` | 巨大 repeat count、不正カラーマップ |
| `kitty_image` | `image::decode_kitty()` | 巨大 width/height、不正 base64 |
| `osc_url` | OSC 8 / 52 / 133 経由 | 巨大 URL、未知スキーマ、終端なし |

## ローカル実行

```bash
# 初回のみ
cargo install cargo-fuzz
rustup toolchain install nightly

cd nexterm-vt

# 60 秒だけ実行（CI と同じ条件）
cargo +nightly fuzz run vt_parser_input -- -max_total_time=60
cargo +nightly fuzz run sixel_decode    -- -max_total_time=60
cargo +nightly fuzz run kitty_image     -- -max_total_time=60
cargo +nightly fuzz run osc_url         -- -max_total_time=60

# 無限実行（クラッシュ発見モード）
cargo +nightly fuzz run vt_parser_input
```

## CI

`.github/workflows/fuzz.yml` で **毎日 UTC 03:00 (JST 12:00)** に各ターゲット 60 秒ずつ実行する。
`workflow_dispatch` で手動実行も可能。クラッシュ発見時は GitHub Actions のサマリで通知する。

## クラッシュ発見時の対応

```bash
# クラッシュペイロードの最小化
cargo +nightly fuzz tmin <target> artifacts/<target>/crash-xxxxx

# 再現テスト追加
# fuzz/artifacts/ に保存されたバイト列をユニットテストに昇格させる
```

## ワークスペース除外

`nexterm-vt/fuzz/` は親ワークスペースから除外されている (ルート `Cargo.toml` の `exclude` 参照)。
通常の `cargo build` / `cargo test` / `cargo clippy --workspace` には含まれず、
fuzz ディレクトリで `cargo +nightly fuzz` を実行したときのみ依存解決される。

## 関連 CRITICAL

- CRITICAL #5: OSC URL allowlist 強化 (Sprint 3-1 で対応済み)
- CRITICAL #7: APC バッファ上限 (Sprint 1 期間内で対応済み)
- HIGH #4: Sixel 巨大 repeat count
- HIGH #5: Kitty 画像サイズ検証
