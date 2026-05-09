//! TLS 証明書の読み込みと自己署名証明書の自動生成

use std::path::PathBuf;

use tracing::info;

/// nexterm 設定ディレクトリを返す（nexterm.toml の親ディレクトリ）
fn config_dir() -> PathBuf {
    nexterm_config::toml_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// (cert_pem_bytes, key_pem_bytes) を返す。
///
/// - `cert_file` / `key_file` が指定されている場合: そのファイルを読み込む。
/// - 未指定の場合: `~/.config/nexterm/tls/` に自己署名証明書を生成または再利用する。
pub fn load_or_generate(
    cert_file: Option<&str>,
    key_file: Option<&str>,
) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    if let (Some(cert_path), Some(key_path)) = (cert_file, key_file) {
        let cert = std::fs::read(cert_path)
            .map_err(|e| anyhow::anyhow!("証明書ファイルの読み込みに失敗: {}: {}", cert_path, e))?;
        let key = std::fs::read(key_path)
            .map_err(|e| anyhow::anyhow!("秘密鍵ファイルの読み込みに失敗: {}: {}", key_path, e))?;
        info!(
            "TLS: {} / {} から証明書を読み込みました",
            cert_path, key_path
        );
        return Ok((cert, key));
    }

    // 自動生成パス
    let tls_dir = config_dir().join("tls");
    let cert_path = tls_dir.join("cert.pem");
    let key_path = tls_dir.join("key.pem");

    if cert_path.exists() && key_path.exists() {
        let cert = std::fs::read(&cert_path)?;
        let key = std::fs::read(&key_path)?;
        info!("TLS: 既存の自己署名証明書を再利用します ({:?})", tls_dir);
        return Ok((cert, key));
    }

    // 新しい自己署名証明書を生成する
    info!("TLS: 自己署名証明書を生成します");
    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])?;

    let cert_pem = certified.cert.pem();
    let key_pem = certified.key_pair.serialize_pem();

    std::fs::create_dir_all(&tls_dir)?;
    std::fs::write(&cert_path, &cert_pem)?;
    // HIGH H-3: TLS 秘密鍵は 0600 で書き込む（同一サーバーの他ユーザーから読み取り不可）
    write_key_file_secure(&key_path, key_pem.as_bytes())?;

    info!(
        "TLS: 自己署名証明書を {:?} に保存しました。\
        ブラウザの警告を解消するにはこの証明書をシステム/ブラウザの信頼ストアに追加してください。",
        tls_dir
    );

    Ok((cert_pem.into_bytes(), key_pem.into_bytes()))
}

/// TLS 秘密鍵ファイルを所有者限定（0600）で書き込む。
///
/// HIGH H-3 対策: umask に依存せず確実にパーミッションを 0600 に設定する。
/// Windows では NTFS ACL がデフォルトでユーザー固有のためそのまま書き込む。
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
        // config_dirは必ず有効なパスを返す
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn load_or_generate_creates_self_signed_when_no_files() {
        // 既存の証明書がない場合、自己署名証明書が生成される
        // 一時ディレクトリを使用
        let temp_dir = std::env::temp_dir().join("nexterm_tls_test");
        let cert_path = temp_dir.join("test_cert.pem");
        let key_path = temp_dir.join("test_key.pem");

        // 事前にクリーンアップ
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);

        // 証明書を生成
        let result = load_or_generate(None, None);

        // 自己署名証明書の生成に成功する
        assert!(result.is_ok());

        let (cert, key) = result.unwrap();
        // PEM形式であることを確認（先頭にPEMヘッダーがある）
        let cert_str = String::from_utf8_lossy(&cert);
        let key_str = String::from_utf8_lossy(&key);
        assert!(cert_str.contains("BEGIN CERTIFICATE"));
        assert!(key_str.contains("BEGIN")); // RSA PRIVATE KEY または PRIVATE KEY
    }

    #[test]
    fn load_or_generate_with_explicit_files() {
        // 明示的な証明書ファイルパスを指定
        let temp_dir = std::env::temp_dir().join("nexterm_tls_test_explicit");
        let cert_path = temp_dir.join("custom_cert.pem");
        let key_path = temp_dir.join("custom_key.pem");

        // 事前にクリーンアップ
        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = std::fs::create_dir_all(&temp_dir);

        // ダミーの証明書と鍵を書き込む
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

        // クリーンアップ
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn load_or_generate_fails_with_missing_file() {
        // 存在しないファイルを指定した場合はエラー
        let result = load_or_generate(
            Some("/nonexistent/path/cert.pem"),
            Some("/nonexistent/path/key.pem"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn load_or_generate_single_path_falls_back_to_auto() {
        // 証明書のみ指定、鍵が未指定の場合は自動生成パスにフォールバック
        // 明示的パスが存在しない場合も自己署名証明書が生成される
        let result = load_or_generate(Some("/nonexistent/cert.pem"), None);
        // 片方だけ指定の場合は自動生成パスにフォールバック
        // 存在しない場合は自己署名証明書が生成される
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn write_key_file_secure_は_0600_で書き込む() {
        // HIGH H-3: TLS 秘密鍵が 0600 パーミッションで保存されることを保証
        use std::os::unix::fs::PermissionsExt;
        let tmp =
            std::env::temp_dir().join(format!("nexterm_test_tls_key_{}.pem", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        write_key_file_secure(&tmp, b"-----BEGIN PRIVATE KEY-----\nfake\n").unwrap();
        let mode = std::fs::metadata(&tmp).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "TLS 秘密鍵が 0600 ではない: {:o}",
            mode & 0o777
        );

        std::fs::remove_file(&tmp).ok();
    }
}
