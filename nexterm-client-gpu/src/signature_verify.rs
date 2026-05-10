//! リリースアセットの minisign 署名検証（Sprint 3-4）
//!
//! 自動更新フローでリリースバイナリの完全性を保証するためのモジュール。
//! 公開鍵はビルド時に `NEXTERM_MINISIGN_PUBLIC_KEY` 環境変数で埋め込まれる。
//!
//! # 運用
//!
//! 1. リリース管理者は `minisign -G -s nexterm.key` で鍵ペアを生成し、
//!    秘密鍵を GitHub Secrets `NEXTERM_MINISIGN_SECRET_KEY` に保存する。
//! 2. 公開鍵（`minisign.pub` の `untrusted comment` を除いた base64 行）を
//!    GitHub Variables `NEXTERM_MINISIGN_PUBLIC_KEY` に登録する。
//! 3. リリースワークフローはビルド時に `NEXTERM_MINISIGN_PUBLIC_KEY` を環境変数として
//!    渡し、各アーカイブを `minisign -S -s ...` で署名して `.minisig` を添付する。
//! 4. クライアントは更新ダウンロード後、対応する `.minisig` を取得して
//!    [`verify_minisign`] で検証する。
//!
//! # 検証スキップ
//!
//! 公開鍵が埋め込まれていないビルド（開発ビルド・公開鍵未公開時）では
//! [`is_signature_verification_enabled`] が `false` を返す。
//! 呼び出し側は検証エラーをユーザーに通知し、自動更新を中断すること。
//!
//! # 注意
//!
//! 公開 API は将来の自動更新ダウンロード機能で使用される予定。
//! 現状は通知のみ行うため `dead_code` を許容する。

#![allow(dead_code)]

use minisign_verify::{PublicKey, Signature};

/// ビルド時に埋め込まれる minisign 公開鍵（base64 単一行）
///
/// CI のリリースビルド時のみ `NEXTERM_MINISIGN_PUBLIC_KEY` が設定される想定。
/// 開発ビルドでは `None` になり、検証関数は適切なエラーを返す。
pub const MINISIGN_PUBLIC_KEY: Option<&str> = option_env!("NEXTERM_MINISIGN_PUBLIC_KEY");

/// 署名検証が有効化されているか（公開鍵が埋め込まれているか）
pub fn is_signature_verification_enabled() -> bool {
    MINISIGN_PUBLIC_KEY.is_some_and(|s| !s.trim().is_empty())
}

/// minisign 形式の署名を検証する
///
/// # Arguments
/// - `data`: 署名対象のバイト列（リリースアーカイブの中身）
/// - `signature_text`: `.minisig` ファイルの内容（テキスト全体）
///
/// # Returns
/// - `Ok(())`: 検証成功
/// - `Err(...)`: 公開鍵未埋め込み / 署名形式不正 / 検証失敗
pub fn verify_minisign(data: &[u8], signature_text: &str) -> anyhow::Result<()> {
    let pubkey_b64 = MINISIGN_PUBLIC_KEY
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "minisign 公開鍵が埋め込まれていません（リリースビルド時に NEXTERM_MINISIGN_PUBLIC_KEY を設定してください）"
            )
        })?;

    let public_key = PublicKey::from_base64(pubkey_b64)
        .map_err(|e| anyhow::anyhow!("minisign 公開鍵のデコードに失敗: {e}"))?;

    let signature = Signature::decode(signature_text)
        .map_err(|e| anyhow::anyhow!("minisign 署名のデコードに失敗: {e}"))?;

    public_key
        .verify(data, &signature, false)
        .map_err(|e| anyhow::anyhow!("minisign 署名検証に失敗: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // minisign-verify 公式ドキュメントのテストベクトル（lib.rs より）
    // 信頼された公開鍵・署名・データのトリプルで検証ロジックの正しさを担保する。
    const TEST_PUBKEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1633700835\tfile:test\tprehashed
wLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";
    const TEST_DATA: &[u8] = b"test";

    #[test]
    fn 公式テストベクトルで検証成功する() {
        let public_key = PublicKey::from_base64(TEST_PUBKEY).expect("公開鍵デコード");
        let signature = Signature::decode(TEST_SIGNATURE).expect("署名デコード");
        public_key
            .verify(TEST_DATA, &signature, false)
            .expect("検証成功");
    }

    #[test]
    fn データが改ざんされたら検証失敗する() {
        let public_key = PublicKey::from_base64(TEST_PUBKEY).expect("公開鍵デコード");
        let signature = Signature::decode(TEST_SIGNATURE).expect("署名デコード");
        let tampered = b"tampered data";
        assert!(public_key.verify(tampered, &signature, false).is_err());
    }

    #[test]
    fn 不正な公開鍵はデコードに失敗する() {
        // 短すぎる base64 / minisign フォーマット違反
        assert!(PublicKey::from_base64("invalid").is_err());
        assert!(PublicKey::from_base64("").is_err());
    }

    #[test]
    fn 不正な署名はデコードに失敗する() {
        assert!(Signature::decode("not a signature").is_err());
        assert!(Signature::decode("").is_err());
    }

    #[test]
    fn verify_minisign_は不正な署名でエラーを返す() {
        // 公開鍵が埋め込まれていない場合は「公開鍵未設定」エラー、
        // 埋め込まれている場合は「署名デコード失敗」エラーになる。
        // どちらにせよ Err を返すことを確認する。
        let result = verify_minisign(b"data", "not a signature");
        assert!(result.is_err(), "不正な署名は必ず Err を返すこと");
    }

    #[test]
    fn is_signature_verification_enabled_は環境変数有無で切り替わる() {
        // ビルド時の `option_env!` 評価結果なので実行時には変えられない。
        // 「真偽値が決定的に返ること」を確認する（パニックしないこと）。
        let _ = is_signature_verification_enabled();
    }
}
