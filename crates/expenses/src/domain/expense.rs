use chrono::NaiveDate;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::money::Money;
use ledgerly_shared::role::Role;

use super::action::Action;
use super::actor::Actor;
use super::category::Category;
use super::error::{ExpenseError, Forbidden};
use super::status::ExpenseStatus;
use super::text::{InvalidInput, Merchant, Note, RejectionReason};
use crate::ReceiptSummary;

/// The field values of an expense as a caller hands them in: for a new draft, or as the complete new state
/// of an edit.
///
/// `money`, `category` and `incurred_on` are already typed. `merchant` and `note` are still plain text, because
/// Java checks them inside the aggregate, after it has checked who is acting and in which status the expense is.
/// Keeping that order decides whether a client gets 403, 409 or 400.
///
/// Java counterpart: `expenses/NewExpense.java`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExpense {
    pub money: Money,
    pub category: Category,
    pub incurred_on: NaiveDate,
    pub merchant: String,
    pub note: Option<String>,
}

impl NewExpense {
    /// Runs the field rules without creating anything, so that batch and import can check a row before they
    /// write it. Java: `Expense.checkFields`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` naming the first field that breaks a rule, in the order merchant, note, date.
    pub fn check(&self, clock: &dyn Clock) -> Result<(), InvalidInput> {
        self.checked(clock).map(|_| ())
    }

    /// The merchant and note as types that can only hold valid text, once the date is known not to be in the
    /// future.
    fn checked(&self, clock: &dyn Clock) -> Result<(Merchant, Option<Note>), InvalidInput> {
        let merchant = Merchant::new(&self.merchant)?;
        let note = self.note.as_deref().map(Note::new).transpose()?;
        if self.incurred_on > clock.today() {
            return Err(InvalidInput::new("incurredOn", "incurredOn must not be in the future"));
        }
        Ok((merchant, note))
    }
}

/// One rejection of an expense. Every rejection is kept; none is overwritten.
///
/// Java counterpart: `expenses/internal/domain/Rejection.java`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    rejected_by: UserId,
    reason: RejectionReason,
}

impl Rejection {
    #[must_use]
    pub const fn rejected_by(&self) -> UserId {
        self.rejected_by
    }

    #[must_use]
    pub const fn reason(&self) -> &RejectionReason {
        &self.reason
    }
}

/// A single amount spent on behalf of a team, claimed by one claimant, moving through the approval workflow.
///
/// The fields are private and there is no setter: the status changes only through the six transitions, the
/// details only through [`Expense::edit`], and the claimant and team never. Every method that can refuse returns
/// a `Result`, and one that refuses leaves the expense as it was.
///
/// Java counterpart: `expenses/internal/domain/Expense.java`, without its JPA parts (version and timestamps
/// arrive with persistence in M2).
#[derive(Debug, Clone)]
pub struct Expense {
    id: ExpenseId,
    team_id: TeamId,
    claimant_id: UserId,
    money: Money,
    category: Category,
    incurred_on: NaiveDate,
    merchant: Merchant,
    note: Option<Note>,
    status: ExpenseStatus,
    rejections: Vec<Rejection>,
}

impl Expense {
    /// Creates a new draft with a fresh id after checking the field rules.
    ///
    /// # Errors
    ///
    /// `InvalidInput` naming the first field that breaks a rule, in the order merchant, note, date.
    pub fn draft(
        team_id: TeamId,
        claimant_id: UserId,
        new: &NewExpense,
        clock: &dyn Clock,
    ) -> Result<Self, InvalidInput> {
        let (merchant, note) = new.checked(clock)?;
        Ok(Self {
            id: ExpenseId::new_v7(),
            team_id,
            claimant_id,
            money: new.money,
            category: new.category,
            incurred_on: new.incurred_on,
            merchant,
            note,
            status: ExpenseStatus::Draft,
            rejections: Vec::new(),
        })
    }

    // --- transitions ------------------------------------------------------------------------------------------

    /// Draft → Submitted. Only the claimant, and only with at least one accepted receipt and no scan pending.
    ///
    /// # Errors
    ///
    /// `Forbidden`, `InvalidTransition` or `RuleViolated`, checked in that order.
    pub fn submit(&mut self, actor: Actor, receipts: ReceiptSummary) -> Result<(), ExpenseError> {
        let target = self.allowed_target(actor, Action::Submit)?;
        if receipts.accepted() < 1 {
            return Err(ExpenseError::RuleViolated(
                "At least one accepted receipt is required to submit",
            ));
        }
        if receipts.pending_scan() > 0 {
            return Err(ExpenseError::RuleViolated("Receipts are still being scanned"));
        }
        self.status = target;
        Ok(())
    }

    /// Submitted → Draft. Only the claimant.
    ///
    /// # Errors
    ///
    /// `Forbidden` or `InvalidTransition`, checked in that order.
    pub fn withdraw(&mut self, actor: Actor) -> Result<(), ExpenseError> {
        self.status = self.allowed_target(actor, Action::Withdraw)?;
        Ok(())
    }

    /// Submitted → Approved. Needs an approver or owner; never the claimant.
    ///
    /// # Errors
    ///
    /// `Forbidden`, `SelfDealing` or `InvalidTransition`, checked in that order.
    pub fn approve(&mut self, actor: Actor) -> Result<(), ExpenseError> {
        self.status = self.allowed_target(actor, Action::Approve)?;
        Ok(())
    }

    /// Submitted → Rejected, with a reason that is added to the history. Needs an approver or owner; never the
    /// claimant.
    ///
    /// The reason arrives as plain text and is checked last, as in Java: a member who sends a blank reason is
    /// told they may not reject, not that the reason is missing.
    ///
    /// # Errors
    ///
    /// `Forbidden`, `SelfDealing`, `InvalidTransition` or `InvalidInput` for field `reason`, checked in that
    /// order.
    pub fn reject(&mut self, actor: Actor, reason: &str) -> Result<(), ExpenseError> {
        let target = self.allowed_target(actor, Action::Reject)?;
        let reason = RejectionReason::new(reason)?;
        self.rejections.push(Rejection {
            rejected_by: actor.id(),
            reason,
        });
        self.status = target;
        Ok(())
    }

    /// Rejected → Draft, so that the claimant can fix the expense and submit it again. Only the claimant.
    ///
    /// # Errors
    ///
    /// `Forbidden` or `InvalidTransition`, checked in that order.
    pub fn reopen(&mut self, actor: Actor) -> Result<(), ExpenseError> {
        self.status = self.allowed_target(actor, Action::Reopen)?;
        Ok(())
    }

    /// Approved → Reimbursed. Needs an owner; never the claimant.
    ///
    /// # Errors
    ///
    /// `Forbidden`, `SelfDealing` or `InvalidTransition`, checked in that order.
    pub fn reimburse(&mut self, actor: Actor) -> Result<(), ExpenseError> {
        self.status = self.allowed_target(actor, Action::Reimburse)?;
        Ok(())
    }

    // --- draft-only operations --------------------------------------------------------------------------------

    /// Replaces the details of a draft. Only the claimant.
    ///
    /// # Errors
    ///
    /// `Forbidden`, `NotDraft` or `InvalidInput`, checked in that order. Nothing changes on an error.
    pub fn edit(&mut self, actor: Actor, new: &NewExpense, clock: &dyn Clock) -> Result<(), ExpenseError> {
        self.require_draft_of(actor, Forbidden::EditByNonClaimant)?;
        let (merchant, note) = new.checked(clock)?;
        self.money = new.money;
        self.category = new.category;
        self.incurred_on = new.incurred_on;
        self.merchant = merchant;
        self.note = note;
        Ok(())
    }

    /// Whether the actor may add or remove receipts: the claimant, while it is a draft.
    ///
    /// # Errors
    ///
    /// `Forbidden` or `NotDraft`, checked in that order.
    pub fn require_can_change_receipts(&self, actor: Actor) -> Result<(), ExpenseError> {
        self.require_draft_of(actor, Forbidden::ChangeReceiptsByNonClaimant)
    }

    /// Whether the receipts controls should be offered: claimant and still a draft.
    #[must_use]
    pub fn can_change_receipts(&self, actor: Actor) -> bool {
        self.is_draft_of(actor)
    }

    /// Whether the actor may delete this expense: the claimant, while it is a draft.
    ///
    /// # Errors
    ///
    /// `Forbidden` or `NotDraft`, checked in that order.
    pub fn require_deletable_by(&self, actor: Actor) -> Result<(), ExpenseError> {
        self.require_draft_of(actor, Forbidden::DeleteByNonClaimant)
    }

    /// Whether the delete control should be offered: claimant and still a draft.
    #[must_use]
    pub fn can_delete(&self, actor: Actor) -> bool {
        self.is_draft_of(actor)
    }

    /// The claimant sees their own expenses; approvers and owners see the whole team's.
    #[must_use]
    pub fn is_visible_to(&self, actor: Actor) -> bool {
        self.is_claimed_by(actor) || actor.role().includes(Role::Approver)
    }

    // --- accessors --------------------------------------------------------------------------------------------

    #[must_use]
    pub const fn id(&self) -> ExpenseId {
        self.id
    }

    #[must_use]
    pub const fn team_id(&self) -> TeamId {
        self.team_id
    }

    #[must_use]
    pub const fn claimant_id(&self) -> UserId {
        self.claimant_id
    }

    #[must_use]
    pub const fn money(&self) -> Money {
        self.money
    }

    #[must_use]
    pub const fn category(&self) -> Category {
        self.category
    }

    #[must_use]
    pub const fn incurred_on(&self) -> NaiveDate {
        self.incurred_on
    }

    #[must_use]
    pub const fn merchant(&self) -> &Merchant {
        &self.merchant
    }

    #[must_use]
    pub const fn note(&self) -> Option<&Note> {
        self.note.as_ref()
    }

    /// Where the expense is in its lifecycle.
    #[must_use]
    pub const fn status(&self) -> ExpenseStatus {
        self.status
    }

    /// Every rejection so far, oldest first.
    #[must_use]
    pub fn rejections(&self) -> &[Rejection] {
        &self.rejections
    }

    // --- helpers ----------------------------------------------------------------------------------------------

    /// The status `action` leads to, if `actor` may perform it and the current status allows it.
    ///
    /// Who is acting is checked before where the expense stands, as in Java: the order decides whether a client
    /// sees 403 or 409.
    fn allowed_target(&self, actor: Actor, action: Action) -> Result<ExpenseStatus, ExpenseError> {
        self.authorize(actor, action)?;
        action.target_from(self.status).ok_or(ExpenseError::InvalidTransition {
            from: self.status,
            action,
        })
    }

    /// Who may perform which action. Exhaustive: a new action does not compile until it is listed here.
    fn authorize(&self, actor: Actor, action: Action) -> Result<(), ExpenseError> {
        match action {
            Action::Submit | Action::Withdraw | Action::Reopen => {
                if self.is_claimed_by(actor) {
                    Ok(())
                } else {
                    Err(Forbidden::NotClaimant(action).into())
                }
            }
            Action::Approve | Action::Reject => self.require_decider(actor, Role::Approver, action),
            Action::Reimburse => self.require_decider(actor, Role::Owner, action),
        }
    }

    /// Deciding on an expense needs a high enough role and must not be self-dealing.
    fn require_decider(&self, actor: Actor, minimum: Role, action: Action) -> Result<(), ExpenseError> {
        if !actor.role().includes(minimum) {
            return Err(Forbidden::RoleTooLow {
                role: actor.role(),
                action,
            }
            .into());
        }
        if self.is_claimed_by(actor) {
            return Err(ExpenseError::SelfDealing { action });
        }
        Ok(())
    }

    fn require_draft_of(&self, actor: Actor, otherwise: Forbidden) -> Result<(), ExpenseError> {
        if !self.is_claimed_by(actor) {
            return Err(otherwise.into());
        }
        match self.status {
            ExpenseStatus::Draft => Ok(()),
            status @ (ExpenseStatus::Submitted
            | ExpenseStatus::Approved
            | ExpenseStatus::Rejected
            | ExpenseStatus::Reimbursed) => Err(ExpenseError::NotDraft { status }),
        }
    }

    fn is_draft_of(&self, actor: Actor) -> bool {
        self.status == ExpenseStatus::Draft && self.is_claimed_by(actor)
    }

    fn is_claimed_by(&self, actor: Actor) -> bool {
        actor.id() == self.claimant_id
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use chrono::Days;
    use ledgerly_shared::clock::FixedClock;
    use ledgerly_shared::id::TeamId;
    use ledgerly_shared::role::Role;

    use super::*;
    use crate::domain::error::Forbidden;

    // The fixtures of Java's ExpenseTest, with the same values.

    fn clock() -> FixedClock {
        FixedClock::at("2026-11-05T10:00:00Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap())
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 5).unwrap()
    }

    const READY: ReceiptSummary = ReceiptSummary::new(1, 0);

    static CLAIMANT_ID: LazyLock<UserId> = LazyLock::new(UserId::new_v7);
    static CLAIMANT: LazyLock<Actor> = LazyLock::new(|| Actor::new(*CLAIMANT_ID, Role::Member));
    static APPROVER: LazyLock<Actor> = LazyLock::new(|| Actor::new(UserId::new_v7(), Role::Approver));
    static OWNER: LazyLock<Actor> = LazyLock::new(|| Actor::new(UserId::new_v7(), Role::Owner));
    static OTHER_MEMBER: LazyLock<Actor> = LazyLock::new(|| Actor::new(UserId::new_v7(), Role::Member));

    fn money(amount: &str) -> Money {
        Money::parse(amount, "EUR").unwrap()
    }

    fn fields(
        amount: &str,
        category: Category,
        incurred_on: NaiveDate,
        merchant: &str,
        note: Option<&str>,
    ) -> NewExpense {
        NewExpense {
            money: money(amount),
            category,
            incurred_on,
            merchant: merchant.to_owned(),
            note: note.map(str::to_owned),
        }
    }

    fn lunch() -> NewExpense {
        fields("12.50", Category::Meals, today(), "Cafe Central", Some("Lunch"))
    }

    fn try_draft(new: &NewExpense) -> Result<Expense, InvalidInput> {
        Expense::draft(TeamId::new_v7(), *CLAIMANT_ID, new, &clock())
    }

    fn draft() -> Expense {
        try_draft(&lunch()).unwrap()
    }

    fn in_status(target: ExpenseStatus) -> Expense {
        let mut e = draft();
        match target {
            ExpenseStatus::Draft => {}
            ExpenseStatus::Submitted => e.submit(*CLAIMANT, READY).unwrap(),
            ExpenseStatus::Rejected => {
                e.submit(*CLAIMANT, READY).unwrap();
                e.reject(*APPROVER, "no receipt for tip").unwrap();
            }
            ExpenseStatus::Approved => {
                e.submit(*CLAIMANT, READY).unwrap();
                e.approve(*APPROVER).unwrap();
            }
            ExpenseStatus::Reimbursed => {
                e.submit(*CLAIMANT, READY).unwrap();
                e.approve(*APPROVER).unwrap();
                e.reimburse(*OWNER).unwrap();
            }
        }
        assert_eq!(e.status(), target);
        e
    }

    fn invalid(field: &'static str, message: &str) -> InvalidInput {
        InvalidInput {
            field,
            message: message.to_owned(),
        }
    }

    mod creation {
        use super::*;

        #[test]
        fn starts_as_draft() {
            let e = draft();
            assert_eq!(e.status(), ExpenseStatus::Draft);
            assert_eq!(e.id().as_uuid().get_version_num(), 7);
            assert_eq!(e.claimant_id(), *CLAIMANT_ID);
            assert!(e.rejections().is_empty());
        }

        #[test]
        fn keeps_what_it_was_given() {
            let team = TeamId::new_v7();
            let e = Expense::draft(team, *CLAIMANT_ID, &lunch(), &clock()).unwrap();
            assert_eq!(e.team_id(), team);
            assert_eq!(e.money(), money("12.50"));
            assert_eq!(e.category(), Category::Meals);
            assert_eq!(e.incurred_on(), today());
            assert_eq!(e.merchant().as_str(), "Cafe Central");
            assert_eq!(e.note().map(Note::as_str), Some("Lunch"));
        }

        #[test]
        fn every_draft_gets_its_own_id() {
            assert_ne!(draft().id(), draft().id());
        }

        #[test]
        fn future_date_is_rejected() {
            let tomorrow = today().checked_add_days(Days::new(1)).unwrap();
            assert_eq!(
                try_draft(&fields("1", Category::Other, tomorrow, "x", None)).unwrap_err(),
                invalid("incurredOn", "incurredOn must not be in the future")
            );
        }

        #[test]
        fn today_is_allowed() {
            assert_eq!(draft().incurred_on(), today());
        }

        #[test]
        fn today_is_the_clocks_utc_date() {
            // One second before midnight UTC, "tomorrow" is still in the future; one second later it is today.
            let clock = FixedClock::at("2026-11-05T23:59:59Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap());
            let tomorrow = today().checked_add_days(Days::new(1)).unwrap();
            let new = fields("1", Category::Other, tomorrow, "x", None);
            assert!(Expense::draft(TeamId::new_v7(), *CLAIMANT_ID, &new, &clock).is_err());
            clock.advance(chrono::TimeDelta::seconds(1));
            assert!(Expense::draft(TeamId::new_v7(), *CLAIMANT_ID, &new, &clock).is_ok());
        }

        #[test]
        fn merchant_is_required() {
            for merchant in ["", "   "] {
                assert_eq!(
                    try_draft(&fields("1", Category::Other, today(), merchant, None)).unwrap_err(),
                    invalid("merchant", "Merchant is required")
                );
            }
        }

        #[test]
        fn merchant_and_note_are_capped() {
            let long_merchant = "m".repeat(Merchant::MAX_LEN + 1);
            assert_eq!(
                try_draft(&fields("1", Category::Other, today(), &long_merchant, None)).unwrap_err(),
                invalid("merchant", "Merchant is longer than 200")
            );
            let long_note = "n".repeat(Note::MAX_LEN + 1);
            assert_eq!(
                try_draft(&fields("1", Category::Other, today(), "m", Some(&long_note))).unwrap_err(),
                invalid("note", "Note is longer than 2000")
            );
        }

        #[test]
        fn merchant_is_stripped_and_note_is_not() {
            let e = try_draft(&fields("1", Category::Other, today(), "  Bahn ", Some(" Ticket "))).unwrap();
            assert_eq!(e.merchant().as_str(), "Bahn");
            assert_eq!(e.note().map(Note::as_str), Some(" Ticket "));
        }

        #[test]
        fn a_missing_note_stays_missing() {
            let e = try_draft(&fields("1", Category::Other, today(), "x", None)).unwrap();
            assert_eq!(e.note(), None);
        }

        #[test]
        fn the_first_broken_field_is_named_in_javas_order_merchant_note_date() {
            let tomorrow = today().checked_add_days(Days::new(1)).unwrap();
            let long_note = "n".repeat(Note::MAX_LEN + 1);
            let all_wrong = fields("1", Category::Other, tomorrow, " ", Some(&long_note));
            assert_eq!(try_draft(&all_wrong).unwrap_err().field, "merchant");
            let note_and_date_wrong = fields("1", Category::Other, tomorrow, "x", Some(&long_note));
            assert_eq!(try_draft(&note_and_date_wrong).unwrap_err().field, "note");
        }

        #[test]
        fn check_runs_the_same_rules_without_creating_anything() {
            let tomorrow = today().checked_add_days(Days::new(1)).unwrap();
            assert_eq!(lunch().check(&clock()), Ok(()));
            assert_eq!(
                fields("1", Category::Other, today(), "", None).check(&clock()),
                Err(invalid("merchant", "Merchant is required"))
            );
            assert_eq!(
                fields("1", Category::Other, tomorrow, "x", None).check(&clock()),
                Err(invalid("incurredOn", "incurredOn must not be in the future"))
            );
        }
    }

    mod transitions {
        use super::*;

        /// An actor who is allowed to perform the action.
        fn valid_actor(action: Action) -> Actor {
            match action {
                Action::Submit | Action::Withdraw | Action::Reopen => *CLAIMANT,
                Action::Approve | Action::Reject => *APPROVER,
                Action::Reimburse => *OWNER,
            }
        }

        fn perform_as(e: &mut Expense, action: Action, actor: Actor) -> Result<(), ExpenseError> {
            match action {
                Action::Submit => e.submit(actor, READY),
                Action::Withdraw => e.withdraw(actor),
                Action::Approve => e.approve(actor),
                Action::Reject => e.reject(actor, "reason"),
                Action::Reopen => e.reopen(actor),
                Action::Reimburse => e.reimburse(actor),
            }
        }

        fn perform(e: &mut Expense, action: Action) -> Result<(), ExpenseError> {
            perform_as(e, action, valid_actor(action))
        }

        /// The workflow as Java's test spells it out, independently of the implementation.
        fn legal_target(from: ExpenseStatus, action: Action) -> Option<ExpenseStatus> {
            use ExpenseStatus::{Approved, Draft, Reimbursed, Rejected, Submitted};
            let (source, target) = match action {
                Action::Submit => (Draft, Submitted),
                Action::Withdraw => (Submitted, Draft),
                Action::Approve => (Submitted, Approved),
                Action::Reject => (Submitted, Rejected),
                Action::Reopen => (Rejected, Draft),
                Action::Reimburse => (Approved, Reimbursed),
            };
            (from == source).then_some(target)
        }

        #[test]
        fn legal_pairs_succeed_and_the_rest_are_invalid_transitions() {
            let mut legal = 0;
            let mut illegal = 0;
            for from in ExpenseStatus::ALL {
                for action in Action::ALL {
                    let mut e = in_status(from);
                    let result = perform(&mut e, action);
                    match legal_target(from, action) {
                        Some(expected) => {
                            assert_eq!(result, Ok(()), "{from} + {action}");
                            assert_eq!(e.status(), expected, "{from} + {action}");
                            legal += 1;
                        }
                        None => {
                            assert_eq!(
                                result,
                                Err(ExpenseError::InvalidTransition { from, action }),
                                "{from} + {action}"
                            );
                            assert_eq!(e.status(), from, "{from} + {action} must not change the status");
                            illegal += 1;
                        }
                    }
                }
            }
            assert_eq!((legal, illegal), (6, 24));
        }

        #[test]
        fn an_invalid_transition_says_what_was_tried_from_where() {
            let error = in_status(ExpenseStatus::Draft).approve(*APPROVER).unwrap_err();
            assert_eq!(error.to_string(), "Expense is DRAFT and cannot APPROVE");
        }

        #[test]
        fn only_the_claimant_may_submit_withdraw_or_reopen() {
            let cases = [
                (ExpenseStatus::Draft, Action::Submit, *OWNER),
                (ExpenseStatus::Submitted, Action::Withdraw, *OWNER),
                (ExpenseStatus::Rejected, Action::Reopen, *APPROVER),
                (ExpenseStatus::Draft, Action::Submit, *OTHER_MEMBER),
            ];
            for (status, action, actor) in cases {
                let mut e = in_status(status);
                let error = perform_as(&mut e, action, actor).unwrap_err();
                assert_eq!(error, ExpenseError::Forbidden(Forbidden::NotClaimant(action)));
                assert_eq!(error.to_string(), format!("Only the claimant may {action}"));
                assert_eq!(e.status(), status);
            }
        }

        #[test]
        fn member_cannot_approve_or_reject() {
            for action in [Action::Approve, Action::Reject] {
                let mut e = in_status(ExpenseStatus::Submitted);
                let error = perform_as(&mut e, action, *OTHER_MEMBER).unwrap_err();
                assert_eq!(
                    error,
                    ExpenseError::Forbidden(Forbidden::RoleTooLow {
                        role: Role::Member,
                        action
                    })
                );
                assert_eq!(error.to_string(), format!("Role MEMBER may not {action}"));
                assert_eq!(e.status(), ExpenseStatus::Submitted);
            }
        }

        #[test]
        fn owner_may_approve_and_reject() {
            let mut a = in_status(ExpenseStatus::Submitted);
            a.approve(*OWNER).unwrap();
            assert_eq!(a.status(), ExpenseStatus::Approved);
            let mut r = in_status(ExpenseStatus::Submitted);
            r.reject(*OWNER, "no").unwrap();
            assert_eq!(r.status(), ExpenseStatus::Rejected);
        }

        #[test]
        fn only_owner_may_reimburse() {
            let error = in_status(ExpenseStatus::Approved).reimburse(*APPROVER).unwrap_err();
            assert_eq!(error.to_string(), "Role APPROVER may not REIMBURSE");
            // The claimant here is a MEMBER, so the role refuses before self-dealing is even considered.
            let error = in_status(ExpenseStatus::Approved).reimburse(*CLAIMANT).unwrap_err();
            assert_eq!(
                error,
                ExpenseError::Forbidden(Forbidden::RoleTooLow {
                    role: Role::Member,
                    action: Action::Reimburse
                })
            );
        }

        #[test]
        fn self_dealing_on_approve_and_reject_is_refused() {
            for role in [Role::Approver, Role::Owner] {
                let myself = Actor::new(*CLAIMANT_ID, role);
                for action in [Action::Approve, Action::Reject] {
                    let mut e = in_status(ExpenseStatus::Submitted);
                    let error = perform_as(&mut e, action, myself).unwrap_err();
                    assert_eq!(error, ExpenseError::SelfDealing { action });
                    assert_eq!(e.status(), ExpenseStatus::Submitted);
                }
            }
        }

        #[test]
        fn self_dealing_on_reimburse_is_refused() {
            let myself = Actor::new(*CLAIMANT_ID, Role::Owner);
            let error = in_status(ExpenseStatus::Approved).reimburse(myself).unwrap_err();
            assert_eq!(
                error,
                ExpenseError::SelfDealing {
                    action: Action::Reimburse
                }
            );
            assert_eq!(error.to_string(), "Claimants cannot REIMBURSE their own expense");
        }

        #[test]
        fn who_is_checked_before_where_the_expense_stands() {
            // Java checks role and claimant first, then the status. The order decides between 403 and 409.
            let error = in_status(ExpenseStatus::Draft).approve(*OTHER_MEMBER).unwrap_err();
            assert!(matches!(error, ExpenseError::Forbidden(_)), "{error:?}");
            let myself = Actor::new(*CLAIMANT_ID, Role::Owner);
            let error = in_status(ExpenseStatus::Draft).approve(myself).unwrap_err();
            assert_eq!(
                error,
                ExpenseError::SelfDealing {
                    action: Action::Approve
                }
            );
            let error = in_status(ExpenseStatus::Approved).submit(*OWNER, READY).unwrap_err();
            assert!(matches!(error, ExpenseError::Forbidden(_)), "{error:?}");
        }

        #[test]
        fn submit_needs_an_accepted_receipt() {
            let mut e = draft();
            let error = e.submit(*CLAIMANT, ReceiptSummary::new(0, 0)).unwrap_err();
            assert_eq!(
                error,
                ExpenseError::RuleViolated("At least one accepted receipt is required to submit")
            );
            assert_eq!(error.to_string(), "At least one accepted receipt is required to submit");
            assert_eq!(e.status(), ExpenseStatus::Draft);
        }

        #[test]
        fn submit_needs_no_pending_scans() {
            let mut e = draft();
            let error = e.submit(*CLAIMANT, ReceiptSummary::new(2, 1)).unwrap_err();
            assert_eq!(error, ExpenseError::RuleViolated("Receipts are still being scanned"));
            assert_eq!(e.status(), ExpenseStatus::Draft);
        }

        #[test]
        fn submit_reports_the_missing_receipt_before_the_pending_scan() {
            let error = draft().submit(*CLAIMANT, ReceiptSummary::new(0, 1)).unwrap_err();
            assert_eq!(
                error,
                ExpenseError::RuleViolated("At least one accepted receipt is required to submit")
            );
        }

        #[test]
        fn submit_checks_claimant_and_status_before_receipts() {
            let nothing = ReceiptSummary::new(0, 0);
            let error = draft().submit(*OWNER, nothing).unwrap_err();
            assert!(matches!(error, ExpenseError::Forbidden(_)), "{error:?}");
            let error = in_status(ExpenseStatus::Submitted)
                .submit(*CLAIMANT, nothing)
                .unwrap_err();
            assert!(matches!(error, ExpenseError::InvalidTransition { .. }), "{error:?}");
        }

        #[test]
        fn reject_needs_a_reason() {
            for reason in ["", "  "] {
                let mut e = in_status(ExpenseStatus::Submitted);
                let error = e.reject(*APPROVER, reason).unwrap_err();
                assert_eq!(
                    error,
                    ExpenseError::InvalidInput(invalid("reason", "A rejection reason is required"))
                );
                assert_eq!(e.status(), ExpenseStatus::Submitted);
                assert!(e.rejections().is_empty());
            }
        }

        #[test]
        fn reject_reason_is_capped() {
            let long = "r".repeat(RejectionReason::MAX_LEN + 1);
            let error = in_status(ExpenseStatus::Submitted)
                .reject(*APPROVER, &long)
                .unwrap_err();
            assert_eq!(
                error,
                ExpenseError::InvalidInput(invalid("reason", "Rejection reason is longer than 1000"))
            );
        }

        #[test]
        fn reject_checks_role_and_status_before_the_reason() {
            // A MEMBER sending a blank reason gets "not allowed" (403), not "reason required" (400), as in Java.
            let error = in_status(ExpenseStatus::Submitted)
                .reject(*OTHER_MEMBER, " ")
                .unwrap_err();
            assert!(matches!(error, ExpenseError::Forbidden(_)), "{error:?}");
            let error = in_status(ExpenseStatus::Draft).reject(*APPROVER, " ").unwrap_err();
            assert!(matches!(error, ExpenseError::InvalidTransition { .. }), "{error:?}");
        }

        #[test]
        fn the_rejection_keeps_the_stripped_reason_and_who_rejected() {
            let mut e = in_status(ExpenseStatus::Submitted);
            e.reject(*APPROVER, "  no receipt for tip \n").unwrap();
            let [rejection] = e.rejections() else {
                panic!("expected exactly one rejection")
            };
            assert_eq!(rejection.reason().as_str(), "no receipt for tip");
            assert_eq!(rejection.rejected_by(), APPROVER.id());
        }

        #[test]
        fn every_rejection_is_kept() {
            let mut e = in_status(ExpenseStatus::Submitted);
            e.reject(*APPROVER, "first").unwrap();
            e.reopen(*CLAIMANT).unwrap();
            e.submit(*CLAIMANT, READY).unwrap();
            e.reject(*OWNER, "second").unwrap();
            let reasons: Vec<&str> = e.rejections().iter().map(|r| r.reason().as_str()).collect();
            assert_eq!(reasons, ["first", "second"]);
            assert_eq!(e.rejections().last().unwrap().rejected_by(), OWNER.id());
        }

        // Java's `rejectionsListIsReadOnly` has no counterpart: `rejections()` returns `&[Rejection]`, and a
        // shared slice has no method that could add or remove an element.

        #[test]
        fn the_full_happy_path() {
            let mut e = draft();
            e.submit(*CLAIMANT, READY).unwrap();
            e.withdraw(*CLAIMANT).unwrap();
            assert_eq!(e.status(), ExpenseStatus::Draft);
            e.submit(*CLAIMANT, READY).unwrap();
            e.approve(*APPROVER).unwrap();
            e.reimburse(*OWNER).unwrap();
            assert_eq!(e.status(), ExpenseStatus::Reimbursed);
            assert!(e.rejections().is_empty());
        }
    }

    mod draft_only_operations {
        use super::*;

        const NON_DRAFT: [ExpenseStatus; 4] = [
            ExpenseStatus::Submitted,
            ExpenseStatus::Approved,
            ExpenseStatus::Rejected,
            ExpenseStatus::Reimbursed,
        ];

        fn yesterday() -> NaiveDate {
            today().checked_sub_days(Days::new(1)).unwrap()
        }

        fn train_ticket() -> NewExpense {
            fields("20", Category::Travel, today(), "Bahn", None)
        }

        #[test]
        fn claimant_may_edit_draft() {
            let mut e = draft();
            let id = e.id();
            e.edit(
                *CLAIMANT,
                &fields("20", Category::Travel, yesterday(), "Bahn", Some("Ticket")),
                &clock(),
            )
            .unwrap();
            assert_eq!(e.money(), money("20.00"));
            assert_eq!(e.category(), Category::Travel);
            assert_eq!(e.incurred_on(), yesterday());
            assert_eq!(e.merchant().as_str(), "Bahn");
            assert_eq!(e.note().map(Note::as_str), Some("Ticket"));
            assert_eq!(e.id(), id);
            assert_eq!(e.status(), ExpenseStatus::Draft);
        }

        #[test]
        fn edit_can_remove_the_note() {
            let mut e = draft();
            e.edit(*CLAIMANT, &train_ticket(), &clock()).unwrap();
            assert_eq!(e.note(), None);
        }

        #[test]
        fn edit_validates_like_creation() {
            let tomorrow = today().checked_add_days(Days::new(1)).unwrap();
            let mut e = draft();
            let error = e
                .edit(
                    *CLAIMANT,
                    &fields("20", Category::Travel, tomorrow, "Bahn", None),
                    &clock(),
                )
                .unwrap_err();
            assert_eq!(
                error,
                ExpenseError::InvalidInput(invalid("incurredOn", "incurredOn must not be in the future"))
            );
            let error = e
                .edit(
                    *CLAIMANT,
                    &fields("20", Category::Travel, today(), "  ", None),
                    &clock(),
                )
                .unwrap_err();
            assert_eq!(
                error,
                ExpenseError::InvalidInput(invalid("merchant", "Merchant is required"))
            );
        }

        #[test]
        fn a_refused_edit_changes_nothing() {
            let tomorrow = today().checked_add_days(Days::new(1)).unwrap();
            let mut e = draft();
            e.edit(
                *CLAIMANT,
                &fields("20", Category::Travel, tomorrow, "Bahn", None),
                &clock(),
            )
            .unwrap_err();
            assert_eq!(e.money(), money("12.50"));
            assert_eq!(e.category(), Category::Meals);
            assert_eq!(e.merchant().as_str(), "Cafe Central");
            assert_eq!(e.note().map(Note::as_str), Some("Lunch"));
        }

        #[test]
        fn edit_by_non_claimant_is_refused() {
            let error = draft().edit(*OWNER, &train_ticket(), &clock()).unwrap_err();
            assert_eq!(error, ExpenseError::Forbidden(Forbidden::EditByNonClaimant));
            assert_eq!(error.to_string(), "Only the claimant may edit an expense");
        }

        #[test]
        fn edit_outside_draft_is_refused() {
            for status in NON_DRAFT {
                let error = in_status(status)
                    .edit(*CLAIMANT, &train_ticket(), &clock())
                    .unwrap_err();
                assert_eq!(error, ExpenseError::NotDraft { status });
                assert_eq!(
                    error.to_string(),
                    format!("Only a draft allows this, but the expense is {status}")
                );
            }
        }

        #[test]
        fn edit_checks_claimant_then_status_then_the_fields() {
            let blank = fields("20", Category::Travel, today(), " ", None);
            let error = in_status(ExpenseStatus::Submitted)
                .edit(*OWNER, &blank, &clock())
                .unwrap_err();
            assert!(matches!(error, ExpenseError::Forbidden(_)), "{error:?}");
            let error = in_status(ExpenseStatus::Submitted)
                .edit(*CLAIMANT, &blank, &clock())
                .unwrap_err();
            assert!(matches!(error, ExpenseError::NotDraft { .. }), "{error:?}");
        }

        #[test]
        fn receipts_and_delete_only_for_claimant_of_draft() {
            let d = draft();
            assert!(d.can_change_receipts(*CLAIMANT));
            assert!(d.can_delete(*CLAIMANT));
            assert!(!d.can_change_receipts(*OWNER));
            assert!(!d.can_delete(*OWNER));
            for status in NON_DRAFT {
                let e = in_status(status);
                assert!(!e.can_change_receipts(*CLAIMANT), "receipts in {status}");
                assert!(!e.can_delete(*CLAIMANT), "delete in {status}");
            }
        }

        #[test]
        fn delete_is_for_the_claimant_of_a_draft() {
            assert_eq!(draft().require_deletable_by(*CLAIMANT), Ok(()));
            let error = draft().require_deletable_by(*OWNER).unwrap_err();
            assert_eq!(error, ExpenseError::Forbidden(Forbidden::DeleteByNonClaimant));
            assert_eq!(error.to_string(), "Only the claimant may delete an expense");
            for status in NON_DRAFT {
                assert_eq!(
                    in_status(status).require_deletable_by(*CLAIMANT),
                    Err(ExpenseError::NotDraft { status })
                );
                // Someone else is told "not you" before "not a draft".
                assert_eq!(
                    in_status(status).require_deletable_by(*OWNER),
                    Err(ExpenseError::Forbidden(Forbidden::DeleteByNonClaimant))
                );
            }
        }
    }

    mod receipt_permissions {
        use super::*;

        #[test]
        fn claimant_may_change_receipts_of_a_draft() {
            assert_eq!(draft().require_can_change_receipts(*CLAIMANT), Ok(()));
        }

        #[test]
        fn someone_else_may_not_even_with_a_higher_role() {
            for actor in [*OWNER, *OTHER_MEMBER] {
                let error = draft().require_can_change_receipts(actor).unwrap_err();
                assert_eq!(error, ExpenseError::Forbidden(Forbidden::ChangeReceiptsByNonClaimant));
                assert_eq!(error.to_string(), "Only the claimant may change receipts");
            }
        }

        #[test]
        fn not_after_the_draft_stage() {
            for status in [
                ExpenseStatus::Submitted,
                ExpenseStatus::Approved,
                ExpenseStatus::Rejected,
                ExpenseStatus::Reimbursed,
            ] {
                assert_eq!(
                    in_status(status).require_can_change_receipts(*CLAIMANT),
                    Err(ExpenseError::NotDraft { status })
                );
            }
        }
    }

    mod visibility {
        use super::*;

        #[test]
        fn claimant_sees_own_expense() {
            assert!(draft().is_visible_to(*CLAIMANT));
        }

        #[test]
        fn other_member_does_not() {
            assert!(!draft().is_visible_to(*OTHER_MEMBER));
        }

        #[test]
        fn approver_and_owner_see_everything() {
            assert!(draft().is_visible_to(*APPROVER));
            assert!(draft().is_visible_to(*OWNER));
        }

        #[test]
        fn claimant_who_is_no_longer_on_the_team_still_sees_own() {
            // The domain only knows the actor's role; a removed claimant is handled by the caller.
            assert!(draft().is_visible_to(Actor::new(*CLAIMANT_ID, Role::Member)));
        }
    }
}
