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

use std::sync::Arc;

use chrono::TimeDelta;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::TeamsConfig;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::web::Routes;
use sqlx::PgPool;

use crate::application::{
    AcceptInvitation, ChangeMemberRole, Context, CreateTeam, DeclineInvitation, GetMe, InviteMember, LeaveTeam,
    RemoveMember, RevokeInvitation, TeamAuthorization,
};
use crate::web::TeamsApi;

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

    /// The teams endpoints and `GET /me`.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.api.clone().routes()
    }
}
