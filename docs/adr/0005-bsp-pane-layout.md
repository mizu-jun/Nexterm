# ADR-0005: BSP ツリーによる pane 分割設計

## ステータス

採用 (2026-05-12、Sprint 1 の決定を遡及記録)

## コンテキスト

ターミナルエミュレータの pane 分割（複数ターミナル並列表示）には複数の設計流派がある:

- **均等タイリング**: GridLayout で N×M に分割。シンプルだが任意の比率指定が難しい
- **BSP (Binary Space Partitioning)**: 各分割を 2 分木として表現。tmux / i3wm / Zellij 等が採用
- **フローティング**: 各 pane が自由位置・サイズを持つ。Windows Terminal が部分対応

### Nexterm の要件

- tmux 互換のキーバインドで pane 分割（`Ctrl+B "` 等）
- 任意の split direction (horizontal / vertical) の入れ子
- ペインのリサイズが直感的（隣接 pane が連動）
- セッションの永続化・テンプレート保存が可能（構造をシリアライズできる）

## 決定

**BSP（Binary Space Partitioning）ツリーを採用** する。

### データ構造

```rust
enum SplitNode {
    Pane(u32),  // pane_id（リーフ）
    Split {
        dir: SplitDir,    // Horizontal / Vertical
        ratio: f32,       // 0.0 〜 1.0
        left: Box<SplitNode>,
        right: Box<SplitNode>,
    },
}
```

各 `Window` が 1 つの `SplitNode` ルートを持ち、再帰的に分割を表現する。

### 主要操作

- **分割**: 既存リーフを `Split { left: 旧, right: 新 }` に置き換える
- **削除**: pane を含むリーフを取り、親 `Split` を兄弟リーフに昇格
- **リサイズ**: 該当 `Split.ratio` を変更し、再帰的に再計算
- **シリアライズ**: ツリー全体を JSON / postcard でラウンドトリップ

### Pane 追加の順序（重要）

「chicken-and-egg 問題」を避けるため、Pane 追加は以下の順:

1. **pane_id を事前確保**（`SessionManager` で連番採番）
2. **ツリーに挿入**（まだ PTY なし）
3. **全 pane サイズを再計算**（全リーフに対応する `PaneRect` 計算）
4. **PTY をスポーン**（このとき正しい cols/rows で起動）
5. **既存 pane をリサイズ**（PTY に SIGWINCH 相当の通知）

順序を間違えると PTY が誤ったサイズで起動し、その後のリサイズが効かない。

## 影響

### ポジティブ

- tmux と同じメンタルモデル（既存ユーザーが習熟済み）
- 任意の入れ子分割が表現可能（split 内に split を入れられる）
- ツリーをそのまま JSON 永続化できる（テンプレート機能）
- リサイズの局所性が高い（変更が必要な領域だけ再計算）

### ネガティブ

- データ構造が再帰的で、フラットな配列より読みにくい
- 「pane を別 window に移動」のような操作はツリー再構築が必要
- フローティング pane との混在は別途 `FloatRect` で管理（2 つの表現方式が並列）

## 代替案

- **代替案 A: グリッドレイアウト (N×M)**: シンプルだが、任意比率の分割や入れ子が困難
- **代替案 B: タイリング配列 (Zellij 流)**: フラット表現は読みやすいが、リサイズロジックが複雑
- **代替案 C: フローティングのみ**: 自由度高いが、tmux 互換性が失われる

## 参照

- `nexterm-server/src/window/bsp.rs` — BSP 分割アルゴリズム（`PaneRect` / `SplitDir`）
- `nexterm-server/src/window/tiling.rs` — タイリングレイアウトロジック
- `nexterm-server/src/window/floating.rs` — フローティング `FloatRect`
- `nexterm-server/src/window/tests.rs` — `bsp_split` レイアウトユニットテスト
- tmux 比較: https://github.com/tmux/tmux/wiki
