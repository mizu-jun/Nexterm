//! Secure storage for SSH credentials — OS keychain integration.

use anyhow::Result;
use zeroize::Zeroizing;

const SERVICE: &str = "nexterm-ssh";

/// Stores an SSH host password in the OS keychain.
pub fn store_password(host_name: &str, username: &str, password: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, &format!("{}@{}", username, host_name))?;
    entry.set_password(password)?;
    Ok(())
}

/// Retrieves an SSH host password from the OS keychain.
pub fn get_password(host_name: &str, username: &str) -> Result<Zeroizing<String>> {
    let entry = keyring::Entry::new(SERVICE, &format!("{}@{}", username, host_name))?;
    Ok(Zeroizing::new(entry.get_password()?))
}

/// Deletes an SSH host password from the OS keychain.
pub fn delete_password(host_name: &str, username: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, &format!("{}@{}", username, host_name))?;
    entry.delete_credential()?;
    Ok(())
}
