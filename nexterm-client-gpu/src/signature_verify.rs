//! Release-asset minisign signature verification (Sprint 3-4).
//!
//! Guarantees the integrity of release binaries during the auto-update flow.
//! The public key is embedded at build time via the `NEXTERM_MINISIGN_PUBLIC_KEY`
//! environment variable.
//!
//! # Operational flow
//!
//! 1. The release maintainer generates a key pair with `minisign -G -s nexterm.key`
//!    and stores the secret key in the `NEXTERM_MINISIGN_SECRET_KEY` GitHub Secret.
//! 2. The public key (the base64 line from `minisign.pub` excluding the
//!    `untrusted comment` header) is registered in the
//!    `NEXTERM_MINISIGN_PUBLIC_KEY` GitHub Variable.
//! 3. The release workflow passes `NEXTERM_MINISIGN_PUBLIC_KEY` as an environment
//!    variable at build time and signs each archive with `minisign -S -s ...`,
//!    attaching the resulting `.minisig`.
//! 4. After downloading an update, the client fetches the matching `.minisig`
//!    and validates it via [`verify_minisign`].
//!
//! # Skipping verification
//!
//! For builds that do not embed a public key (development builds, or when the
//! public key has not been published yet) [`is_signature_verification_enabled`]
//! returns `false`. Callers should surface the verification failure to the user
//! and abort the auto-update.
//!
//! # Note
//!
//! The public API is planned for the future auto-update download feature.
//! For now we only emit notifications, so `dead_code` is allowed.

#![allow(dead_code)]

use minisign_verify::{PublicKey, Signature};

/// The minisign public key embedded at build time (a single base64 line).
///
/// `NEXTERM_MINISIGN_PUBLIC_KEY` is expected to be set only for CI release builds.
/// In development builds it is `None`, and the verification function returns an
/// appropriate error.
pub const MINISIGN_PUBLIC_KEY: Option<&str> = option_env!("NEXTERM_MINISIGN_PUBLIC_KEY");

/// Whether signature verification is enabled (i.e. a public key is embedded).
pub fn is_signature_verification_enabled() -> bool {
    MINISIGN_PUBLIC_KEY.is_some_and(|s| !s.trim().is_empty())
}

/// Verify a minisign signature.
///
/// # Arguments
/// - `data`: bytes covered by the signature (the contents of the release archive).
/// - `signature_text`: full text contents of the `.minisig` file.
///
/// # Returns
/// - `Ok(())`: verification succeeded.
/// - `Err(...)`: public key not embedded / signature format invalid / verification failed.
pub fn verify_minisign(data: &[u8], signature_text: &str) -> anyhow::Result<()> {
    let pubkey_b64 = MINISIGN_PUBLIC_KEY
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "minisign public key is not embedded (set NEXTERM_MINISIGN_PUBLIC_KEY at release build time)"
            )
        })?;

    let public_key = PublicKey::from_base64(pubkey_b64)
        .map_err(|e| anyhow::anyhow!("failed to decode minisign public key: {e}"))?;

    let signature = Signature::decode(signature_text)
        .map_err(|e| anyhow::anyhow!("failed to decode minisign signature: {e}"))?;

    public_key
        .verify(data, &signature, false)
        .map_err(|e| anyhow::anyhow!("minisign signature verification failed: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test vectors from the minisign-verify official documentation (lib.rs).
    // A trusted (public key, signature, data) triple that exercises the verification logic.
    const TEST_PUBKEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1633700835\tfile:test\tprehashed
wLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";
    const TEST_DATA: &[u8] = b"test";

    #[test]
    fn official_test_vector_verifies() {
        let public_key = PublicKey::from_base64(TEST_PUBKEY).expect("public key decodes");
        let signature = Signature::decode(TEST_SIGNATURE).expect("signature decodes");
        public_key
            .verify(TEST_DATA, &signature, false)
            .expect("verification succeeds");
    }

    #[test]
    fn tampered_data_fails_verification() {
        let public_key = PublicKey::from_base64(TEST_PUBKEY).expect("public key decodes");
        let signature = Signature::decode(TEST_SIGNATURE).expect("signature decodes");
        let tampered = b"tampered data";
        assert!(public_key.verify(tampered, &signature, false).is_err());
    }

    #[test]
    fn invalid_public_key_fails_to_decode() {
        // Too-short base64 / not a minisign-formatted key.
        assert!(PublicKey::from_base64("invalid").is_err());
        assert!(PublicKey::from_base64("").is_err());
    }

    #[test]
    fn invalid_signature_fails_to_decode() {
        assert!(Signature::decode("not a signature").is_err());
        assert!(Signature::decode("").is_err());
    }

    #[test]
    fn verify_minisign_returns_err_for_invalid_signature() {
        // When the public key is not embedded we get a "public key not set" error;
        // when it is embedded we get a "signature decode failed" error.
        // Either way the call must return `Err`.
        let result = verify_minisign(b"data", "not a signature");
        assert!(result.is_err(), "invalid signatures must always return Err");
    }

    #[test]
    fn is_signature_verification_enabled_switches_on_env_var() {
        // The result is determined at build time by `option_env!`, so it cannot be
        // varied at run time. Just confirm the call returns a value without panicking.
        let _ = is_signature_verification_enabled();
    }
}
