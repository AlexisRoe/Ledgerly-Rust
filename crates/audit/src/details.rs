//! A JSON object whose members keep their order.
//!
//! Java reads `details` back as text and Jackson turns it into a `LinkedHashMap`, so the response lists the
//! members in the order Postgres prints the `jsonb` (shorter keys first). `serde_json`'s own map sorts them
//! alphabetically, which would put `importedRows` before `skippedRows`; this type keeps them as read.

use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Details(pub Vec<(String, Value)>);

impl Serialize for Details {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Details {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Members;

        impl<'de> Visitor<'de> for Members {
            type Value = Details;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Details, A::Error> {
                let mut members = Vec::new();
                while let Some(member) = access.next_entry::<String, Value>()? {
                    members.push(member);
                }
                Ok(Details(members))
            }
        }

        deserializer.deserialize_map(Members)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_keep_the_order_they_were_read_in() {
        let text = r#"{"failed": false, "skippedRows": 1, "importedRows": 3}"#;

        let details: Details = serde_json::from_str(text).unwrap();

        assert_eq!(
            serde_json::to_string(&details).unwrap(),
            r#"{"failed":false,"skippedRows":1,"importedRows":3}"#
        );
    }
}
