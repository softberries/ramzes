/// When to retry a failed delivery and when to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub base_ms: i64,
    pub max_ms: i64,
    pub max_attempts: i64,
    pub max_age_ms: i64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            base_ms: 5_000,
            max_ms: 3_600_000,
            max_attempts: 8,
            max_age_ms: 24 * 3_600_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Retry at this unix-millisecond time.
    At(i64),
    GiveUp,
}

/// Decides the next attempt after `attempts_done` failed attempts.
///
/// The delay doubles from `base_ms` up to `max_ms` and is scaled by 0.8 to 1.2 according to
/// `jitter` (0.0 to 1.0). A `retry_after_ms` asked for by the service replaces a shorter delay.
pub fn next_attempt(
    policy: &RetryPolicy,
    attempts_done: i64,
    created_at: i64,
    now_ms: i64,
    retry_after_ms: Option<i64>,
    jitter: f64,
) -> Next {
    if attempts_done >= policy.max_attempts {
        return Next::GiveUp;
    }
    let doublings = u32::try_from(attempts_done.saturating_sub(1))
        .unwrap_or(0)
        .min(40);
    let base = policy
        .base_ms
        .saturating_mul(1_i64 << doublings)
        .min(policy.max_ms);
    let scaled = (base as f64 * (0.8 + 0.4 * jitter.clamp(0.0, 1.0))).round() as i64;
    let delay = scaled.max(retry_after_ms.unwrap_or(0));
    let at = now_ms.saturating_add(delay);
    if at > created_at.saturating_add(policy.max_age_ms) {
        Next::GiveUp
    } else {
        Next::At(at)
    }
}
