//! The audit module: the append-only audit trail, written from the events of every other module.
//!
//! Java counterpart: `src/main/java/com/ledgerly/audit/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`. Like Java's, it depends only on the other modules' events, never on their
//! facades: who may read the trail is asked through [`TeamRoles`], which the app hands over (Java reaches
//! `@teamAuthorization` by bean name).

#![forbid(unsafe_code)]

mod details;
mod listener;
mod trail;
mod web;

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::Limits;
use ledgerly_shared::events::Listener;
use ledgerly_shared::role::TeamRoles;
use ledgerly_shared::web::Routes;
use sqlx::PgPool;

pub use listener::AUDITED_EVENT_TYPES;

use crate::listener::AuditListener;
use crate::web::AuditApi;

/// The audit module as the app wires it.
#[derive(Debug, Clone)]
pub struct Audit {
    api: AuditApi,
    listener: AuditListener,
}

impl Audit {
    /// Rows are stamped with `clock`; readers' roles come from `roles`; pages hold at most
    /// `limits.max_page_size` rows.
    #[must_use]
    pub fn new(pool: &PgPool, clock: &Arc<dyn Clock>, roles: Arc<dyn TeamRoles>, limits: &Limits) -> Self {
        Self {
            api: AuditApi {
                pool: pool.clone(),
                roles,
                max_page_size: limits.max_page_size,
            },
            listener: AuditListener {
                pool: pool.clone(),
                clock: clock.clone(),
            },
        }
    }

    /// `GET /teams/{teamId}/audit-events`.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.api.clone().routes()
    }

    /// The one listener of all 25 audited event types. Java: `AuditListeners`.
    #[must_use]
    pub fn listeners(&self) -> Vec<Arc<dyn Listener>> {
        vec![Arc::new(self.listener.clone())]
    }
}
