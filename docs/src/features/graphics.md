# Graphics — Sixel & Kitty プロトコル

Nexterm は **Sixel** と **Kitty Graphics Protocol** の両方をサポートしており、
ターミナル内で直接画像を表示できます。

## Sixel

[Sixel](https://en.wikipedia.org/wiki/Sixel) は DEC 社が開発した画像フォーマットです。
多くの CLIツールが Sixel 出力をサポートしています。

### 動作確認方法

```bash
# img2sixel をインストール（libsixel）
brew install libsixel   # macOS
apt install libsixel-bin # Ubuntu

# 画像を表示
img2sixel ~/Pictures/photo.jpg

# viu（Rust 製）でも可能
cargo install viu
viu ~/Pictures/photo.jpg
```

### 対応エスケープシーケンス

```
DCS P1;P2;P3 q <sixel_data> ST
```

- `P1`: 縦横比（通常 0 または省略）
- `P3`: 背景色の扱い（0=背景透過, 1=背景維持）

---

## Kitty Graphics Protocol

[Kitty Graphics Protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) は
より高機能な画像プロトコルです。Base64 エンコードされた RGBA/RGB データを送信します。

### 動作確認方法

```bash
# Kitty プロトコル対応ツール
pip install ranger-fm  # ファイルマネージャ
cargo install termpdf  # PDF ビューア

# Python による手動テスト
python3 - <<'EOF'
import base64, sys

def show_image(path):
    with open(path, 'rb') as f:
        data = f.read()
    encoded = base64.standard_b64encode(data).decode()
    # Kitty プロトコル送信（形式: f=32 RGBA, s=幅, v=高さ）
    sys.stdout.buffer.write(
        b'\x1b_Ga=T,f=32,s=100,v=100;' + encoded.encode() + b'\x1b\\'
    )
    sys.stdout.flush()
EOF
```

### 対応パラメータ

| パラメータ | 説明 |
|-----------|------|
| `a=T` | 送信アクション（Transmit）|
| `f=32` | RGBA 8-bit フォーマット |
| `f=24` | RGB 8-bit フォーマット（自動で RGBA に変換）|
| `s=<width>` | 画像幅（ピクセル）|
| `v=<height>` | 画像高さ（ピクセル）|
| `m=1` | 分割転送（複数チャンクで送信可能）|

---

## パフォーマンス

- 画像は GPU テクスチャにキャッシュされます（`image_id` で管理）
- 同じ画像 ID の再描画はテクスチャ再生成なしで処理されます
- 大きな画像（4K 以上）は転送に時間がかかる場合があります

---

## 既知の制限

- Sixel の HLS カラーモデルは簡易変換（白色近似）
- アニメーション Sixel は未対応
- Kitty の `a=p`（表示のみ、転送なし）は未対応
