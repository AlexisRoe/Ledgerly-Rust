//! Lesson 119: repetition, recursion, internal rules and hygiene. Run `cargo test ex119`.

/// 1. Recursion: `max_of!(a)` is `a`; `max_of!(a, b, …)` is `std::cmp::max(a, max_of!(b, …))`.
///    Two rules, the single-value one first (rules are tried top to bottom).
macro_rules! max_of {
    ($only:expr) => { $only };
    ($first:expr, $($rest:expr),+) => {
        std::cmp::max($first, max_of!($($rest),+))
    };
}

/// 2. Internal rules, like Ledgerly's `typed_id!`: public arms forward to an `@common` arm that holds the
///    shared code. Both public arms already call it; write the `@common` arm so it defines
///    `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct $name(u32);` with a `pub const fn get(self) -> u32`.
macro_rules! number_id {
    ($name:ident, even) => {
        number_id!(@common $name);
        impl $name {
            /// `None` unless the number is even.
            pub fn new(value: u32) -> Option<Self> {
                (value % 2 == 0).then_some(Self(value))
            }
        }
    };
    ($name:ident, any) => {
        number_id!(@common $name);
        impl $name {
            pub fn new(value: u32) -> Option<Self> {
                Some(Self(value))
            }
        }
    };
    (@common $name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(u32);
        impl $name {
            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

/// The highest fee: `max_of!` in use outside the tests.
pub fn highest_fee() -> i32 {
    max_of!(5, 12, 7)
}

number_id!(PairId, even);
number_id!(AnyId, any);

/// 3. Hygiene: a variable the macro defines is invisible to the caller's code, and vice versa. `double_it!(x)`
///    defines its own `result`, which can't clash with the caller's `result`.
macro_rules! double_it {
    ($value:expr) => {{
        let result = $value * 2;
        result
    }};
}

pub fn hygiene_demo() -> (i32, i32) {
    let result = 10;
    let doubled = double_it!(result + 1);
    (result, doubled)
}

/// 4. Generating tests, like Ledgerly's `required_in_prod!`: each `name: input => expected` line becomes a
///    `#[test] fn name()` asserting `max_of!` over the input. Write the expansion: `$( #[test] fn $name() { … } )*`.
#[cfg(test)]
macro_rules! max_cases {
    ($($name:ident: [$($input:expr),+] => $expected:expr,)*) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(max_of!($($input),+), $expected);
            }
        )*
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_max() {
        assert_eq!(max_of!(3), 3);
        assert_eq!(max_of!(3, 9, 4), 9);
    }

    #[test]
    fn internal_rules() {
        assert_eq!(PairId::new(4).map(PairId::get), Some(4));
        assert_eq!(PairId::new(3), None);
        assert_eq!(AnyId::new(3).map(AnyId::get), Some(3));
    }

    #[test]
    fn macros_are_hygienic() {
        assert_eq!(hygiene_demo(), (10, 22));
    }

    max_cases! {
        max_of_two: [1, 2] => 2,
        max_of_negatives: [-5, -1, -9] => -1,
    }

}
