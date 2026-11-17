//! Lesson 107: where allocations hide. Run `cargo test ex107`.

use std::fmt::Write as _;

/// 1. Bytes as lowercase hex ("0a ff" → "0aff"). Allocate ONCE: `String::with_capacity(bytes.len() * 2)`,
///    then `write!(text, "{byte:02x}")` into it (no `format!` per byte, which would allocate each time).
pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// 2. Write `fields` as one CSV line into a buffer the CALLER owns: `buf.clear()` (keeps the capacity), then
///    push the fields separated by ",". Called in a loop, this reuses one allocation for every line.
pub fn write_csv_line(buf: &mut String, fields: &[&str]) {
    buf.clear();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            buf.push(',');
        }
        buf.push_str(field);
    }
}

/// 3. The longest word, BORROWED from the input: no clone. (`max_by_key(|w| w.len())`, then `.as_str()`.)
pub fn longest(words: &[String]) -> Option<&str> {
    words.iter().max_by_key(|word| word.len()).map(String::as_str)
}

/// 4. How many words are longer than `min`? Count straight from the iterator: no `collect::<Vec<_>>()` first.
pub fn count_longer(text: &str, min: usize) -> usize {
    text.split_whitespace().filter(|word| word.len() > min).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_allocates_exactly_once() {
        let text = hex(&[0x0a, 0xff, 0x00]);
        assert_eq!(text, "0aff00");
        assert_eq!(text.capacity(), 6, "with_capacity, then no growth");
    }

    #[test]
    fn reuses_the_buffer() {
        let mut buf = String::new();
        write_csv_line(&mut buf, &["2026-11-17", "Taxi", "12.50", "EUR"]);
        assert_eq!(buf, "2026-11-17,Taxi,12.50,EUR");
        let address = buf.as_ptr();
        write_csv_line(&mut buf, &["x", "y"]);
        assert_eq!(buf, "x,y");
        assert_eq!(buf.as_ptr(), address, "same allocation, reused");
    }

    #[test]
    fn borrows_instead_of_cloning() {
        let words = vec!["taxi".to_owned(), "restaurant".to_owned()];
        let found = longest(&words);
        assert_eq!(found, Some("restaurant"));
        assert_eq!(found.map(str::as_ptr), Some(words[1].as_ptr()));
        assert_eq!(longest(&[]), None);
    }

    #[test]
    fn counts_without_collecting() {
        assert_eq!(count_longer("a hotel taxi restaurant", 4), 2);
    }
}
