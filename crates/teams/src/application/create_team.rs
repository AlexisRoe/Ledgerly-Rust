//! Creating a team. Java: `teams/internal/application/CreateTeam.java`.

use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;

use super::{Context, UseCaseError};
use crate::domain::team_name;
use crate::events::{TeamEvent, TeamEventKind};
use crate::persistence;

/// The new team's id and its name as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreatedTeam {
    pub id: TeamId,
    pub name: String,
}

/// Creates a team and makes the caller its first OWNER, in one transaction.
#[derive(Debug, Clone)]
pub(crate) struct CreateTeam {
    context: Context,
}

impl CreateTeam {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    pub(crate) async fn execute(&self, name: &str, creator: UserId) -> Result<CreatedTeam, UseCaseError> {
        let now = self.context.clock.now();
        let team = CreatedTeam {
            id: TeamId::new_v7(),
            name: team_name(name),
        };
        let mut transaction = self.context.pool.begin().await?;
        persistence::insert_team(&mut transaction, team.id, &team.name, now).await?;
        persistence::insert_membership(&mut transaction, team.id, creator, Role::Owner, now).await?;
        let event = TeamEvent::new(team.id, creator, now, TeamEventKind::TeamCreated);
        self.context.events.publish(&mut transaction, &event).await?;
        transaction.commit().await?;
        Ok(team)
    }
}
