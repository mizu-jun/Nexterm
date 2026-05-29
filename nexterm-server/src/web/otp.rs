//! TOTP (Time-based One-Time Password) — OTP authentication per RFC 6238.
//!
//! The secret is stored in Base32 form under the `[web.auth]` section of `nexterm.toml`.
//! When the secret is not configured at first boot, setup is performed in the browser.
//!
//! # Replay defense (CRITICAL #6)
//!
//! The same OTP code is not accepted more than once within ±1 window (up to 90 seconds).
//! Prevents an attacker who captured a code via network eavesdropping, screen recording, or
//! shoulder-surfing from logging in multiple times with the same code.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use subtle::ConstantTimeEq;
use totp_rs::{Algorithm, Secret, TOTP};

/// TOTP step (seconds). RFC 6238 standard value is 30 seconds.
const TOTP_STEP: u64 = 30;

/// Maximum window range over which used OTPs are retained (currently ±2 windows = past 90 seconds).
///
/// Allowing OTPs within `±1`, retaining out to `±2` keeps GC of older entries safe.
const REPLAY_WINDOW_RANGE: i64 = 2;

/// TOTP manager — holds the secret and the TOTP instance.
pub struct TotpManager {
    totp: TOTP,
    /// Base32-encoded secret (for display and persistence).
    secret_b32: String,
    /// Used (window_id, code) pairs. Prevents replay attacks.
    used_codes: Mutex<HashSet<(u64, String)>>,
}

impl TotpManager {
    /// Construct from a Base32 secret in the configuration file.
    pub fn from_secret(secret_b32: &str, issuer: &str) -> anyhow::Result<Self> {
        let secret = Secret::Encoded(secret_b32.to_uppercase());
        let totp = TOTP::new(
            Algorithm::SHA1,
            6,
            1,
            30,
            secret
                .to_bytes()
                .map_err(|e| anyhow::anyhow!("invalid TOTP secret: {}", e))?,
            Some(issuer.to_string()),
            "web-terminal".to_string(),
        )
        .map_err(|e| anyhow::anyhow!("TOTP initialization error: {}", e))?;

        Ok(Self {
            totp,
            secret_b32: secret_b32.to_uppercase(),
            used_codes: Mutex::new(HashSet::new()),
        })
    }

    /// Generate a cryptographically secure random secret (Base32 string).
    pub fn generate_secret() -> String {
        Secret::generate_secret().to_string()
    }

    /// Return the otpauth:// URL used for QR code generation.
    pub fn get_url(&self) -> String {
        self.totp.get_url()
    }

    /// Return the Base32 secret string for storage / display.
    pub fn secret_b32(&self) -> &str {
        &self.secret_b32
    }

    /// Verify a 6-digit OTP code against the current time window.
    ///
    /// On success, records into the used-codes set and rejects further reuse of the same
    /// `(window, code)`.
    /// Allows ±1 window (up to 90 seconds) and detects replays inside that range.
    pub fn verify(&self, code: &str) -> bool {
        let code = code.trim();
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }

        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs(),
            Err(_) => return false,
        };
        let current_window = (now / TOTP_STEP) as i64;

        // Try ±1 windows (RFC 6238 allows for clock drift).
        for offset in [-1_i64, 0, 1] {
            let window = current_window + offset;
            if window < 0 {
                continue;
            }
            let window = window as u64;
            let unix_time = window * TOTP_STEP;

            let expected = match self.totp.generate(unix_time) {
                s if s.len() == code.len() => s,
                _ => continue,
            };

            // Constant-time comparison (defense against timing attacks).
            if expected.as_bytes().ct_eq(code.as_bytes()).unwrap_u8() == 1 {
                return self.try_record_use(window, expected, current_window);
            }
        }
        false
    }

    /// Record `(window, code)` in the used set. Rejects (replay) if already present.
    /// GCs entries from older windows.
    fn try_record_use(&self, window: u64, code: String, current_window: i64) -> bool {
        let mut used = match self.used_codes.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!("TOTP used_codes mutex is poisoned; recovering and continuing");
                poisoned.into_inner()
            }
        };

        if !used.insert((window, code)) {
            tracing::warn!(
                "TOTP replay attack detected: code for window={} was reused",
                window
            );
            return false;
        }

        // GC: delete entries older than `REPLAY_WINDOW_RANGE` from the current window.
        let cutoff = current_window.saturating_sub(REPLAY_WINDOW_RANGE);
        used.retain(|(w, _)| (*w as i64) >= cutoff);

        true
    }
}

/// Write the TOTP secret to `nexterm.toml`.
///
/// Creates the `[web]` / `[web.auth]` tables if missing.
pub fn save_secret_to_config(secret_b32: &str) -> anyhow::Result<()> {
    use toml_edit::DocumentMut;

    let path = nexterm_config::toml_path();

    let content = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };

    let mut doc = content
        .parse::<DocumentMut>()
        .unwrap_or_else(|_| DocumentMut::new());

    // Create `[web]` if it does not exist.
    if !doc.contains_table("web") {
        doc["web"] = toml_edit::table();
    }

    // Create `[web.auth]` if it does not exist.
    {
        let web = doc["web"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("[web] is not a table"))?;
        if !web.contains_key("auth") {
            web["auth"] = toml_edit::table();
        }
        web["auth"]["totp_secret"] = toml_edit::value(secret_b32);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, doc.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_generation_is_unique() {
        let secret1 = TotpManager::generate_secret();
        let secret2 = TotpManager::generate_secret();
        assert_ne!(secret1, secret2);
    }

    #[test]
    fn generate_secret_returns_non_empty() {
        let secret = TotpManager::generate_secret();
        assert!(!secret.is_empty());
    }

    #[test]
    fn from_secret_invalid_base32_fails() {
        let invalid_secret = "INVALID!@#$";
        let result = TotpManager::from_secret(invalid_secret, "TestIssuer");
        assert!(result.is_err());
    }

    /// Construct a TotpManager from a valid Base32 secret for tests.
    fn test_manager() -> TotpManager {
        // 32 characters = 160 bits of Base32 (RFC 6238 recommends >= 128 bits).
        TotpManager::from_secret("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP", "TestIssuer")
            .expect("valid secret")
    }

    #[test]
    fn rejects_malformed_codes() {
        let mgr = test_manager();
        assert!(!mgr.verify(""));
        assert!(!mgr.verify("12345")); // 5 digits
        assert!(!mgr.verify("1234567")); // 7 digits
        assert!(!mgr.verify("12345a")); // contains non-digit
        assert!(!mgr.verify("abcdef")); // all letters
    }

    #[test]
    fn accepts_correct_code_on_first_use() {
        let mgr = test_manager();
        // Generate a code for the current window.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let window = now / TOTP_STEP;
        let code = mgr.totp.generate(window * TOTP_STEP);

        assert!(
            mgr.verify(&code),
            "the current-window code should be accepted"
        );
    }

    #[test]
    fn rejects_replay_attacks() {
        // CRITICAL #6: reusing the same OTP code multiple times must be rejected.
        let mgr = test_manager();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let window = now / TOTP_STEP;
        let code = mgr.totp.generate(window * TOTP_STEP);

        // 1st: accepted.
        assert!(mgr.verify(&code), "the first attempt should be accepted");

        // 2nd (replay): rejected.
        assert!(
            !mgr.verify(&code),
            "the second use of the same code should be rejected as a replay"
        );

        // 3rd (replay): rejected.
        assert!(
            !mgr.verify(&code),
            "the third use of the same code should also be rejected"
        );
    }

    #[test]
    fn different_windows_are_verified_independently() {
        let mgr = test_manager();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_window = now / TOTP_STEP;

        // Use the current-window code.
        let code_now = mgr.totp.generate(current_window * TOTP_STEP);
        assert!(mgr.verify(&code_now));

        // Code from the previous window (within tolerance).
        // (If less than 30s have elapsed, the previous window exists.)
        if current_window > 0 {
            let code_prev = mgr.totp.generate((current_window - 1) * TOTP_STEP);
            // As long as the codes differ, accept independently (clock drift tolerance).
            // TOTP codes are time-dependent so they usually differ.
            if code_prev != code_now {
                let prev_accepted = mgr.verify(&code_prev);
                // ±1 window verification accepts previous-window codes (clock drift tolerance).
                assert!(
                    prev_accepted,
                    "the previous-window code should be accepted under clock-drift tolerance"
                );
                // A second use of the same code is rejected.
                assert!(!mgr.verify(&code_prev));
            }
        }
    }

    #[test]
    fn entries_in_old_windows_are_gced() {
        let mgr = test_manager();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_window = now / TOTP_STEP;

        // Directly inject stale fake data (this cannot happen in reality; just verifying GC logic).
        {
            let mut used = mgr.used_codes.lock().unwrap();
            used.insert((current_window.saturating_sub(100), "999999".to_string()));
            used.insert((current_window.saturating_sub(50), "888888".to_string()));
        }

        // Verify a current-window code -> triggers GC.
        let code = mgr.totp.generate(current_window * TOTP_STEP);
        mgr.verify(&code);

        // After GC, no entries outside the retention range should remain.
        let used = mgr.used_codes.lock().unwrap();
        assert!(
            used.iter()
                .all(|(w, _)| (*w as i64) >= current_window as i64 - REPLAY_WINDOW_RANGE),
            "entries outside the GC range still remain: {:?}",
            *used
        );
    }
}
