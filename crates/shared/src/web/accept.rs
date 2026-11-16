//! Whether an endpoint can answer in a media type the client accepts.
//!
//! Java counterpart: Spring MVC's `ProducesRequestCondition` with `HeaderContentNegotiationStrategy` and
//! `MimeType.isCompatibleWith`. The generated `*Api` interfaces declare `produces` as every media type of the
//! operation's responses in the spec, so an operation whose success has no body produces only
//! `application/problem+json`.

use axum::http::{HeaderMap, header};

/// What most operations produce: their JSON body, or a problem.
pub const JSON: &[&str] = &["application/json", "application/problem+json"];
/// What an operation without a response body produces: only its problems.
pub const PROBLEM: &[&str] = &["application/problem+json"];
/// What the CSV export produces.
pub const CSV: &[&str] = &["text/csv", "application/problem+json"];

/// True if one of `produces` is compatible with one of the `Accept` header's media types. No `Accept`, or one
/// with no media type in it, accepts everything; one that cannot be parsed accepts nothing, as Spring then
/// throws `HttpMediaTypeNotAcceptableException` too. The quality is not looked at: `q=0` still matches, as in
/// Spring's condition.
#[must_use]
pub fn accepts(headers: &HeaderMap, produces: &[&str]) -> bool {
    let mut accepted = Vec::new();
    for value in headers.get_all(header::ACCEPT) {
        let Ok(text) = std::str::from_utf8(value.as_bytes()) else {
            return false;
        };
        for token in tokenize(text) {
            match MediaType::parse(token) {
                Some(media_type) => accepted.push(media_type),
                None => return false,
            }
        }
    }
    if accepted.is_empty() {
        return true;
    }
    produces
        .iter()
        .filter_map(|produced| MediaType::parse(produced))
        .any(|produced| accepted.iter().any(|wanted| produced.is_compatible_with(wanted)))
}

/// `MimeTypeUtils.tokenize`: the comma-separated entries, commas inside quotes not counting, blank ones dropped.
fn tokenize(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    for (index, c) in text.char_indices() {
        match c {
            '"' if !escaped => quoted = !quoted,
            ',' if !quoted => {
                tokens.push(text.get(start..index).unwrap_or_default());
                start = index + 1;
            }
            _ => {}
        }
        escaped = c == '\\' && !escaped;
    }
    tokens.push(text.get(start..).unwrap_or_default());
    tokens
        .into_iter()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect()
}

/// A type and subtype, in lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaType {
    kind: String,
    subtype: String,
}

impl MediaType {
    /// `MimeTypeUtils.parseMimeType` plus `MediaType`'s check of `q`: `None` where Spring throws
    /// `InvalidMediaTypeException`.
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(';');
        let full = parts.next().unwrap_or_default().trim();
        let full = if full == "*" { "*/*" } else { full };
        let (kind, subtype) = full.split_once('/')?;
        if !is_token(kind) || !is_token(subtype) || (kind == "*" && subtype != "*") {
            return None;
        }
        for parameter in parts {
            let parameter = parameter.trim();
            if parameter.is_empty() {
                continue;
            }
            let (name, value) = parameter.split_once('=')?;
            let (name, value) = (name.trim(), value.trim());
            let quoted = value.len() >= 2 && value.starts_with('"') && value.ends_with('"');
            if !is_token(name) || !(quoted || is_token(value)) {
                return None;
            }
            if name.eq_ignore_ascii_case("q") {
                let quality: f64 = value.trim_matches('"').parse().ok()?;
                if !(0.0..=1.0).contains(&quality) {
                    return None;
                }
            }
        }
        Some(Self {
            kind: kind.to_ascii_lowercase(),
            subtype: subtype.to_ascii_lowercase(),
        })
    }

    fn is_wildcard_subtype(&self) -> bool {
        self.subtype == "*" || self.subtype.starts_with("*+")
    }

    /// `json` of `problem+json`.
    fn suffix(&self) -> Option<&str> {
        self.subtype.rsplit_once('+').map(|(_, suffix)| suffix)
    }

    /// `MimeType.isCompatibleWith`.
    fn is_compatible_with(&self, other: &Self) -> bool {
        if self.kind == "*" || other.kind == "*" {
            return true;
        }
        if self.kind != other.kind {
            return false;
        }
        if self.subtype == other.subtype {
            return true;
        }
        if self.is_wildcard_subtype() || other.is_wildcard_subtype() {
            if self.subtype == "*" || other.subtype == "*" {
                return true;
            }
            if self.is_wildcard_subtype()
                && let Some(suffix) = self.suffix()
            {
                return suffix == other.subtype || Some(suffix) == other.suffix();
            }
            if other.is_wildcard_subtype()
                && let Some(suffix) = other.suffix()
            {
                return self.subtype == suffix || Some(suffix) == self.suffix();
            }
        }
        false
    }
}

/// An RFC 2616 token: visible ASCII without separators.
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b"()<>@,;:\\\"/[]?={}".contains(&b))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn accept(value: &'static str, produces: &[&str]) -> bool {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static(value));
        accepts(&headers, produces)
    }

    #[test]
    fn no_accept_header_accepts_everything() {
        assert!(accepts(&HeaderMap::new(), PROBLEM));
        assert!(accept("", PROBLEM));
        assert!(accept(" , ", CSV));
    }

    #[test]
    fn wildcards_and_exact_types_match() {
        assert!(accept("*/*", JSON));
        assert!(accept("*", JSON));
        assert!(accept("application/*", CSV));
        assert!(accept("text/html, application/json;q=0.9", JSON));
        assert!(accept("APPLICATION/JSON", JSON));
        assert!(accept("text/csv", CSV));
        // Spring's condition ignores the quality.
        assert!(accept("application/json;q=0", JSON));
    }

    #[test]
    fn a_structured_suffix_matches_its_wildcard_only() {
        assert!(accept("application/*+json", PROBLEM));
        assert!(accept("application/*+json", JSON));
        assert!(!accept("application/json", PROBLEM));
        assert!(accept("application/problem+json", PROBLEM));
    }

    #[test]
    fn types_nobody_produces_do_not_match() {
        assert!(!accept("text/html", JSON));
        assert!(!accept("application/json", CSV));
        assert!(!accept("text/csv", JSON));
        assert!(!accept("application/xml, text/plain", JSON));
    }

    #[test]
    fn an_unparsable_accept_matches_nothing() {
        for bad in [
            "json",
            "application/",
            "*/json",
            "application/json;q=2",
            "application/json;q",
            "text/html, x",
        ] {
            assert!(!accept(bad, JSON), "{bad}");
        }
        assert!(accept("application/json;charset=\"utf-8, really\"", JSON));
    }
}
