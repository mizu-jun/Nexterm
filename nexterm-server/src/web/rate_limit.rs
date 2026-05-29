//! Simple rate limit helper — defense against brute force on authentication endpoints.
//!
//! Limits the number of attempts per IP address (or equivalent key) within a sliding window.
//! Applied to authentication-related endpoints such as TOTP / legacy_token / OAuth callback.
//!
//! # CRITICAL #2
//!
//! TOTP codes are 6 digits with a 30-second validity window. Without a rate limit, an attacker
//! can perform hundreds of thousands of attempts per window, making brute force feasible in
//! practice from a network-reachable position. This module limits attempts to 5/min by default
//! and suppresses brute force.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Rate-limit configuration.
#[derive(Clone, Copy)]
pub struct RateLimitConfig {
    /// Measurement window.
    pub window: Duration,
    /// Maximum number of attempts inside the window.
    pub max_attempts: usize,
}

impl RateLimitConfig {
    /// Default configuration for TOTP login (5 attempts / 60 seconds).
    pub const fn totp_default() -> Self {
        Self {
            window: Duration::from_secs(60),
            max_attempts: 5,
        }
    }
}

/// Simple IP-based rate limiter.
pub struct RateLimiter {
    config: RateLimitConfig,
    /// (key -> list of attempt timestamps).
    attempts: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// Record an attempt and return whether to allow or reject it.
    ///
    /// `true`: allowed (attempts in the window are within the limit).
    /// `false`: rejected (limit exceeded).
    ///
    /// Entries older than the window are garbage-collected automatically.
    pub fn check_and_record(&self, key: &str) -> bool {
        let mut guard = match self.attempts.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!("RateLimiter mutex is poisoned; recovering and continuing");
                poisoned.into_inner()
            }
        };

        let now = Instant::now();
        let entry = guard.entry(key.to_string()).or_default();

        // Drop entries outside the window.
        entry.retain(|t| now.duration_since(*t) < self.config.window);

        if entry.len() >= self.config.max_attempts {
            return false;
        }

        entry.push(now);

        // Periodically purge empty entries to keep the overall size bounded
        // (cleanup is cheap when only a few keys are active).
        if guard.len() > 1024 {
            guard.retain(|_, v| !v.is_empty());
        }

        true
    }

    /// Reset the record on success (free legitimate users from penalties after a successful auth).
    pub fn reset(&self, key: &str) {
        if let Ok(mut guard) = self.attempts.lock() {
            guard.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_within_limit_are_allowed() {
        let cfg = RateLimitConfig {
            window: Duration::from_secs(60),
            max_attempts: 3,
        };
        let limiter = RateLimiter::new(cfg);

        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
    }

    #[test]
    fn attempts_over_limit_are_rejected() {
        let cfg = RateLimitConfig {
            window: Duration::from_secs(60),
            max_attempts: 3,
        };
        let limiter = RateLimiter::new(cfg);

        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        // The 4th attempt is rejected.
        assert!(
            !limiter.check_and_record("1.2.3.4"),
            "the 4th attempt should be rejected for exceeding the limit"
        );
    }

    #[test]
    fn different_keys_are_counted_independently() {
        let cfg = RateLimitConfig {
            window: Duration::from_secs(60),
            max_attempts: 2,
        };
        let limiter = RateLimiter::new(cfg);

        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        // A different IP is independent.
        assert!(limiter.check_and_record("5.6.7.8"));
        assert!(limiter.check_and_record("5.6.7.8"));
        // Each hits its own limit.
        assert!(!limiter.check_and_record("1.2.3.4"));
        assert!(!limiter.check_and_record("5.6.7.8"));
    }

    #[test]
    fn allowed_again_after_window_passes() {
        let cfg = RateLimitConfig {
            window: Duration::from_millis(50),
            max_attempts: 2,
        };
        let limiter = RateLimiter::new(cfg);

        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(!limiter.check_and_record("1.2.3.4"));

        std::thread::sleep(Duration::from_millis(60));

        assert!(
            limiter.check_and_record("1.2.3.4"),
            "should be allowed again after the window passes"
        );
    }

    #[test]
    fn reset_clears_record() {
        let cfg = RateLimitConfig {
            window: Duration::from_secs(60),
            max_attempts: 2,
        };
        let limiter = RateLimiter::new(cfg);

        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(!limiter.check_and_record("1.2.3.4"));

        limiter.reset("1.2.3.4");

        assert!(
            limiter.check_and_record("1.2.3.4"),
            "should be allowed again after reset"
        );
    }

    #[test]
    fn totp_default_is_5_attempts_per_60s() {
        let cfg = RateLimitConfig::totp_default();
        assert_eq!(cfg.max_attempts, 5);
        assert_eq!(cfg.window, Duration::from_secs(60));
    }
}
