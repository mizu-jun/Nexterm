//! Test utilities — shared test helpers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use tempfile::TempDir;

static TEST_PORT: AtomicU16 = AtomicU16::new(9000);

/// Generate a unique path for tests.
pub fn temp_json_path(name: &str) -> PathBuf {
    let temp = TempDir::new().expect("failed to create temp directory");
    temp.path()
        .join(format!("{}_{}.json", name, std::process::id()))
}

/// Temporarily set an environment variable for the duration of the test; automatically cleaned
/// up at scope end.
pub struct TempEnvVar {
    key: String,
    original: Option<String>,
}

impl TempEnvVar {
    /// Set an environment variable temporarily.
    pub fn set(key: &str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key: key.to_string(),
            original,
        }
    }
}

impl Drop for TempEnvVar {
    fn drop(&mut self) {
        if let Some(ref val) = self.original {
            unsafe {
                std::env::set_var(&self.key, val);
            }
        } else {
            unsafe {
                std::env::remove_var(&self.key);
            }
        }
    }
}

/// Manage a test temporary file path with automatic cleanup.
pub struct TempFile {
    /// Path to the temporary file (the caller writes the contents).
    pub path: PathBuf,
    _temp_dir: TempDir,
}

impl TempFile {
    /// Generate a `<name>_<pid>.json` path under a temp directory (file is not created).
    pub fn new(name: &str) -> Self {
        let temp_dir = TempDir::new().expect("failed to create temp directory");
        let path = temp_dir
            .path()
            .join(format!("{}_{}.json", name, std::process::id()));
        Self {
            path,
            _temp_dir: temp_dir,
        }
    }
}

/// Return a unique port number for tests.
pub fn next_test_port() -> u16 {
    TEST_PORT.fetch_add(1, Ordering::SeqCst)
}

/// A temp directory with test-scoped lifetime.
pub fn temp_dir() -> TempDir {
    TempDir::new().expect("failed to create temp directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_creates_and_cleans_up() {
        let path = {
            let temp = TempFile::new("test");
            let p = temp.path.clone();
            assert!(!p.exists());
            std::fs::write(&p, "test data").unwrap();
            assert!(p.exists());
            p
        };
        assert!(!path.exists());
    }

    #[test]
    fn temp_env_var_restores_original() {
        let key = "___TEST_VAR___";
        unsafe {
            std::env::remove_var(key);
        }

        {
            let _env = TempEnvVar::set(key, "test_value");
            assert_eq!(std::env::var(key).unwrap(), "test_value");
        }

        assert!(std::env::var(key).is_err());
    }

    #[test]
    fn unique_test_ports() {
        let p1 = next_test_port();
        let p2 = next_test_port();
        assert_ne!(p1, p2);
    }
}
