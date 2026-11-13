//! The caller's role in a team, as far as expenses needs to know it.
//!
//! Java counterpart: the port `expenses/internal/application/MembershipLookup.java` and its adapter
//! `expenses/internal/infrastructure/TeamsMembershipLookup.java`, which asks `TeamsFacade`. The adapter arrives
//! with the teams module (M6); until then [`NoTeamsYet`] stands in.

use std::fmt;
use std::pin::Pin;

use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;
use sqlx::PgConnection;

/// A boxed future, so that the trait can be used as `dyn MembershipLookup`.
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Port: the user's role in the team, or `None` when they are not a teammate (callers answer that with 404).
///
/// It takes the use case's connection, so that the M6 adapter reads the membership in the same transaction as
/// the expense, as Java's does.
pub(crate) trait MembershipLookup: fmt::Debug + Send + Sync {
    fn role_of<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team: TeamId,
        user: UserId,
    ) -> BoxFuture<'a, Result<Option<Role>, sqlx::Error>>;
}

/// Until the teams module exists (M6), nobody is a member of any team, so every caller gets 404.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoTeamsYet;

impl MembershipLookup for NoTeamsYet {
    fn role_of<'a>(
        &'a self,
        _connection: &'a mut PgConnection,
        _team: TeamId,
        _user: UserId,
    ) -> BoxFuture<'a, Result<Option<Role>, sqlx::Error>> {
        Box::pin(std::future::ready(Ok(None)))
    }
}
