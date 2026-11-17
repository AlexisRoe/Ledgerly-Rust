//! Lesson 12: paths and `use`. Run `cargo test ex12`.
//!
//! Two inline modules stand in for two files. Complete the `todo!()`s using the hints.

pub mod money {
    /// Formats cents as euros: 1250 → "12.50 EUR". Already done; you'll call it from `report`.
    pub fn format_cents(cents: u32) -> String {
        format!("{}.{:02} EUR", cents / 100, cents % 100)
    }

    pub mod vat {
        pub const RATE_PERCENT: u32 = 19;

        /// Should return "<gross> incl. VAT", e.g. 1000 → "11.90 EUR incl. VAT".
        /// Call `format_cents` from the parent module through `super::`.
        pub fn gross_label(net_cents: u32) -> String {
            let gross = net_cents * (100 + RATE_PERCENT) / 100;
            todo!()
        }
    }
}

pub mod report {
    // Add a `use` line here that imports `format_cents` under the name `euros`
    // (path starts with `crate::ex12_paths::money`).

    /// Should return "Total: <amount>", e.g. 250 → "Total: 2.50 EUR". Use your `euros` shortcut.
    pub fn total_line(cents: u32) -> String {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::money::vat::gross_label;
    use super::report::total_line;

    #[test]
    fn gross_label_uses_the_parent_formatter() {
        assert_eq!(gross_label(1000), "11.90 EUR incl. VAT");
    }

    #[test]
    fn total_line_uses_the_renamed_import() {
        assert_eq!(total_line(250), "Total: 2.50 EUR");
    }
}
