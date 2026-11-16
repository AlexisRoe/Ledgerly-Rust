//! The role a teammate holds in a team.
//!
//! Java counterpart: `shared/role/Role.java`.

use std::fmt;
use std::pin::Pin;
use std::str::FromStr;

use sqlx::PgConnection;

use crate::id::{TeamId, UserId};

/// A rank within a team. The ladder is Member < Approver < Owner, and a higher rank includes the lower ones.
///
/// `Role` deliberately has no `Ord`: the only question to ask is [`Role::includes`], so "at least an approver"
/// cannot be written the wrong way round with `<`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Member,
    Approver,
    Owner,
}

/// The text is not the name of a [`Role`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown role: {0}")]
pub struct UnknownRole(String);

impl Role {
    /// Every role, lowest rank first.
    pub const ALL: [Self; 3] = [Self::Member, Self::Approver, Self::Owner];

    /// True if this role has at least the rank of `other`: an owner includes approver and member.
    #[must_use]
    pub const fn includes(self, other: Self) -> bool {
        self.rank() >= other.rank()
    }

    /// The name as Java, the database and the API write it: `MEMBER`, `APPROVER`, `OWNER`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "MEMBER",
            Self::Approver => "APPROVER",
            Self::Owner => "OWNER",
        }
    }

    /// The position on the ladder. Adding a role does not compile until it has one.
    const fn rank(self) -> u8 {
        match self {
            Self::Member => 0,
            Self::Approver => 1,
            Self::Owner => 2,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// As its name, `"OWNER"`, like Jackson writes Java's enum.
impl serde::Serialize for Role {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Role {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        name.parse().map_err(serde::de::Error::custom)
    }
}

impl FromStr for Role {
    type Err = UnknownRole;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|role| role.as_str() == s)
            .ok_or_else(|| UnknownRole(s.to_owned()))
    }
}

/// Port: the user's role in the team, `None` when they are not a teammate. The teams module implements it and
/// the app hands it to modules that may not depend on teams (audit), as Java's audit module reaches
/// `@teamAuthorization` by bean name without importing teams (CONCEPT, ADR 0010).
///
/// It takes the caller's connection, so that the role is read in the caller's transaction.
pub trait TeamRoles: fmt::Debug + Send + Sync {
    fn role_of<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team: TeamId,
        user: UserId,
    ) -> Pin<Box<dyn Future<Output = sqlx::Result<Option<Role>>> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder_is_member_approver_owner() {
        use Role::{Approver, Member, Owner};
        // (role, other, role.includes(other))
        let table = [
            (Member, Member, true),
            (Member, Approver, false),
            (Member, Owner, false),
            (Approver, Member, true),
            (Approver, Approver, true),
            (Approver, Owner, false),
            (Owner, Member, true),
            (Owner, Approver, true),
            (Owner, Owner, true),
        ];
        for (role, other, expected) in table {
            assert_eq!(role.includes(other), expected, "{role} includes {other}");
        }
    }

    #[test]
    fn names_are_javas_enum_constants() {
        assert_eq!(Role::Member.to_string(), "MEMBER");
        assert_eq!(Role::Approver.to_string(), "APPROVER");
        assert_eq!(Role::Owner.to_string(), "OWNER");
    }

    #[test]
    fn every_role_parses_from_its_name() {
        for role in Role::ALL {
            assert_eq!(role.as_str().parse::<Role>(), Ok(role));
        }
    }

    #[test]
    fn other_text_is_not_a_role() {
        for text in ["", "owner", "Owner", " OWNER", "ADMIN"] {
            let error = text.parse::<Role>().unwrap_err();
            assert_eq!(error.to_string(), format!("Unknown role: {text}"));
        }
    }
}
