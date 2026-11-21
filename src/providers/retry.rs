use std::time::Duration;

use crate::config::Resolved;

/// How a provider call that failed before streaming started is retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt; `0` disables retrying.
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
        }
    }
}

impl RetryPolicy {
    pub fn disabled() -> Self {
        Self {
            max_retries: 0,
            ..Self::default()
        }
    }

    pub fn from_resolved(resolved: &Resolved) -> Self {
        Self {
            max_retries: resolved.max_retries,
            base_delay: Duration::from_millis(resolved.retry_base_delay_ms.max(1)),
            max_delay: Duration::from_millis(resolved.retry_max_delay_ms.max(1)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.max_retries > 0
    }

    /// Wait before retry number `attempt` (1-based).
    ///
    /// The schedule doubles from `base_delay`; a `Retry-After` hint from the
    /// server wins over it. Both are capped at `max_delay`.
    pub fn delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        if let Some(hint) = retry_after {
            return hint.min(self.max_delay);
        }
        let exponent = attempt.saturating_sub(1).min(16);
        let delay = self.base_delay.saturating_mul(1u32 << exponent);
        delay.min(self.max_delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_retries: 4,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        }
    }

    #[test]
    fn backoff_doubles_per_attempt() {
        let policy = policy();
        assert_eq!(policy.delay(1, None), Duration::from_millis(100));
        assert_eq!(policy.delay(2, None), Duration::from_millis(200));
        assert_eq!(policy.delay(3, None), Duration::from_millis(400));
        assert_eq!(policy.delay(4, None), Duration::from_millis(800));
    }

    #[test]
    fn backoff_is_capped() {
        let policy = policy();
        assert_eq!(policy.delay(10, None), Duration::from_secs(2));
        assert_eq!(policy.delay(u32::MAX, None), Duration::from_secs(2));
    }

    #[test]
    fn server_hint_wins_but_is_capped() {
        let policy = policy();
        assert_eq!(policy.delay(1, Some(Duration::from_millis(50))), Duration::from_millis(50));
        assert_eq!(policy.delay(1, Some(Duration::from_secs(60))), Duration::from_secs(2));
    }

    #[test]
    fn zero_retries_disables_the_policy() {
        assert!(!RetryPolicy::disabled().is_enabled());
        assert!(RetryPolicy::default().is_enabled());
    }
}
