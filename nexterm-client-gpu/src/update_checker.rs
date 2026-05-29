//! Background update checker.
//!
//! Waits 5 s after startup, then polls the GitHub Releases API. If a release
//! newer than the running version is available, it is published over a
//! `tokio::sync::watch` channel.
//!
//! Sprint 3-4: release-asset verification during automatic download relies on
//! [`verify_minisign`] from the [`crate::signature_verify`] module. The
//! notification path itself does not download anything, so the verification
//! happens only inside [`download_and_verify_asset`] when the download is
//! actually performed.
//!
//! [`verify_minisign`]: crate::signature_verify::verify_minisign

use crate::signature_verify;
use tokio::sync::watch;
use tracing::{info, warn};

/// Minimal subset of the GitHub Releases API response.
#[derive(serde::Deserialize)]
struct GhRelease {
    tag_name: String,
}

/// Start the background update check.
///
/// Returns a `watch::Receiver` that receives the latest version string (e.g.
/// `"0.9.15"`). If `auto_check_update` is false, the channel is left at `None`
/// and the receiver is returned immediately.
pub fn start(current_version: &str, enabled: bool) -> watch::Receiver<Option<String>> {
    let (tx, rx) = watch::channel(None);

    if !enabled {
        return rx;
    }

    let current = current_version.to_string();
    tokio::spawn(async move {
        // Wait 5 s to avoid competing with the startup-time resource burst.
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        match fetch_latest_version().await {
            Ok(latest) if is_newer(&latest, &current) => {
                info!("a new version is available: v{}", latest);
                let _ = tx.send(Some(latest));
            }
            Ok(latest) => {
                info!(
                    "running the latest version v{} (no update required)",
                    latest
                );
            }
            Err(e) => {
                warn!("update check failed: {}", e);
            }
        }
    });

    rx
}

/// Fetch the tag of the most recent release from the GitHub Releases API.
async fn fetch_latest_version() -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("nexterm/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let release: GhRelease = client
        .get("https://api.github.com/repos/mizu-jun/nexterm/releases/latest")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Strip the leading "v" (e.g. "v0.9.15" → "0.9.15").
    Ok(release.tag_name.trim_start_matches('v').to_string())
}

/// Download the release asset at the given URL plus its `.minisig`, verify the
/// minisign signature, and return the asset bytes.
///
/// Returns `Err` for development builds that do not embed a public key
/// (use [`signature_verify::is_signature_verification_enabled`] for an upfront check).
///
/// # Arguments
/// - `asset_url`: direct link to the release archive (e.g.
///   `nexterm-v1.0.0-linux-x86_64.tar.gz`).
///
/// # Returns
/// - `Ok(bytes)`: the verified archive bytes.
/// - `Err(...)`: download failure / public key missing / signature verification failed.
#[allow(dead_code)] // Sprint 3-4: scheduled for use by the future auto-update flow.
pub async fn download_and_verify_asset(asset_url: &str) -> anyhow::Result<Vec<u8>> {
    if !signature_verify::is_signature_verification_enabled() {
        anyhow::bail!(
            "aborting auto-update: minisign public key is not embedded (set NEXTERM_MINISIGN_PUBLIC_KEY for release builds)"
        );
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("nexterm/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let bytes = client
        .get(asset_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();

    let sig_url = format!("{}.minisig", asset_url);
    let signature_text = client
        .get(&sig_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    signature_verify::verify_minisign(&bytes, &signature_text)?;
    info!(
        "minisign signature verified: {} ({} bytes)",
        asset_url,
        bytes.len()
    );
    Ok(bytes)
}

/// Decide whether `latest` is semver-newer than `current`.
/// Returns false on parse failure (fail-safe).
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() < 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].split('-').next()?.parse().ok()?,
        ))
    };

    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_true() {
        assert!(is_newer("0.9.15", "0.9.14"));
        assert!(is_newer("1.0.0", "0.9.99"));
        assert!(is_newer("0.10.0", "0.9.99"));
    }

    #[test]
    fn test_is_newer_false() {
        assert!(!is_newer("0.9.14", "0.9.14")); // identical
        assert!(!is_newer("0.9.13", "0.9.14")); // older
    }

    #[test]
    fn test_is_newer_prerelease_suffix() {
        // Suffixes like "-beta" are ignored; only the patch number is compared.
        assert!(is_newer("0.9.15", "0.9.14-beta"));
        assert!(!is_newer("0.9.14-beta", "0.9.14"));
    }

    #[test]
    fn test_is_newer_invalid() {
        assert!(!is_newer("invalid", "0.9.14"));
        assert!(!is_newer("0.9.15", "not-semver"));
    }
}
