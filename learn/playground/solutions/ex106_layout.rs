//! Lesson 106: stack, heap and layout. Run `cargo test ex106`.
//!
//! Predict each size in bytes (on a 64-bit machine), write it in place of the 0, then run the tests. Don't
//! guess blindly: reason from the lesson (pointer = 8, length = 8, capacity = 8, padding, niches).

#![allow(dead_code)]

use std::num::NonZeroU32;

pub enum Status {
    Draft,
    Submitted,
    Approved,
}

pub struct Unordered {
    flag: u8,
    amount: u64,
    other: u8,
}

/// `&str`: a pointer and a length.
pub const STR_REF: usize = 16;
/// `String`: pointer, capacity, length (the text itself is on the heap).
pub const STRING: usize = 24;
/// `Vec<u64>`: same shape as String.
pub const VEC: usize = 24;
/// `Box<u64>`: one pointer.
pub const BOX: usize = 8;
/// `Option<Box<u64>>`: a Box is never null, so `None` can BE the null pointer (a "niche").
pub const OPTION_BOX: usize = 8;
/// `Option<u32>`: every u32 bit pattern is a valid number, so a tag is needed (and padding to align it).
pub const OPTION_U32: usize = 8;
/// `Option<NonZeroU32>`: zero is free to mean `None`.
pub const OPTION_NONZERO: usize = 4;
/// `Status`: three variants without data.
pub const STATUS: usize = 1;
/// `Option<Status>`: the enum's byte has unused values left for `None`.
pub const OPTION_STATUS: usize = 1;
/// `Unordered`: u8, u64, u8. Rust may reorder fields to reduce padding (unlike C).
pub const UNORDERED: usize = 16;
/// `Box<dyn Fn()>`: a pointer to the data plus a pointer to the vtable.
pub const BOX_DYN: usize = 16;
/// `[u16; 5]`: arrays are inline, no header.
pub const ARRAY: usize = 10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointers_and_headers() {
        assert_eq!(STR_REF, size_of::<&str>(), "&str");
        assert_eq!(STRING, size_of::<String>(), "String");
        assert_eq!(VEC, size_of::<Vec<u64>>(), "Vec<u64>");
        assert_eq!(BOX, size_of::<Box<u64>>(), "Box<u64>");
        assert_eq!(BOX_DYN, size_of::<Box<dyn Fn()>>(), "Box<dyn Fn()>");
        assert_eq!(ARRAY, size_of::<[u16; 5]>(), "[u16; 5]");
    }

    #[test]
    fn niches_and_tags() {
        assert_eq!(OPTION_BOX, size_of::<Option<Box<u64>>>(), "Option<Box<u64>>");
        assert_eq!(OPTION_U32, size_of::<Option<u32>>(), "Option<u32>");
        assert_eq!(OPTION_NONZERO, size_of::<Option<NonZeroU32>>(), "Option<NonZeroU32>");
        assert_eq!(STATUS, size_of::<Status>(), "Status");
        assert_eq!(OPTION_STATUS, size_of::<Option<Status>>(), "Option<Status>");
    }

    #[test]
    fn padding() {
        assert_eq!(UNORDERED, size_of::<Unordered>(), "Unordered");
    }
}
