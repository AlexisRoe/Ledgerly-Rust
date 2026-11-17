//! Lesson 37: moves. Run `cargo test ex37`.
//!
//! Each function compiles as shipped. For each one:
//!   1. uncomment the line marked UNCOMMENT, run `cargo test ex37`, read the E0382 error;
//!   2. change the code so it compiles WITHOUT `.clone()` and the test passes.

pub struct Rejection {
    pub reason: String,
}

/// Should return the reason's length AND the rejection. Hint: compute the length before the move,
/// or read it from the struct afterwards.
pub fn reject(reason: String) -> (usize, Rejection) {
    let rejection = Rejection { reason };
    let length = 0; // UNCOMMENT the next line and delete this one
    // let length = reason.len();
    (length, rejection)
}

/// Should return the total length of all names, and still return the names.
/// Hint: loop over `&names` instead of `names`.
pub fn total_length(names: Vec<String>) -> (usize, Vec<String>) {
    let mut total = 0;
    for name in names.iter() { // UNCOMMENT: replace `names.iter()` with plain `names` first, to see the error
        total += name.len();
    }
    (total, names)
}

fn shout(text: String) -> String {
    text.to_uppercase()
}

/// Should return "BAHN" and "bahn". `shout` takes ownership, so pass it something it may keep.
/// Hint: change `shout` to take `&str` instead.
pub fn both(text: String) -> (String, String) {
    let loud = String::new(); // UNCOMMENT the next line and delete this one
    // let loud = shout(text);
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
