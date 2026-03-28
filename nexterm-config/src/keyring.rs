//! SSH 認証情報の安全な保存 — OS キーチェーン統合

use anyhow::Result;
use zeroize::Zeroizing;

const SERVICE: &str = "nexterm-ssh";

/// SSH ホストのパスワードを OS キーチェーンに保存する
pub fn store_password(host_name: &str, username: &str, password: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, &format!("{}@{}", username, host_name))?;
    entry.set_password(password)?;
    Ok(())
}

/// SSH ホストのパスワードを OS キーチェーンから取得する
pub fn get_password(host_name: &str, username: &str) -> Result<Zeroizing<String>> {
    let entry = keyring::Entry::new(SERVICE, &format!("{}@{}", username, host_name))?;
    Ok(Zeroizing::new(entry.get_password()?))
}

/// SSH ホストのパスワードを OS キーチェーンから削除する
pub fn delete_password(host_name: &str, username: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, &format!("{}@{}", username, host_name))?;
    entry.delete_credential()?;
    Ok(())
}
