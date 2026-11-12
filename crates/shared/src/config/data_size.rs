//! Sizes in bytes as Spring Boot reads them from configuration (`DataSize`).

/// The text is not a data size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("expected a size such as 1048576, 512KB or 10MB")]
pub struct InvalidDataSize;

/// Parses a size into bytes: a whole number and a unit `B`, `KB`, `MB`, `GB` or `TB` (any case); without a
/// unit the number counts bytes. The units are binary, as in Spring: `10MB` is 10 × 1024 × 1024 bytes.
///
/// # Errors
///
/// Returns [`InvalidDataSize`] for anything else, including negative sizes and sizes beyond `u64`.
pub fn parse_data_size(text: &str) -> Result<u64, InvalidDataSize> {
    let text = text.trim();
    let digits_end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    let (digits, unit) = text.split_at(digits_end);
    let number: u64 = digits.parse().map_err(|_| InvalidDataSize)?;
    let shift = match unit.to_ascii_uppercase().as_str() {
        "" | "B" => 0_u32,
        "KB" => 10,
        "MB" => 20,
        "GB" => 30,
        "TB" => 40,
        _ => return Err(InvalidDataSize),
    };
    number.checked_mul(1_u64 << shift).ok_or(InvalidDataSize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_binary() {
        assert_eq!(parse_data_size("10MB"), Ok(10_485_760));
        assert_eq!(parse_data_size("1MB"), Ok(1_048_576));
        assert_eq!(parse_data_size("16mb"), Ok(16_777_216));
        assert_eq!(parse_data_size("512KB"), Ok(524_288));
        assert_eq!(parse_data_size("1GB"), Ok(1_073_741_824));
        assert_eq!(parse_data_size("1TB"), Ok(1_099_511_627_776));
        assert_eq!(parse_data_size("5B"), Ok(5));
    }

    #[test]
    fn a_bare_number_counts_bytes() {
        assert_eq!(parse_data_size("1048576"), Ok(1_048_576));
    }

    #[test]
    fn everything_else_is_refused() {
        for text in [
            "",
            "MB",
            "10 MB",
            "1.5MB",
            "-1MB",
            "10M",
            "10PB",
            "99999999999999999999",
            "18446744073709551615KB",
        ] {
            assert_eq!(parse_data_size(text), Err(InvalidDataSize), "{text:?}");
        }
    }
}
