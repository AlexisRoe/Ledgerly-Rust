//! Lesson 49: traits. Run `cargo test ex49`.

pub trait Notifier {
    fn notify(&self, to: &str, message: &str) -> String;

    fn notify_all(&self, recipients: &[&str], message: &str) -> Vec<String> {
        recipients.iter().map(|to| self.notify(to, message)).collect()
    }
}

pub struct Email;
pub struct Sms {
    pub sender: String,
}

impl Notifier for Email {
    fn notify(&self, to: &str, message: &str) -> String {
        format!("email to {to}: {message}")
    }
}

impl Notifier for Sms {
    fn notify(&self, to: &str, message: &str) -> String {
        format!("sms from {} to {to}: {message}", self.sender)
    }
}

pub fn remind(notifier: &dyn Notifier, to: &str) -> String {
    notifier.notify(to, "Your expense is still a draft")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_and_sms() {
        assert_eq!(Email.notify("sam", "hi"), "email to sam: hi");
        let sms = Sms { sender: "Ledgerly".to_owned() };
        assert_eq!(sms.notify("sam", "hi"), "sms from Ledgerly to sam: hi");
    }

    #[test]
    fn default_method_and_dyn() {
        assert_eq!(Email.notify_all(&["a", "b"], "x"), vec!["email to a: x", "email to b: x"]);
        assert_eq!(remind(&Email, "sam"), "email to sam: Your expense is still a draft");
    }
}
