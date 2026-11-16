//! The teams module: teams, memberships, roles, invitations and `/me`.
//!
//! Java counterpart: `src/main/java/com/ledgerly/teams/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod application;
mod domain;
pub mod events;
mod persistence;
mod web;

use std::pin::Pin;
use std::sync::Arc;

use chrono::TimeDelta;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::TeamsConfig;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::{Role, TeamRoles};
use ledgerly_shared::web::Routes;
use sqlx::{PgConnection, PgPool};

use crate::application::{
    AcceptInvitation, ChangeMemberRole, Context, CreateTeam, DeclineInvitation, GetMe, InviteMember, LeaveTeam,
    RemoveMember, RevokeInvitation, TeamAuthorization,
};
use crate::web::TeamsApi;

/// The teams module's answer to other modules' one question: what role does this user have in this team? Java:
/// `TeamsFacade` and `TeamsFacadeImpl`.
///
/// It reads the membership on every call, so a removal or demotion takes effect on the next request; nothing
/// about roles is in the access token.
#[derive(Debug, Clone, Copy, Default)]
pub struct TeamsFacade;

impl TeamsFacade {
    /// The user's role in the team, `None` when they are not a teammate. It reads in the caller's transaction.
    ///
    /// # Errors
    ///
    /// The database error.
    pub async fn role_of(
        self,
        connection: &mut PgConnection,
        team: TeamId,
        user: UserId,
    ) -> sqlx::Result<Option<Role>> {
        persistence::role_of(connection, team, user).await
    }
}

impl TeamRoles for TeamsFacade {
    fn role_of<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team: TeamId,
        user: UserId,
    ) -> Pin<Box<dyn Future<Output = sqlx::Result<Option<Role>>> + Send + 'a>> {
        Box::pin(Self::role_of(*self, connection, team, user))
    }
}

/// The teams module as the app wires it.
#[derive(Debug, Clone)]
pub struct Teams {
    api: TeamsApi,
}

impl Teams {
    /// Invitations can be answered for `config.invitation_ttl`.
    #[must_use]
    pub fn new(config: &TeamsConfig, pool: &PgPool, clock: &Arc<dyn Clock>, events: &Arc<dyn Publisher>) -> Self {
        let context = Context {
            pool: pool.clone(),
            clock: clock.clone(),
            events: events.clone(),
        };
        let ttl = TimeDelta::from_std(config.invitation_ttl).unwrap_or(TimeDelta::MAX);
        Self {
            api: TeamsApi {
                authorization: TeamAuthorization::new(pool.clone()),
                create_team: CreateTeam::new(context.clone()),
                invite: InviteMember::new(context.clone(), ttl),
                revoke: RevokeInvitation::new(context.clone()),
                accept: AcceptInvitation::new(context.clone()),
                decline: DeclineInvitation::new(context.clone()),
                change_role: ChangeMemberRole::new(context.clone()),
                remove: RemoveMember::new(context.clone()),
                leave: LeaveTeam::new(context),
                get_me: GetMe::new(pool.clone(), clock.clone()),
            },
        }
    }

    /// What other modules ask the teams module.
    #[must_use]
    pub const fn facade(&self) -> TeamsFacade {
        TeamsFacade
    }

    /// The teams endpoints and `GET /me`.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.api.clone().routes()
    }
}
