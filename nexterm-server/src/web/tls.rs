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
        info!("TLS: {} / {} から証明書を読み込みました", cert_path, key_path);
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
    let certified = rcgen::generate_simple_self_signed(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ])?;

    let cert_pem = certified.cert.pem();
    let key_pem = certified.key_pair.serialize_pem();

    std::fs::create_dir_all(&tls_dir)?;
    std::fs::write(&cert_path, &cert_pem)?;
    std::fs::write(&key_path, &key_pem)?;

    info!(
        "TLS: 自己署名証明書を {:?} に保存しました。\
        ブラウザの警告を解消するにはこの証明書をシステム/ブラウザの信頼ストアに追加してください。",
        tls_dir
    );

    Ok((cert_pem.into_bytes(), key_pem.into_bytes()))
}
