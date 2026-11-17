//! Lesson 49: traits. Run `cargo test ex49`.

/// Something that can deliver a message. `notify` is required; `notify_all` has a default body.
pub trait Notifier {
    /// Returns a log line describing what was sent.
    fn notify(&self, to: &str, message: &str) -> String;

    /// Default method: notify every recipient, collecting the log lines. Implement the body here, once,
    /// using `self.notify(...)`.
    fn notify_all(&self, recipients: &[&str], message: &str) -> Vec<String> {
        todo!()
    }
}

pub struct Email;
pub struct Sms {
    pub sender: String,
}

// 1. Implement Notifier for Email:  notify → "email to <to>: <message>"
// 2. Implement Notifier for Sms:    notify → "sms from <sender> to <to>: <message>"
//    (Write `impl Notifier for Email { … }` blocks below; then uncomment the tests.)

/// 3. Works with any Notifier (lesson 53 explains `&dyn`).
pub fn remind(notifier: &dyn Notifier, to: &str) -> String {
    notifier.notify(to, "Your expense is still a draft")
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn email_and_sms() {
    //     assert_eq!(Email.notify("sam", "hi"), "email to sam: hi");
    //     let sms = Sms { sender: "Ledgerly".to_owned() };
    //     assert_eq!(sms.notify("sam", "hi"), "sms from Ledgerly to sam: hi");
    // }
    //
    // #[test]
    // fn default_method_and_dyn() {
    //     assert_eq!(Email.notify_all(&["a", "b"], "x"), vec!["email to a: x", "email to b: x"]);
    //     assert_eq!(remind(&Email, "sam"), "email to sam: Your expense is still a draft");
    // }
}
