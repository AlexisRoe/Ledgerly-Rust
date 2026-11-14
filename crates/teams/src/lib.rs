//! The teams module: teams, memberships, roles, invitations and `/me`.
//!
//! Java counterpart: `src/main/java/com/ledgerly/teams/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod application;
mod domain;
mod persistence;
mod web;

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::web::Routes;
use sqlx::PgPool;

use crate::application::GetMe;
use crate::web::TeamsApi;

/// The teams module as the app wires it.
#[derive(Debug, Clone)]
pub struct Teams {
    api: TeamsApi,
}

impl Teams {
    #[must_use]
    pub fn new(pool: &PgPool, clock: &Arc<dyn Clock>) -> Self {
        Self {
            api: TeamsApi {
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
