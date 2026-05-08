//! 簡易レート制限ヘルパー — 認証エンドポイントのブルートフォース対策
//!
//! IP アドレス（または同等のキー）ごとに一定時間内の試行数を制限する。
//! TOTP / legacy_token / OAuth コールバック等、認証関連エンドポイントに適用する。
//!
//! # CRITICAL #2 対応
//!
//! TOTP は 6 桁数字の有効ウィンドウが 30 秒。レート制限なしだと 1 ウィンドウ内で
//! 数十万回の試行が可能であり、ネットワーク到達可能な攻撃者が現実的な時間で総当たり
//! できる。本モジュールでデフォルト 5 試行/分 に制限し、ブルートフォースを抑制する。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// レート制限設定
#[derive(Clone, Copy)]
pub struct RateLimitConfig {
    /// 計測ウィンドウ
    pub window: Duration,
    /// ウィンドウ内の最大試行数
    pub max_attempts: usize,
}

impl RateLimitConfig {
    /// TOTP ログイン用デフォルト設定（5 試行 / 60 秒）
    pub const fn totp_default() -> Self {
        Self {
            window: Duration::from_secs(60),
            max_attempts: 5,
        }
    }
}

/// シンプルな IP ベースレート制限器
pub struct RateLimiter {
    config: RateLimitConfig,
    /// (キー → 試行時刻のリスト)
    attempts: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// 試行を記録し、許可するか拒否するかを返す。
    ///
    /// `true`: 許可（ウィンドウ内の試行数が上限以下）
    /// `false`: 拒否（上限超過）
    ///
    /// 過去ウィンドウより古いエントリは自動 GC される。
    pub fn check_and_record(&self, key: &str) -> bool {
        let mut guard = match self.attempts.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!("RateLimiter mutex がポイズン状態。回復して継続します");
                poisoned.into_inner()
            }
        };

        let now = Instant::now();
        let entry = guard.entry(key.to_string()).or_default();

        // ウィンドウ外のエントリを削除
        entry.retain(|t| now.duration_since(*t) < self.config.window);

        if entry.len() >= self.config.max_attempts {
            return false;
        }

        entry.push(now);

        // 全体のサイズが大きくなりすぎないよう、空エントリを定期的に削除
        // （アクティブキーが少ない場合のクリーンアップ）
        if guard.len() > 1024 {
            guard.retain(|_, v| !v.is_empty());
        }

        true
    }

    /// 成功時に記録をリセットする（成功した正規ユーザーをペナルティから解放）
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
    fn 上限以下の試行は許可される() {
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
    fn 上限超過は拒否される() {
        let cfg = RateLimitConfig {
            window: Duration::from_secs(60),
            max_attempts: 3,
        };
        let limiter = RateLimiter::new(cfg);

        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        // 4 回目は拒否
        assert!(
            !limiter.check_and_record("1.2.3.4"),
            "4 回目は上限超過で拒否されるべき"
        );
    }

    #[test]
    fn 異なるキーは独立にカウントされる() {
        let cfg = RateLimitConfig {
            window: Duration::from_secs(60),
            max_attempts: 2,
        };
        let limiter = RateLimiter::new(cfg);

        assert!(limiter.check_and_record("1.2.3.4"));
        assert!(limiter.check_and_record("1.2.3.4"));
        // 別 IP は独立
        assert!(limiter.check_and_record("5.6.7.8"));
        assert!(limiter.check_and_record("5.6.7.8"));
        // 各々上限超過
        assert!(!limiter.check_and_record("1.2.3.4"));
        assert!(!limiter.check_and_record("5.6.7.8"));
    }

    #[test]
    fn ウィンドウ経過で再度許可される() {
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
            "ウィンドウ経過後は再度許可されるべき"
        );
    }

    #[test]
    fn reset_は記録をクリアする() {
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
            "reset 後は再度許可されるべき"
        );
    }

    #[test]
    fn totp_default_は_5_試行_60秒() {
        let cfg = RateLimitConfig::totp_default();
        assert_eq!(cfg.max_attempts, 5);
        assert_eq!(cfg.window, Duration::from_secs(60));
    }
}
