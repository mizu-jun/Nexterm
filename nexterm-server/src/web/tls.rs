//! TLS certificate loading and self-signed certificate auto-generation.

use std::path::PathBuf;

use tracing::info;

/// Return the nexterm config directory (the parent of `nexterm.toml`).
fn config_dir() -> PathBuf {
    nexterm_config::toml_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Return `(cert_pem_bytes, key_pem_bytes)`.
///
/// - When `cert_file` / `key_file` are specified: read the files.
/// - When unspecified: generate or reuse a self-signed certificate in `~/.config/nexterm/tls/`.
pub fn load_or_generate(
    cert_file: Option<&str>,
    key_file: Option<&str>,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    if let (Some(cert_path), Some(key_path)) = (cert_file, key_file) {
        let cert = std::fs::read(cert_path).map_err(|e| {
            anyhow::anyhow!("failed to read certificate file: {}: {}", cert_path, e)
        })?;
        let key = std::fs::read(key_path)
            .map_err(|e| anyhow::anyhow!("failed to read private key file: {}: {}", key_path, e))?;
        info!("TLS: loaded certificate from {} / {}", cert_path, key_path);
        return Ok((cert, key));
    }

    // Auto-generation path.
    let tls_dir = config_dir().join("tls");
    let cert_path = tls_dir.join("cert.pem");
    let key_path = tls_dir.join("key.pem");

    if cert_path.exists() && key_path.exists() {
        let cert = std::fs::read(&cert_path)?;
        let key = std::fs::read(&key_path)?;
        info!(
            "TLS: reusing existing self-signed certificate ({:?})",
            tls_dir
        );
        return Ok((cert, key));
    }

    // Generate a fresh self-signed certificate.
    info!("TLS: generating a self-signed certificate");
    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])?;

    let cert_pem = certified.cert.pem();
    let key_pem = certified.key_pair.serialize_pem();

    std::fs::create_dir_all(&tls_dir)?;
    std::fs::write(&cert_path, &cert_pem)?;
    // HIGH H-3: write the TLS private key with mode 0600 (unreadable by other users on the host).
    write_key_file_secure(&key_path, key_pem.as_bytes())?;

    info!(
        "TLS: saved self-signed certificate to {:?}. \
        To silence browser warnings, add the certificate to your system/browser trust store.",
        tls_dir
    );

    Ok((cert_pem.into_bytes(), key_pem.into_bytes()))
}

/// Write the TLS private key file with owner-only permissions (0600).
///
/// HIGH H-3 mitigation: do not rely on umask; explicitly set 0600 permissions.
/// On Windows, NTFS ACLs default to per-user access, so writes proceed as-is.
fn write_key_file_secure(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(content)?;
        f.sync_all()?;
        Ok(())
    }
    #[cfg(windows)]
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        f.write_all(content)?;
        f.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_returns_path() {
        let dir = config_dir();
        // config_dir must always return a valid path.
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn load_or_generate_creates_self_signed_when_no_files() {
        // When no existing certificate is found, a self-signed certificate must be generated.
        // Use a temp directory.
        let temp_dir = std::env::temp_dir().join("nexterm_tls_test");
        let cert_path = temp_dir.join("test_cert.pem");
        let key_path = temp_dir.join("test_key.pem");

        // Clean up beforehand.
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);

        // Generate the certificate.
        let result = load_or_generate(None, None);

        // Self-signed certificate generation must succeed.
        assert!(result.is_ok());

        let (cert, key) = result.unwrap();
        // Verify they are PEM-formatted (PEM header appears at the start).
        let cert_str = String::from_utf8_lossy(&cert);
        let key_str = String::from_utf8_lossy(&key);
        assert!(cert_str.contains("BEGIN CERTIFICATE"));
        assert!(key_str.contains("BEGIN")); // Either "RSA PRIVATE KEY" or "PRIVATE KEY".
    }

    #[test]
    fn load_or_generate_with_explicit_files() {
        // Provide explicit certificate file paths.
        let temp_dir = std::env::temp_dir().join("nexterm_tls_test_explicit");
        let cert_path = temp_dir.join("custom_cert.pem");
        let key_path = temp_dir.join("custom_key.pem");

        // Clean up beforehand.
        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = std::fs::create_dir_all(&temp_dir);

        // Write dummy certificate and key.
        let dummy_cert = b"-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----";
        let dummy_key = b"-----BEGIN PRIVATE KEY-----\ntest\n-----END PRIVATE KEY-----";

        std::fs::write(&cert_path, dummy_cert).unwrap();
        std::fs::write(&key_path, dummy_key).unwrap();

        let result = load_or_generate(
            Some(cert_path.to_str().unwrap()),
            Some(key_path.to_str().unwrap()),
        );

        assert!(result.is_ok());
        let (cert, key) = result.unwrap();
        assert_eq!(cert, dummy_cert);
        assert_eq!(key, dummy_key);

        // Cleanup.
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn load_or_generate_fails_with_missing_file() {
        // An explicit but missing file path must error.
        let result = load_or_generate(
            Some("/nonexistent/path/cert.pem"),
            Some("/nonexistent/path/key.pem"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn load_or_generate_single_path_falls_back_to_auto() {
        // When only the certificate path is given (no key), fall back to the auto-generation path.
        // The explicit path being missing still ends up generating a self-signed certificate.
        let result = load_or_generate(Some("/nonexistent/cert.pem"), None);
        // Falling back to auto-generation succeeds.
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn write_key_file_secure_uses_0600() {
        // HIGH H-3: the TLS private key must be stored with 0600 permissions.
        use std::os::unix::fs::PermissionsExt;
        let tmp =
            std::env::temp_dir().join(format!("nexterm_test_tls_key_{}.pem", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        write_key_file_secure(&tmp, b"-----BEGIN PRIVATE KEY-----\nfake\n").unwrap();
        let mode = std::fs::metadata(&tmp).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "TLS private key is not 0600: {:o}",
            mode & 0o777
        );

        std::fs::remove_file(&tmp).ok();
    }
}
