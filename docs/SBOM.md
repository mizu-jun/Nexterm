# SBOM (Software Bill of Materials) 運用ガイド

> Sprint 4-3 で導入。CycloneDX 形式の SBOM をリリースごとに生成し、GitHub Release に添付する。

## 目的

- サプライチェーン透明性: ユーザーが nexterm に組み込まれている全依存パッケージとそのバージョン・ライセンスを機械処理可能な形式で取得できる
- 脆弱性追跡: 後日新たな CVE が公開された場合、SBOM をスキャンして影響範囲を特定可能
- コンプライアンス: ISO 27001 / SLSA L2 相当の依存関係台帳要件を満たす
- 監査: 第三者が公開された SBOM と実バイナリを比較できる

## 形式

[CycloneDX](https://cyclonedx.org/) JSON v1.5 を採用する。理由:

- Rust エコシステム標準（`cargo-cyclonedx` が公式メンテナンス）
- セキュリティツール（OSV Scanner / Trivy / Dependency-Track 等）の対応が広い
- SBOM 全文が JSON で表現され、機械処理しやすい

SPDX 形式が必要な場合は `cyclonedx-cli` で相互変換可能。

## 生成タイミング

| トリガー | 動作 |
|---------|------|
| `v[0-9]+.[0-9]+.[0-9]+` タグ push | `.github/workflows/sbom.yml` が起動。SBOM 生成 → SLSA Provenance 付与 → GitHub Release に `.tar.gz` を添付 |
| `workflow_dispatch` 手動実行 | SBOM 生成のみ（artifact として 90 日保持、Release への添付なし） |

## 添付ファイル

リリースには `nexterm-sbom-vX.Y.Z.tar.gz` という単一アーカイブを添付する。展開すると workspace 内 12 クレートそれぞれの SBOM が並ぶ:

```
nexterm-sbom-v1.0.2/
  nexterm.cdx.json                     # nexterm-launcher (バイナリ名 nexterm)
  nexterm-server.cdx.json
  nexterm-client-gpu.cdx.json
  nexterm-client-tui.cdx.json
  nexterm-client-core.cdx.json
  nexterm-ctl.cdx.json
  nexterm-config.cdx.json
  nexterm-vt.cdx.json
  nexterm-proto.cdx.json
  nexterm-i18n.cdx.json
  nexterm-ssh.cdx.json
  nexterm-plugin.cdx.json
```

## ローカル生成手順

```bash
# 初回のみ
cargo install cargo-cyclonedx --locked

# 全クレートの SBOM を JSON で生成
cargo cyclonedx --all --format json

# 各クレートディレクトリ直下に <crate-name>.cdx.json が出力される
find . -name '*.cdx.json' -not -path './target/*'
```

## 検証手順

### 1. ファイル整合性

リリースアーカイブには SLSA Provenance が付与されている。`gh` CLI で検証:

```bash
gh attestation verify nexterm-sbom-v1.0.2.tar.gz -R mizu-jun/Nexterm
```

minisign 公開鍵でも署名されている場合（運用後）:

```bash
minisign -V -p nexterm.pub -m nexterm-sbom-v1.0.2.tar.gz -x nexterm-sbom-v1.0.2.tar.gz.minisig
```

### 2. SBOM 内容確認

CycloneDX JSON を任意のツールで解析できる:

```bash
# jq で全コンポーネント名を抽出
jq -r '.components[].purl' nexterm-server.cdx.json

# 特定 crate のバージョン確認
jq '.components[] | select(.name == "ring")' nexterm-server.cdx.json
```

### 3. 既知脆弱性スキャン

[OSV Scanner](https://github.com/google/osv-scanner) で SBOM を直接スキャン:

```bash
osv-scanner --sbom=nexterm-server.cdx.json
```

[Trivy](https://github.com/aquasecurity/trivy) でも可:

```bash
trivy sbom nexterm-server.cdx.json
```

### 4. ライセンスサマリ

```bash
jq -r '.components[] | "\(.name)\t\(.version)\t\(.licenses[]?.license.id // .licenses[]?.license.name // "Unknown")"' nexterm-server.cdx.json | sort -u
```

## CI ガードレール

リリース以外の PR でも依存ポリシーは `cargo-deny` で常時チェックされている（`deny.toml` 参照）。SBOM はリリース時の証跡として機能し、`cargo-deny` は変更時の事前検出として機能する 2 段構えの設計。

| ツール | タイミング | 役割 |
|--------|----------|------|
| `cargo-deny` (`.github/workflows/ci.yml` の `deny` ジョブ) | PR / push ごと | ライセンス違反・既知脆弱性・不審ソースを事前ブロック |
| `cargo-audit` (同 `security` ジョブ) | PR / push ごと | RustSec Advisory DB との照合 |
| `cargo-cyclonedx` (`.github/workflows/sbom.yml`) | リリースタグ時 | SBOM 生成 + リリース添付 |
| SLSA Provenance | リリースタグ時 | ビルド出所の改ざん検出 |
| `minisign` | リリースタグ時（鍵設定時のみ） | アーカイブの完全性検証 |

## トラブルシューティング

### `cargo cyclonedx` が依存解決に失敗する

ネイティブライブラリ（X11 / ALSA / libudev 等）が解決できないため。CI と同じ Linux 開発依存をインストール:

```bash
sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev libudev-dev
```

### SBOM のサイズが大きい

workspace 全クレートで 12 ファイル × 数百 KB 程度。圧縮済みの `.tar.gz` は 100〜300 KB に収まる。明らかに肥大化した場合は重複バージョン（`cargo deny check bans` で検出）が増えていないか確認すること。

### `cargo cyclonedx` のバージョン互換性

CycloneDX 仕様 v1.5 出力は `cargo-cyclonedx` 0.5.0 以降が対応。CI では `cargo install --locked` でロック済みバージョンを取得するため、`Cargo.lock` を介して再現性を担保している。

## 参考資料

- [CycloneDX 仕様](https://cyclonedx.org/specification/overview/)
- [SLSA Build Provenance](https://slsa.dev/spec/v1.0/provenance)
- [OSV Scanner ドキュメント](https://google.github.io/osv-scanner/)
- [RustSec Advisory Database](https://rustsec.org/)
