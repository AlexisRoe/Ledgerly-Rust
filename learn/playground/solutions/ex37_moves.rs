//! Lesson 37: moves. Run `cargo test ex37`.

pub struct Rejection {
    pub reason: String,
}

pub fn reject(reason: String) -> (usize, Rejection) {
    let length = reason.len(); // read before the move…
    let rejection = Rejection { reason };
    // …or: let length = rejection.reason.len();
    (length, rejection)
}

pub fn total_length(names: Vec<String>) -> (usize, Vec<String>) {
    let mut total = 0;
    for name in &names {
        total += name.len();
    }
    (total, names)
}

fn shout(text: &str) -> String {
    text.to_uppercase()
}

pub fn both(text: String) -> (String, String) {
    let loud = shout(&text);
    (loud, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_keeps_both() {
        let (len, r) = reject("No receipt".to_owned());
        assert_eq!(len, 10);
        assert_eq!(r.reason, "No receipt");
    }

    #[test]
    fn total_keeps_names() {
        let (total, names) = total_length(vec!["ab".to_owned(), "cde".to_owned()]);
        assert_eq!(total, 5);
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn both_versions() {
        assert_eq!(both("bahn".to_owned()), ("BAHN".to_owned(), "bahn".to_owned()));
    }
}
