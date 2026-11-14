//! Where the time comes from.
//!
//! Java counterpart: the `java.time.Clock` bean Ledgerly-Java injects everywhere (always UTC).
//!
//! The domain never asks the operating system for the time. It is handed a [`Clock`], so a test can decide what
//! "now" and "today" are (CONCEPT §3, rule 4).

use std::fmt;
#[cfg(any(test, feature = "testing"))]
use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(any(test, feature = "testing"))]
use chrono::TimeDelta;
use chrono::{DateTime, NaiveDate, Utc};

/// A source of the current time.
///
/// `Send + Sync` so that one clock can be shared by every request as an `Arc<dyn Clock>`.
pub trait Clock: fmt::Debug + Send + Sync {
    /// The current instant.
    fn now(&self) -> DateTime<Utc>;

    /// The current date in UTC: Java's `LocalDate.now(clock)`.
    fn today(&self) -> NaiveDate {
        self.now().date_naive()
    }
}

/// The real time, from the operating system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock for tests: it shows the time it was given until a test moves it. Java's `Clock.fixed(...)`.
///
/// Only compiled for tests and with the `testing` feature, so it cannot end up in the `ledgerly` binary.
#[cfg(any(test, feature = "testing"))]
#[derive(Debug)]
pub struct FixedClock {
    now: Mutex<DateTime<Utc>>,
}

#[cfg(any(test, feature = "testing"))]
impl FixedClock {
    /// A clock that stands still at `now`.
    #[must_use]
    pub fn at<Tz: chrono::TimeZone>(now: DateTime<Tz>) -> Self {
        Self {
            now: Mutex::new(now.with_timezone(&Utc)),
        }
    }

    /// Moves the clock to `now`.
    pub fn set<Tz: chrono::TimeZone>(&self, now: DateTime<Tz>) {
        *self.lock() = now.with_timezone(&Utc);
    }

    /// Moves the clock forward by `duration` (or back, if it is negative).
    pub fn advance(&self, duration: TimeDelta) {
        let mut now = self.lock();
        *now = now.checked_add_signed(duration).unwrap_or(*now);
    }

    /// A test that panicked while holding the lock leaves a perfectly good timestamp behind, so poisoning is
    /// ignored.
    fn lock(&self) -> MutexGuard<'_, DateTime<Utc>> {
        self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(any(test, feature = "testing"))]
impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.lock()
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeDelta};

    use super::*;

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    #[test]
    fn a_fixed_clock_always_tells_the_same_time() {
        let clock = FixedClock::at(instant("2026-11-05T10:00:00Z"));
        assert_eq!(clock.now(), instant("2026-11-05T10:00:00Z"));
        assert_eq!(clock.now(), clock.now());
    }

    #[test]
    fn today_is_the_utc_date() {
        let late = FixedClock::at(instant("2026-11-05T23:59:59.999Z"));
        assert_eq!(late.today(), NaiveDate::from_ymd_opt(2026, 11, 5).unwrap());

        // 00:30 on the 6th in Berlin is still the 5th in UTC.
        let berlin = FixedClock::at(instant("2026-11-06T00:30:00+01:00"));
        assert_eq!(berlin.today(), NaiveDate::from_ymd_opt(2026, 11, 5).unwrap());
    }

    #[test]
    fn a_fixed_clock_can_be_moved() {
        let clock = FixedClock::at(instant("2026-11-05T10:00:00Z"));
        clock.advance(TimeDelta::hours(14));
        assert_eq!(clock.now(), instant("2026-11-06T00:00:00Z"));
        clock.set(instant("2030-01-01T00:00:00Z"));
        assert_eq!(clock.today(), NaiveDate::from_ymd_opt(2030, 1, 1).unwrap());
    }

    #[test]
    fn the_system_clock_follows_real_time() {
        let before = Utc::now();
        let now = SystemClock.now();
        let after = Utc::now();
        assert!(before <= now && now <= after);
    }

    #[test]
    fn clocks_are_usable_as_shared_trait_objects() {
        let clock: std::sync::Arc<dyn Clock> = std::sync::Arc::new(FixedClock::at(instant("2026-11-05T10:00:00Z")));
        let handle = std::thread::spawn({
            let clock = std::sync::Arc::clone(&clock);
            move || clock.today()
        });
        assert_eq!(handle.join().unwrap(), clock.today());
    }
}
