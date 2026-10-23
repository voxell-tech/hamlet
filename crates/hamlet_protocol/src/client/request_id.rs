//! The id a client gives each request, which the answers carry back.

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A request's id: a UUID in its 36-character hyphenated form, in
/// either case, kept as it was written.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId(String);

/// Text that is not a [`RequestId`].
#[derive(Debug, PartialEq, thiserror::Error)]
#[error("a request id is a UUID in the hyphenated form")]
pub struct InvalidRequestId;

impl RequestId {
    /// A random id, a version 4 UUID.
    #[expect(
        clippy::new_without_default,
        reason = "every id is new and random; a default would read \
                  as a fixed one"
    )]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RequestId {
    type Error = InvalidRequestId;

    fn try_from(text: String) -> Result<Self, InvalidRequestId> {
        // The schema's `uuid` format takes the hyphenated form alone;
        // uuid parses the braced, URN and simple forms too, none of
        // them 36 characters long.
        if text.len() == 36 && uuid::Uuid::try_parse(&text).is_ok() {
            Ok(Self(text))
        } else {
            Err(InvalidRequestId)
        }
    }
}

impl FromStr for RequestId {
    type Err = InvalidRequestId;

    fn from_str(text: &str) -> Result<Self, InvalidRequestId> {
        Self::try_from(text.to_owned())
    }
}

impl Deref for RequestId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for RequestId {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for RequestId {
    // Inline, each field's schema is the one it had as a plain
    // string, so the wire's schema does not move.
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "RequestId".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "format": "uuid"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    #[test]
    fn only_the_hyphenated_form_is_a_request_id() {
        for text in [
            "",
            "0f8fad5bd9cb469fa16570867728950e",
            "{0f8fad5b-d9cb-469f-a165-70867728950e}",
            "urn:uuid:0f8fad5b-d9cb-469f-a165-70867728950e",
            "0f8fad5b-d9cb-469f-a165-70867728950",
            "0f8fad5b-d9cb-469f-a165-70867728950e0",
            "0f8fad5bd-9cb-469f-a165-70867728950e",
            "0f8fad5b-d9cb-469f-a165-70867728950g",
            " f8fad5b-d9cb-469f-a165-70867728950e",
            "0f8fad5b-d9cb-469f-a165-7086772895\u{e9}",
        ] {
            assert_eq!(
                text.parse::<RequestId>(),
                Err(InvalidRequestId)
            );
        }
        let upper = ID.to_uppercase();
        for text in
            [ID, &upper, "00000000-0000-0000-0000-000000000000"]
        {
            assert_eq!(
                text.parse::<RequestId>().unwrap().as_str(),
                text
            );
        }
    }

    #[test]
    fn a_request_id_is_a_plain_string_on_the_wire() {
        let id = ID.parse::<RequestId>().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{ID}\""));
        assert_eq!(
            serde_json::from_str::<RequestId>(&json).unwrap(),
            id
        );
        assert!(serde_json::from_str::<RequestId>("\"7\"").is_err());
    }
}
