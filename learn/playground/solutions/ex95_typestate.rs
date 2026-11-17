//! Lesson 95: typestate, states as types. Run `cargo test ex95`.

use std::marker::PhantomData;

/// The states. Empty structs: they exist only at compile time.
pub struct Draft;
pub struct Submitted;
pub struct Approved;

/// A claim in state `S`. `PhantomData<S>` says "this type is about an S" without storing one (size 0).
pub struct Claim<S> {
    amount: u64,
    approver: Option<String>,
    state: PhantomData<S>,
}

/// Methods every state has.
impl<S> Claim<S> {
    pub fn amount(&self) -> u64 {
        self.amount
    }
}

impl Claim<Draft> {
    pub fn new(amount: u64) -> Self {
        Self { amount, approver: None, state: PhantomData }
    }

    /// Drafts can still change.
    pub fn set_amount(&mut self, amount: u64) {
        self.amount = amount;
    }

    /// 1. Consume the draft (`self` by value) and return the same data as a `Claim<Submitted>`.
    pub fn submit(self) -> Claim<Submitted> {
        Claim { amount: self.amount, approver: None, state: PhantomData }
    }
}

impl Claim<Submitted> {
    /// 2. Record who approved it, as a `Claim<Approved>`.
    pub fn approve(self, by: &str) -> Claim<Approved> {
        Claim { amount: self.amount, approver: Some(by.to_owned()), state: PhantomData }
    }

    /// 3. Send it back: a `Claim<Draft>` again (the approver stays `None`).
    pub fn reject(self) -> Claim<Draft> {
        Claim { amount: self.amount, approver: None, state: PhantomData }
    }
}

impl Claim<Approved> {
    /// Only approved claims have an approver. (An approved claim always has one: `approve` sets it.)
    pub fn approved_by(&self) -> &str {
        self.approver.as_deref().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_the_happy_path() {
        let mut draft = Claim::new(100);
        draft.set_amount(120);
        let approved = draft.submit().approve("sam");
        assert_eq!(approved.amount(), 120);
        assert_eq!(approved.approved_by(), "sam");
    }

    #[test]
    fn rejected_claims_are_drafts_again() {
        let mut again = Claim::new(50).submit().reject();
        again.set_amount(40);
        assert_eq!(again.submit().amount(), 40);
    }

    #[test]
    fn states_cost_nothing() {
        assert_eq!(size_of::<Claim<Draft>>(), size_of::<Claim<Approved>>());
        assert_eq!(size_of::<Draft>(), 0);
    }

    // 4. Uncomment, read the error (E0599: no method `approve` for `Claim<Draft>`), and comment it out again.
    // #[test]
    // fn cannot_approve_a_draft() {
    //     let _ = Claim::new(10).approve("sam");
    // }
}
