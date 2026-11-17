//! The caller's role in a team, as far as expenses needs to know it.
//!
//! Java counterpart: the port `expenses/internal/application/MembershipLookup.java` and its adapter
//! `expenses/internal/infrastructure/TeamsMembershipLookup.java`, which asks `TeamsFacade`.

use std::fmt;
use std::pin::Pin;

use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;
use ledgerly_teams::TeamsFacade;
use sqlx::PgConnection;

/// A boxed future, so that the trait can be used as `dyn MembershipLookup`.
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Port: the user's role in the team, or `None` when they are not a teammate (callers answer that with 404).
///
/// It takes the use case's connection, so that the adapter reads the membership in the same transaction as the
/// expense, as Java's does.
pub(crate) trait MembershipLookup: fmt::Debug + Send + Sync {
    fn role_of<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team: TeamId,
        user: UserId,
    ) -> BoxFuture<'a, Result<Option<Role>, sqlx::Error>>;
}

/// The adapter: asks the teams module on every call, so that a removal or demotion bites on the next request.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TeamsMembershipLookup(pub TeamsFacade);

impl MembershipLookup for TeamsMembershipLookup {
    fn role_of<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team: TeamId,
        user: UserId,
    ) -> BoxFuture<'a, Result<Option<Role>, sqlx::Error>> {
        Box::pin(self.0.role_of(connection, team, user))
    }
}
