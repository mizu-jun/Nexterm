# Security Policy

## 既知の脆弱性（Known Vulnerabilities）

### 高重大度（High Severity）

#### 1. rustls-webpki v0.103.13 - DoS via malformed CRL
- **CVE/GHSA**: [依存関係で追跡中]
- **影響範囲**: TLS接続時の証明書検証
- **状況**: rustls 0.23系の依存関係により固定
- **対応予定**: rustls-webpki v0.104安定版リリース待機
- **緩和策**: 
  - 信頼できるネットワークのみでの使用
  - クライアント証明書pinningの検討

#### 2. russh v0.59.0 - Pre-auth unbounded allocation
- **CVE/GHSA**: [依存関係で追跡中]
- **影響範囲**: SSH接続時のキーボード対話認証
- **状況**: v0.60系への更新は破壊的変更を伴う
- **対応予定**: 該当認証方式未使用時は影響限定
- **緩和策**:
  - パスワード/鍵認証のみ使用
  - キーボード対話認証の無効化

### 低重大度（Low Severity）

#### lru crate - Stacked Borrows violation
- **影響範囲**: 内部イテレータ使用時のみ
- **評価**: 実用上の安全性は確保

## 依存関係の監視

Dependabotアラートは以下で確認：
https://github.com/mizu-jun/Nexterm/security/dependabot

## 更新ポリシー

| 重大度 | 対応期限 | 対応方法 |
|--------|----------|----------|
| Critical | 即時 | 緊急パッチまたはfork |
| High | 30日以内 | upstream追跡・fork検討 |
| Medium | 90日以内 | 通常更新サイクル |
| Low | 次回リリース | 許容または追跡 |

## 脆弱性レポート

脆弱性の報告は GitHub Security Advisories を通じて行ってください。
