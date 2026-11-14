//! Test doubles for the ports the use cases call: who is a teammate, and which receipts an expense has.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::role::Role;
use sqlx::PgConnection;

use super::MembershipLookup;
use super::membership::BoxFuture;
use crate::{ReceiptChecks, ReceiptSummary};

/// Memberships a test sets up. Clones share them, so a test can change a role after handing a clone over.
#[derive(Debug, Clone, Default)]
pub(crate) struct Memberships(Arc<Mutex<HashMap<(TeamId, UserId), Role>>>);

impl Memberships {
    pub(crate) fn with(self, team: TeamId, user: UserId, role: Role) -> Self {
        self.0.lock().unwrap().insert((team, user), role);
        self
    }
}

impl MembershipLookup for Memberships {
    fn role_of<'a>(
        &'a self,
        _connection: &'a mut PgConnection,
        team: TeamId,
        user: UserId,
    ) -> BoxFuture<'a, Result<Option<Role>, sqlx::Error>> {
        let role = self.0.lock().unwrap().get(&(team, user)).copied();
        Box::pin(std::future::ready(Ok(role)))
    }
}

/// Every expense has the same receipts. Java's tests seed an accepted receipt with SQL; here it is a number.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FixedReceipts(pub ReceiptSummary);

impl ReceiptChecks for FixedReceipts {
    fn summary_for(&self, _team_id: TeamId, _expense_id: ExpenseId) -> ReceiptSummary {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use ledgerly_shared::database::TestDatabase;

    use super::*;
    use crate::application::NoTeamsYet;

    #[tokio::test]
    async fn the_doubles_answer_what_the_test_set_up_and_the_stand_in_knows_nobody() {
        let database = TestDatabase::new().await.unwrap();
        let mut connection = database.pool().acquire().await.unwrap();
        let (team, member, stranger) = (TeamId::new_v7(), UserId::new_v7(), UserId::new_v7());
        let memberships = Memberships::default().with(team, member, Role::Approver);

        assert_eq!(
            memberships.role_of(&mut connection, team, member).await.unwrap(),
            Some(Role::Approver)
        );
        assert_eq!(
            memberships.role_of(&mut connection, team, stranger).await.unwrap(),
            None
        );
        assert_eq!(NoTeamsYet.role_of(&mut connection, team, member).await.unwrap(), None);
        assert_eq!(
            FixedReceipts(ReceiptSummary::new(1, 0)).summary_for(team, ExpenseId::new_v7()),
            ReceiptSummary::new(1, 0)
        );
    }
}
