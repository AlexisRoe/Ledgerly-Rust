//! The role a teammate holds in a team.
//!
//! Java counterpart: `shared/role/Role.java`.

use std::fmt;
use std::str::FromStr;

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

impl FromStr for Role {
    type Err = UnknownRole;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|role| role.as_str() == s)
            .ok_or_else(|| UnknownRole(s.to_owned()))
    }
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
