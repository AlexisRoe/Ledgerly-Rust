//! Lesson 31: match. Run `cargo test ex31`.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Light {
    Red,
    Yellow,
    Green,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Signal {
    Stop,
    Go,
}

/// A tiny workflow table in the style of Ledgerly's `Action::target_from`:
///   Red    + Go   → Green
///   Green  + Stop → Yellow
///   Yellow + Stop → Red
///   everything else → None
/// Write it as a match on `from` with an inner match on `signal`, and NO `_` arms.
pub fn next(from: Light, signal: Signal) -> Option<Light> {
    match from {
        Light::Red => match signal {
            Signal::Go => Some(Light::Green),
            Signal::Stop => None,
        },
        Light::Green => match signal {
            Signal::Stop => Some(Light::Yellow),
            Signal::Go => None,
        },
        Light::Yellow => match signal {
            Signal::Stop => Some(Light::Red),
            Signal::Go => None,
        },
    }
}

/// Classify an HTTP status with ranges: 200..=299 "success", 300..=399 "redirect", 404 "not found",
/// other 400..=499 "client error", 500..=599 "server error", anything else "unknown".
/// (Mind the order of 404 and the 4xx range.)
pub fn classify(status: u16) -> &'static str {
    match status {
        200..=299 => "success",
        300..=399 => "redirect",
        404 => "not found",
        400..=499 => "client error",
        500..=599 => "server error",
        _ => "unknown",
    }
}

/// Shipping cost in cents: None (no weight given) → 0; Some(w) with w > 20 kg → 2000; Some(_) → 500.
/// Use a guard for the heavy case.
pub fn shipping(weight_kg: Option<u32>) -> u32 {
    match weight_kg {
        None => 0,
        Some(w) if w > 20 => 2000,
        Some(_) => 500,
    }
}

// Bonus: add a variant `Blinking` to `Light`. Run `cargo build` and read the errors: the compiler lists every
// match that must now decide what to do with it. Then remove it again (or handle it).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow() {
        assert_eq!(next(Light::Red, Signal::Go), Some(Light::Green));
        assert_eq!(next(Light::Green, Signal::Stop), Some(Light::Yellow));
        assert_eq!(next(Light::Yellow, Signal::Stop), Some(Light::Red));
        assert_eq!(next(Light::Red, Signal::Stop), None);
        assert_eq!(next(Light::Yellow, Signal::Go), None);
        assert_eq!(next(Light::Green, Signal::Go), None);
    }

    #[test]
    fn statuses() {
        assert_eq!(classify(201), "success");
        assert_eq!(classify(302), "redirect");
        assert_eq!(classify(404), "not found");
        assert_eq!(classify(422), "client error");
        assert_eq!(classify(503), "server error");
        assert_eq!(classify(42), "unknown");
    }

    #[test]
    fn shipping_costs() {
        assert_eq!(shipping(None), 0);
        assert_eq!(shipping(Some(25)), 2000);
        assert_eq!(shipping(Some(3)), 500);
    }
}
