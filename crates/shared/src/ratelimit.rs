//! Token-bucket limits kept in memory, one bucket per (scope, key): ("login", email) from M5, ("api", user id)
//! and ("auth-ip", client address) from M10.
//!
//! Java counterpart: `shared/ratelimit/RateLimiter.java` and `RateLimit.java`, Bucket4j 8 buckets in a Caffeine
//! cache. The refill arithmetic is Bucket4j's:
//!
//! - **gradual** (`refillGreedy`): tokens come back continuously, `capacity` per `period`, with the remainder of
//!   the division carried over so that none are lost to rounding;
//! - **all at once** (`refillIntervally`): all `capacity` tokens come back at the end of each `period`, counted
//!   from when the bucket was made. A lock after five failed logins lasts until the next such boundary, not
//!   15 minutes from the fifth failure.
//!
//! Refills follow the application clock, so tests can move time. Like Caffeine's cache, the store forgets a
//! bucket two hours after its last use, measured on the monotonic clock, and holds at most 200,000 buckets, so
//! the limiter cannot itself be used to exhaust memory.
//!
//! **More than one instance:** the buckets live in the memory of one process. Two instances behind a load
//! balancer each keep their own, and a restart forgets all of them, including the login lock. Java documents
//! the same.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::clock::Clock;

/// Caffeine's `maximumSize(200_000)`.
const MAX_BUCKETS: usize = 200_000;
/// Caffeine's `expireAfterAccess(Duration.ofHours(2))`.
const IDLE_EXPIRY: Duration = Duration::from_secs(2 * 60 * 60);

/// How many actions are allowed per period. Java: `RateLimit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    capacity: u64,
    period_nanos: u64,
    gradual: bool,
}

/// A limit needs at least one token and a period longer than zero. Java: `IllegalArgumentException("A rate limit
/// needs a positive capacity and period")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("A rate limit needs a positive capacity and period")]
pub struct InvalidRateLimit;

impl RateLimit {
    /// Smooth: the allowance refills continuously over a minute. Java: `RateLimit.perMinute`.
    ///
    /// # Errors
    ///
    /// [`InvalidRateLimit`] for zero requests.
    pub fn per_minute(requests: u32) -> Result<Self, InvalidRateLimit> {
        Self::new(requests, Duration::from_secs(60), true)
    }

    /// All `capacity` tokens come back together once `period` has passed. Java: `RateLimit.intervally`, used for
    /// the login lock.
    ///
    /// # Errors
    ///
    /// [`InvalidRateLimit`] for a zero capacity or period.
    pub fn intervally(capacity: u32, period: Duration) -> Result<Self, InvalidRateLimit> {
        Self::new(capacity, period, false)
    }

    fn new(capacity: u32, period: Duration, gradual: bool) -> Result<Self, InvalidRateLimit> {
        let period_nanos = u64::try_from(period.as_nanos()).unwrap_or(u64::MAX);
        if capacity == 0 || period_nanos == 0 {
            return Err(InvalidRateLimit);
        }
        Ok(Self {
            capacity: u64::from(capacity),
            period_nanos,
            gradual,
        })
    }
}

/// The outcome of asking for one token. Java: `RateLimiter.Decision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    /// Whether the token was taken.
    pub allowed: bool,
    /// How long until a token is there again; zero when allowed.
    pub retry_after: Duration,
    /// Tokens left after this one.
    pub remaining: u64,
}

/// The token buckets of one process. Cheap to clone; clones share the buckets.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    clock: Arc<dyn Clock>,
    buckets: Arc<Mutex<HashMap<String, Entry>>>,
}

#[derive(Debug)]
struct Entry {
    bucket: Bucket,
    last_access: Instant,
}

impl RateLimiter {
    /// An empty limiter on `clock`.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Takes one token if there is one. Java: `tryConsume`.
    #[must_use]
    pub fn try_consume(&self, scope: &str, key: &str, limit: RateLimit) -> Decision {
        self.with_bucket(scope, key, limit, |bucket, now| bucket.try_consume(limit, now))
    }

    /// Whether no token is left, without taking one. Java: `isExhausted`.
    #[must_use]
    pub fn is_exhausted(&self, scope: &str, key: &str, limit: RateLimit) -> bool {
        self.with_bucket(scope, key, limit, |bucket, now| {
            bucket.refill(limit, now);
            bucket.tokens < 1
        })
    }

    /// Fills the bucket again: a successful login forgives earlier failures. Java: `reset`, which keeps the
    /// bucket's refill schedule.
    pub fn reset(&self, scope: &str, key: &str, limit: RateLimit) {
        self.with_bucket(scope, key, limit, |bucket, _| bucket.reset(limit));
    }

    fn with_bucket<T>(
        &self,
        scope: &str,
        key: &str,
        limit: RateLimit,
        use_it: impl FnOnce(&mut Bucket, i64) -> T,
    ) -> T {
        let now = nanos(self.clock.now());
        let accessed = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        let name = format!("{scope}\u{0}{key}");
        if !buckets.contains_key(&name) && buckets.len() >= MAX_BUCKETS {
            evict(&mut buckets, accessed);
        }
        let entry = buckets
            .entry(name)
            .and_modify(|entry| {
                if accessed.duration_since(entry.last_access) >= IDLE_EXPIRY {
                    entry.bucket = Bucket::full(limit, now);
                }
            })
            .or_insert_with(|| Entry {
                bucket: Bucket::full(limit, now),
                last_access: accessed,
            });
        entry.last_access = accessed;
        use_it(&mut entry.bucket, now)
    }
}

/// Makes room for one more bucket: the expired ones go, and if that is not enough, the least recently used.
fn evict(buckets: &mut HashMap<String, Entry>, now: Instant) {
    buckets.retain(|_, entry| now.duration_since(entry.last_access) < IDLE_EXPIRY);
    if buckets.len() >= MAX_BUCKETS
        && let Some(oldest) = buckets
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(name, _)| name.clone())
    {
        buckets.remove(&oldest);
    }
}

/// Nanoseconds since the epoch on the application clock. Java: `ClockTimeMeter.currentTimeNanos`.
fn nanos(now: DateTime<Utc>) -> i64 {
    now.timestamp_nanos_opt().unwrap_or(i64::MAX)
}

/// One Bucket4j bucket with one bandwidth.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: u64,
    last_refill: i64,
    /// The part of a token that a gradual refill has earned but not handed out, in token-nanoseconds.
    rounding_error: u64,
}

impl Bucket {
    const fn full(limit: RateLimit, now: i64) -> Self {
        Self {
            tokens: limit.capacity,
            last_refill: now,
            rounding_error: 0,
        }
    }

    fn try_consume(&mut self, limit: RateLimit, now: i64) -> Decision {
        self.refill(limit, now);
        if self.tokens >= 1 {
            self.tokens -= 1;
            Decision {
                allowed: true,
                retry_after: Duration::ZERO,
                remaining: self.tokens,
            }
        } else {
            Decision {
                allowed: false,
                retry_after: Duration::from_nanos(self.nanos_until_a_token(limit, now)),
                remaining: 0,
            }
        }
    }

    const fn reset(&mut self, limit: RateLimit) {
        self.tokens = limit.capacity;
        self.rounding_error = 0;
    }

    /// Bucket4j's `BucketState.refillAllBandwidth`.
    fn refill(&mut self, limit: RateLimit, now: i64) {
        // A clock that went back refills nothing, as in Bucket4j.
        let Ok(elapsed) = u64::try_from(now.saturating_sub(self.last_refill)) else {
            return;
        };
        if limit.gradual {
            self.refill_gradually(limit, now, elapsed);
        } else {
            self.refill_intervally(limit, elapsed);
        }
    }

    fn refill_gradually(&mut self, limit: RateLimit, now: i64, mut elapsed: u64) {
        self.last_refill = now;
        if elapsed > limit.period_nanos {
            let periods = elapsed / limit.period_nanos;
            let refilled = self.tokens.saturating_add(periods.saturating_mul(limit.capacity));
            if refilled >= limit.capacity {
                self.reset(limit);
                return;
            }
            self.tokens = refilled;
            elapsed %= limit.period_nanos;
        }
        let earned = u128::from(limit.capacity) * u128::from(elapsed) + u128::from(self.rounding_error);
        let period = u128::from(limit.period_nanos);
        let whole = u64::try_from(earned / period).unwrap_or(u64::MAX);
        let refilled = self.tokens.saturating_add(whole);
        if refilled >= limit.capacity {
            self.reset(limit);
        } else {
            self.tokens = refilled;
            self.rounding_error = u64::try_from(earned % period).unwrap_or(0);
        }
    }

    fn refill_intervally(&mut self, limit: RateLimit, elapsed: u64) {
        if elapsed < limit.period_nanos {
            return;
        }
        let periods = elapsed / limit.period_nanos;
        let advanced = i64::try_from(periods.saturating_mul(limit.period_nanos)).unwrap_or(i64::MAX);
        self.last_refill = self.last_refill.saturating_add(advanced);
        self.tokens = self
            .tokens
            .saturating_add(periods.saturating_mul(limit.capacity))
            .min(limit.capacity);
    }

    /// Bucket4j's `calculateDelayNanosAfterWillBePossibleToConsume` for one token, after a refill.
    fn nanos_until_a_token(&self, limit: RateLimit, now: i64) -> u64 {
        if self.tokens >= 1 {
            return 0;
        }
        if limit.gradual {
            // One token is `period / capacity` nanoseconds; part of it is already earned.
            let needed = u128::from(limit.period_nanos).saturating_sub(u128::from(self.rounding_error));
            let capacity = u128::from(limit.capacity);
            u64::try_from(needed.div_ceil(capacity)).unwrap_or(u64::MAX)
        } else {
            let next = self
                .last_refill
                .saturating_add(i64::try_from(limit.period_nanos).unwrap_or(i64::MAX));
            u64::try_from(next.saturating_sub(now)).unwrap_or(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicU32, Ordering};

    use chrono::{TimeDelta, TimeZone};

    use super::*;
    use crate::clock::FixedClock;

    fn limiter() -> (Arc<FixedClock>, RateLimiter) {
        let clock = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 13, 12, 0, 0).unwrap()));
        (clock.clone(), RateLimiter::new(clock))
    }

    /// Java: `RateLimiterTest.allowsUpToTheCapacityThenRefusesWithAWaitTime`.
    #[test]
    fn allows_up_to_the_capacity_then_refuses_with_a_wait_time() {
        let (_, limiter) = limiter();
        let limit = RateLimit::per_minute(3).unwrap();

        for _ in 0..3 {
            assert!(limiter.try_consume("api", "u1", limit).allowed);
        }
        let refused = limiter.try_consume("api", "u1", limit);

        assert!(!refused.allowed);
        assert!(
            (Duration::from_secs(1)..=Duration::from_secs(60)).contains(&refused.retry_after),
            "{refused:?}"
        );
        assert_eq!(refused.retry_after, Duration::from_secs(20));
    }

    /// Java: `RateLimiterTest.remainingTokensCountDown`.
    #[test]
    fn remaining_tokens_count_down() {
        let (_, limiter) = limiter();
        let limit = RateLimit::per_minute(5).unwrap();

        assert_eq!(limiter.try_consume("api", "u1", limit).remaining, 4);
        assert_eq!(limiter.try_consume("api", "u1", limit).remaining, 3);
    }

    /// Java: `RateLimiterTest.gradualLimitsGetTokensBackOverTime`.
    #[test]
    fn gradual_limits_get_tokens_back_over_time() {
        let (clock, limiter) = limiter();
        let limit = RateLimit::per_minute(60).unwrap(); // one token per second
        for _ in 0..60 {
            let _ = limiter.try_consume("api", "u1", limit);
        }
        assert!(!limiter.try_consume("api", "u1", limit).allowed);

        clock.advance(TimeDelta::seconds(2));

        assert!(limiter.try_consume("api", "u1", limit).allowed);
        assert!(limiter.try_consume("api", "u1", limit).allowed);
        assert!(!limiter.try_consume("api", "u1", limit).allowed);
    }

    #[test]
    fn a_gradual_refill_loses_nothing_to_rounding() {
        let (clock, limiter) = limiter();
        let limit = RateLimit::per_minute(7).unwrap(); // one token every 8.57… seconds
        for _ in 0..7 {
            let _ = limiter.try_consume("api", "u1", limit);
        }

        // Sixty one-second steps earn exactly seven tokens, although no single step earns a whole one.
        for _ in 0..60 {
            clock.advance(TimeDelta::seconds(1));
            let _ = limiter.is_exhausted("api", "u1", limit);
        }

        assert_eq!(limiter.try_consume("api", "u1", limit).remaining, 6);
    }

    /// Java: `RateLimiterTest.intervalLimitsGetEverythingBackAtTheEndOfThePeriod`.
    #[test]
    fn interval_limits_get_everything_back_at_the_end_of_the_period() {
        let (clock, limiter) = limiter();
        let limit = RateLimit::intervally(5, Duration::from_secs(15 * 60)).unwrap();
        for _ in 0..5 {
            let _ = limiter.try_consume("login", "a@x.test", limit);
        }
        assert!(limiter.is_exhausted("login", "a@x.test", limit));

        clock.advance(TimeDelta::minutes(14));
        assert!(limiter.is_exhausted("login", "a@x.test", limit));

        clock.advance(TimeDelta::minutes(2));
        assert!(!limiter.is_exhausted("login", "a@x.test", limit));
    }

    #[test]
    fn an_interval_is_counted_from_when_the_bucket_was_made() {
        let (clock, limiter) = limiter();
        let limit = RateLimit::intervally(2, Duration::from_secs(15 * 60)).unwrap();
        let _ = limiter.try_consume("login", "k", limit);
        clock.advance(TimeDelta::minutes(10));
        let refused = {
            let _ = limiter.try_consume("login", "k", limit);
            limiter.try_consume("login", "k", limit)
        };

        // Locked at minute 10, free again at minute 15, not at minute 25.
        assert!(!refused.allowed);
        assert_eq!(refused.retry_after, Duration::from_secs(5 * 60));
        clock.advance(TimeDelta::minutes(5));
        assert!(!limiter.is_exhausted("login", "k", limit));
    }

    /// Java: `RateLimiterTest.checkingIfExhaustedTakesNothing`.
    #[test]
    fn checking_if_exhausted_takes_nothing() {
        let (_, limiter) = limiter();
        let limit = RateLimit::intervally(1, Duration::from_secs(60)).unwrap();

        for _ in 0..10 {
            assert!(!limiter.is_exhausted("login", "k", limit));
        }
        assert!(limiter.try_consume("login", "k", limit).allowed);
    }

    /// Java: `RateLimiterTest.resetFillsTheBucketAgain`.
    #[test]
    fn reset_fills_the_bucket_again() {
        let (_, limiter) = limiter();
        let limit = RateLimit::intervally(2, Duration::from_secs(3600)).unwrap();
        let _ = limiter.try_consume("login", "k", limit);
        let _ = limiter.try_consume("login", "k", limit);
        assert!(limiter.is_exhausted("login", "k", limit));

        limiter.reset("login", "k", limit);

        assert!(!limiter.is_exhausted("login", "k", limit));
    }

    /// Java: `RateLimiterTest.scopesAndKeysAreIndependent`.
    #[test]
    fn scopes_and_keys_are_independent() {
        let (_, limiter) = limiter();
        let limit = RateLimit::per_minute(1).unwrap();

        assert!(limiter.try_consume("api", "u1", limit).allowed);
        assert!(!limiter.try_consume("api", "u1", limit).allowed);
        assert!(limiter.try_consume("api", "u2", limit).allowed);
        assert!(limiter.try_consume("other", "u1", limit).allowed);
    }

    /// Java: `RateLimiterTest.concurrentCallersNeverGetMoreThanTheCapacity`.
    #[test]
    fn concurrent_callers_never_get_more_than_the_capacity() {
        let (_, limiter) = limiter();
        let limit = RateLimit::per_minute(50).unwrap();
        let allowed = AtomicU32::new(0);
        let gate = Barrier::new(8);

        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    gate.wait();
                    for _ in 0..100 {
                        if limiter.try_consume("api", "shared", limit).allowed {
                            allowed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });

        assert_eq!(allowed.load(Ordering::Relaxed), 50);
    }

    /// Java: `RateLimiterTest.aLimitNeedsAPositiveCapacityAndPeriod`.
    #[test]
    fn a_limit_needs_a_positive_capacity_and_period() {
        assert_eq!(RateLimit::per_minute(0), Err(InvalidRateLimit));
        assert_eq!(RateLimit::intervally(1, Duration::ZERO), Err(InvalidRateLimit));
    }
}
