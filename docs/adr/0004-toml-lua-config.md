# ADR-0004: TOML + Lua のハイブリッド設定方式

## ステータス

採用 (2026-05-12、Sprint 1〜2 の決定を遡及記録)

## コンテキスト

ターミナルエミュレータの設定形式は競合各社で大きく分かれている:

- **静的**: Alacritty (TOML), Windows Terminal (JSON)
- **動的**: WezTerm (Lua), kitty (独自 KSL)
- **混合**: Ghostty (テキスト + 環境変数)

Nexterm が設定形式を選ぶにあたり、以下のトレードオフがあった:

- **静的のみ**: シンプル・型チェック容易・ホットリロード簡単。ただし条件分岐や動的計算ができない。
  ステータスバーのカスタマイズ等で柔軟性が足りない
- **動的のみ**: フルプログラマブル・ステータスバー等の動的評価が容易。ただし設定が長くなりがち、
  初心者参入障壁が高い、エラーがランタイムに出る
- **混合**: 静的部分は TOML、動的部分（ステータスバー・フック）は Lua にする

## 決定

**ハイブリッド形式を採用**:

1. `~/.config/nexterm/config.toml` — 静的設定（フォント・色・キーバインド等）
2. `~/.config/nexterm/config.lua` — オプション。Lua 関数で動的計算（ステータスバー左右の式、フック）
3. ロード順序:
   1. ビルトインデフォルト値
   2. config.toml をマージ
   3. config.lua があれば実行・結果をマージ
4. ファイル変更監視 → ホットリロード対応

### Lua の利用範囲

- ステータスバー左右の評価式（毎秒評価、`StatusBarEvaluator` でキャッシュ）
- フック（OSC 133 セマンティックゾーンに反応する `HookEvent`）
- 設定値の動的生成（環境変数・時刻に応じた切り替え等）

### Lua の制約

- `mlua::Lua` インスタンスは専用 OS スレッド (`nexterm-lua-worker`) に閉じ込めて
  メインスレッドとはチャネル通信（Send/Sync 制約への対処）
- サンドボックス化（`os.execute` 等の危険な API を制限。`nexterm-config/src/lua_sandbox.rs`）

## 影響

### ポジティブ

- 初心者は TOML だけで完結する。Lua は不要な人は触らなくてよい
- パワーユーザーは Lua でステータスバー・フックを自由に書ける
- TOML の型チェック（serde）でほとんどの設定エラーを起動時に検出できる
- WezTerm 流の柔軟性と Alacritty 流のシンプルさを両立

### ネガティブ

- 2 つの形式を覚える必要がある（ただし Lua は任意）
- Lua スレッド管理・サンドボックスの実装コスト
- Lua エラーがランタイムに発生する（TOML より遅い検出）
- ドキュメンテーション量が増える

## 代替案

- **代替案 A: TOML のみ**: シンプルだが、ステータスバー等の動的計算が貧弱になる
- **代替案 B: Lua のみ (WezTerm 流)**: フル柔軟だが、初心者参入障壁が高い
- **代替案 C: JSON + JS (VSCode 流)**: Node ランタイム同梱が重い・セキュリティ面でも複雑
- **代替案 D: 独自 DSL**: kitty が KSL でやっているが、新規言語の学習コストが発生

## 参照

- `nexterm-config/src/loader.rs` — TOML + Lua ロード順
- `nexterm-config/src/lua_worker.rs` — Lua 専用スレッド
- `nexterm-config/src/lua_sandbox.rs` — Lua サンドボックス
- `nexterm-config/src/status_bar.rs` — Lua によるステータスバー評価
- WezTerm 比較: https://wezfurlong.org/wezterm/config/files.html
