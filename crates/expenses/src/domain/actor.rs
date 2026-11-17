use ledgerly_shared::id::UserId;
use ledgerly_shared::role::Role;

/// Who is acting, with their role in the expense's team.
///
/// The domain is handed the role; it never looks it up. Whoever builds an `Actor` has therefore already checked
/// that the user is a teammate.
///
/// Java counterpart: `expenses/internal/domain/Actor.java`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    id: UserId,
    role: Role,
}

impl Actor {
    #[must_use]
    pub const fn new(id: UserId, role: Role) -> Self {
        Self { id, role }
    }

    #[must_use]
    pub const fn id(&self) -> UserId {
        self.id
    }

    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_actor_is_a_user_with_their_role_in_the_team() {
        let id = UserId::new_v7();
        let actor = Actor::new(id, Role::Approver);
        assert_eq!(actor.id(), id);
        assert_eq!(actor.role(), Role::Approver);
    }
}
