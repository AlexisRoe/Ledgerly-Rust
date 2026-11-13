//! Typed ids: one newtype per kind of thing, so a `TeamId` cannot be passed where an `ExpenseId` is expected.
//!
//! Java counterpart: `shared/id/` (`ExpenseId`, `TeamId`, `UserId` records and `UuidV7`). Java wraps only those
//! three and passes receipts, invitations, imports and events around as bare `UUID`s; here each has its own type.
//!
//! Each id offers only the constructor for the UUID version Java stores for it (port spec §3.1):
//! time-ordered v7 for everything except the import id, which Java creates with `UUID.randomUUID()` (v4).
//!
//! ```
//! use ledgerly_shared::id::{ExpenseId, TeamId};
//!
//! fn find(_team: TeamId, _expense: ExpenseId) {}
//!
//! find(TeamId::new_v7(), ExpenseId::new_v7());
//! ```
//!
//! Swapping the two arguments is a type error, not a bug found in a test:
//!
//! ```compile_fail,E0308
//! use ledgerly_shared::id::{ExpenseId, TeamId};
//!
//! fn find(_team: TeamId, _expense: ExpenseId) {}
//!
//! find(ExpenseId::new_v7(), TeamId::new_v7());
//! ```

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// The text is not an id in the canonical form `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` (36 characters, hex digits
/// in either case, hyphens at positions 9, 14, 19 and 24).
///
/// Only the canonical form is accepted, by decision of the reviewer after M1. The `uuid` crate also accepts
/// forms without hyphens, in braces or as `urn:uuid:`; Java's `UUID.fromString` rejects those but accepts
/// shortened groups such as `1-1-1-1-1`. Neither set of odd forms is taken over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a UUID in the form xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx")]
pub struct InvalidId;

/// Parses the canonical hyphenated form and nothing else.
fn parse_canonical(text: &str) -> Result<Uuid, InvalidId> {
    let bytes = text.as_bytes();
    let canonical = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if !canonical {
        return Err(InvalidId);
    }
    Uuid::try_parse(text).map_err(|_| InvalidId)
}

/// Defines an id newtype over [`Uuid`].
///
/// The last argument picks the only constructor the type gets: `v7` (time-ordered) or `v4` (random).
macro_rules! typed_id {
    ($(#[$doc:meta])* $name:ident, v7) => {
        typed_id!(@common $(#[$doc])* $name);

        impl $name {
            /// A new time-ordered id (UUIDv7, RFC 9562).
            ///
            /// Ids created one after the other in this process sort in creation order, also within the same
            /// millisecond. Java's `UuidV7` only orders ids from different milliseconds.
            #[must_use]
            pub fn new_v7() -> Self {
                Self(Uuid::now_v7())
            }
        }
    };
    ($(#[$doc:meta])* $name:ident, v4) => {
        typed_id!(@common $(#[$doc])* $name);

        impl $name {
            /// A new random id (UUIDv4), where Java uses `UUID.randomUUID()`.
            #[must_use]
            pub fn new_v4() -> Self {
                Self(Uuid::new_v4())
            }
        }
    };
    (@common $(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        /// Only the canonical form, as [`FromStr`] (see [`InvalidId`]).
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }

        impl $name {
            /// Wraps a UUID that already identifies one of these, such as a column value or a path parameter.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// The wrapped UUID, for SQL parameters and for other crates' APIs.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        /// The bare, lower-case, hyphenated UUID, like `toString()` of Java's id records.
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self.0.as_hyphenated(), f)
            }
        }

        /// Only the canonical hyphenated form; see [`InvalidId`].
        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                parse_canonical(s).map(Self)
            }
        }
    };
}

typed_id!(
    /// Identifies a user.
    UserId,
    v7
);
typed_id!(
    /// Identifies a team.
    TeamId,
    v7
);
typed_id!(
    /// Identifies an expense.
    ExpenseId,
    v7
);
typed_id!(
    /// Identifies a receipt.
    ReceiptId,
    v7
);
typed_id!(
    /// Identifies an invitation.
    InvitationId,
    v7
);
typed_id!(
    /// Identifies an import. Random, not time-ordered: Java creates import ids with `UUID.randomUUID()`.
    ImportId,
    v4
);
typed_id!(
    /// Identifies one published event, so that consumers such as the audit trail can ignore a redelivery.
    EventId,
    v7
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v7_ids_have_version_7_and_the_rfc_variant() {
        let id = ExpenseId::new_v7().as_uuid();
        assert_eq!(id.get_version_num(), 7);
        assert_eq!(id.get_variant(), uuid::Variant::RFC4122);
    }

    #[test]
    fn import_ids_are_random_v4_like_javas() {
        let id = ImportId::new_v4().as_uuid();
        assert_eq!(id.get_version_num(), 4);
        assert_eq!(id.get_variant(), uuid::Variant::RFC4122);
    }

    #[test]
    fn v7_ids_created_in_sequence_sort_in_creation_order() {
        let ids: Vec<ExpenseId> = (0..10_000).map(|_| ExpenseId::new_v7()).collect();
        assert!(
            ids.windows(2).all(|pair| pair[0] < pair[1]),
            "ids are not strictly increasing"
        );

        // The string form sorts the same way, which is what a `uuid` column index and `ORDER BY id` see.
        let strings: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert!(strings.is_sorted());
    }

    #[test]
    fn display_is_the_bare_lowercase_uuid_like_javas_to_string() {
        let id: TeamId = "00000000-0000-7000-8000-00000000A001".parse().unwrap();
        assert_eq!(id.to_string(), "00000000-0000-7000-8000-00000000a001");
    }

    #[test]
    fn display_and_from_str_round_trip() {
        let id = UserId::new_v7();
        assert_eq!(id.to_string().parse::<UserId>().unwrap(), id);
    }

    #[test]
    fn from_str_rejects_what_is_not_a_uuid() {
        assert!("".parse::<UserId>().is_err());
        assert!("not-a-uuid".parse::<UserId>().is_err());
        assert!("00000000-0000-7000-8000-00000000a00".parse::<UserId>().is_err());
    }

    #[test]
    fn only_the_canonical_form_is_accepted() {
        let canonical = "0192f0c1-2b3c-7d4e-8f50-123456789abc";
        assert!(canonical.parse::<UserId>().is_ok());
        assert!(canonical.to_uppercase().parse::<UserId>().is_ok());
        for odd in [
            "0192f0c12b3c7d4e8f50123456789abc",
            "{0192f0c1-2b3c-7d4e-8f50-123456789abc}",
            "urn:uuid:0192f0c1-2b3c-7d4e-8f50-123456789abc",
            "1-1-1-1-1",
            "0192f0c1-2b3c-7d4e-8f50-123456789ab",
            "0192f0c1-2b3c-7d4e-8f50-123456789abcd",
            "0192f0c1+2b3c-7d4e-8f50-123456789abc",
            "0192f0c1-2b3c-7d4e-8f50-123456789abg",
            " 0192f0c1-2b3c-7d4e-8f50-123456789ab",
        ] {
            assert_eq!(odd.parse::<UserId>(), Err(InvalidId), "{odd}");
            assert!(serde_json::from_str::<UserId>(&format!("\"{odd}\"")).is_err(), "{odd}");
        }
    }

    #[test]
    fn serde_form_is_a_plain_uuid_string() {
        let id: ReceiptId = "0192f0c1-2b3c-7d4e-8f50-123456789abc".parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""0192f0c1-2b3c-7d4e-8f50-123456789abc""#);
        assert_eq!(serde_json::from_str::<ReceiptId>(&json).unwrap(), id);
    }

    #[test]
    fn serde_rejects_what_is_not_a_uuid_string() {
        assert!(serde_json::from_str::<InvitationId>(r#""nope""#).is_err());
        assert!(serde_json::from_str::<InvitationId>("42").is_err());
        assert!(serde_json::from_str::<InvitationId>(r#"{"value":"0192f0c1-2b3c-7d4e-8f50-123456789abc"}"#).is_err());
    }

    #[test]
    fn ids_wrap_and_unwrap_a_uuid_unchanged() {
        let raw = Uuid::now_v7();
        assert_eq!(EventId::from_uuid(raw).as_uuid(), raw);
    }
}
