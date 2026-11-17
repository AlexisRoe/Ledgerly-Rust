//! The optimistic-locking version of a stored aggregate.
//!
//! Java counterpart: the `@Version long version` field JPA manages on an entity. It starts at 0, goes up by one
//! with every update, and an update that started from an older version fails. Clients see it as the ETag
//! `"<version>"` (port spec §3.6).

use std::fmt;

/// How often a stored aggregate has been changed: 0 when it was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(i64);

impl Version {
    /// The version of a row that was just inserted, as JPA writes it.
    pub const INITIAL: Self = Self(0);

    /// A version read from the `version bigint` column.
    #[must_use]
    pub const fn from_stored(value: i64) -> Self {
        Self(value)
    }

    /// The value for the `version` column.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// The version after one more update, or `None` if `bigint` is exhausted, which takes 2⁶³ updates.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

/// A stored aggregate together with the version it was read or written at.
///
/// Java keeps the version inside the entity. Here the aggregate stays free of persistence concerns, and whoever
/// needs the version for the next update or for an ETag gets it next to the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned<T> {
    pub value: T,
    pub version: Version,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero_and_counts_up() {
        assert_eq!(Version::INITIAL.get(), 0);
        assert_eq!(Version::INITIAL.next(), Some(Version::from_stored(1)));
        assert_eq!(Version::from_stored(i64::MAX).next(), None);
        assert_eq!(Version::from_stored(7).to_string(), "7");
    }
}
