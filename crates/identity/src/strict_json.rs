//! JSON as Gson reads it for Nimbus: a member name twice in one object makes the whole text unreadable.
//!
//! `serde_json` keeps the last of two equal names. For a token that would let a header or a claim say one
//! thing to one parser and another to the next; refusing such text closes that gap.

use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, Error, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

/// The value in `bytes`, or `None` if it is not JSON or repeats a member name anywhere.
pub(crate) fn parse(bytes: &[u8]) -> Option<Value> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = Strict.deserialize(&mut deserializer).ok()?;
    deserializer.end().ok()?;
    Some(value)
}

#[derive(Clone, Copy)]
struct Strict;

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON without repeated member names")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = access.next_element_seed(self)? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut members = Map::new();
        while let Some(name) = access.next_key::<String>()? {
            let value = access.next_value_seed(self)?;
            if members.insert(name, value).is_some() {
                return Err(A::Error::custom("duplicate member"));
            }
        }
        Ok(Value::Object(members))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_without_repeats_reads_as_usual() {
        assert_eq!(
            parse(br#"{"a":1,"b":[true,null,"x",{"c":-2.5}]}"#),
            Some(json!({"a": 1, "b": [true, null, "x", {"c": -2.5}]}))
        );
    }

    #[test]
    fn a_repeated_member_name_at_any_depth_is_refused() {
        assert_eq!(parse(br#"{"sub":"a","sub":"b"}"#), None);
        assert_eq!(parse(br#"{"x":[{"y":1,"y":1}]}"#), None);
    }

    #[test]
    fn text_that_is_not_one_json_value_is_refused() {
        for text in [&b""[..], b"{", b"{} {}", b"{\"a\":}"] {
            assert_eq!(parse(text), None, "{}", String::from_utf8_lossy(text));
        }
    }
}
