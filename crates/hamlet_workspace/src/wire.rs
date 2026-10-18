//! Workspace protocol v1, independent of Substrate identities.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_FILE: usize = 4 * 1024 * 1024;
pub const MAX_BODY: usize = 12 * 1024 * 1024;
pub const MAX_OUTPUT: usize = 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Execute { request: Request },
    Cancel { id: String },
    Status { id: String },
}

impl Message {
    pub fn id(&self) -> &str {
        match self {
            Self::Execute { request } => &request.id,
            Self::Cancel { id } | Self::Status { id } => id,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub home: String,
    pub actor_uid: String,
    pub message: Message,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub id: String,
    pub workspace: String,
    pub agent: u32,
    pub project: bool,
    pub network: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<Vec<LibraryFile>>,
    pub operation: Operation,
}

/// A regular file from Hamlet's authoritative shared shelf.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryFile {
    pub path: String,
    pub data: String,
    pub mode: u32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    InitHouse,
    TreeList {
        path: String,
        entries: u64,
        bytes: u64,
    },
    TreeWrite {
        path: String,
        data: String,
        mode: u32,
    },
    Exec {
        argv: Vec<String>,
        cwd: String,
        timeout_ms: u64,
    },
    Read {
        path: String,
        no_follow: bool,
    },
    Info {
        path: String,
    },
    List {
        path: String,
    },
    Canonical {
        path: String,
    },
    Write {
        path: String,
        data: String,
        append: bool,
    },
    Mkdir {
        path: String,
        recursive: bool,
    },
    Remove {
        path: String,
        recursive: bool,
        force: bool,
    },
    Rename {
        path: String,
        destination: String,
    },
    Truncate {
        path: String,
        size: u64,
    },
    Flush {
        path: String,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Ok {
        value: Value,
    },
    Error {
        code: String,
        message: String,
        uncertain: bool,
    },
}

impl Reply {
    pub fn error(code: &str, message: &str, uncertain: bool) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
            uncertain,
        }
    }

    pub fn ok(value: impl Into<Value>) -> Self {
        Self::Ok {
            value: value.into(),
        }
    }

    pub fn uncertain() -> Self {
        Self::error(
            "uncertain",
            "The workspace operation may have changed files. \
             Inspect its result before repeating it.",
            true,
        )
    }
}

pub fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde_json::Value;

    use super::{Message, Operation, Reply, valid_id};

    /// The fixture Hamlet's TypeScript client is tested against too.
    const FIXTURE: &str =
        include_str!("../fixtures/protocol-v1.json");

    #[test]
    fn shared_fixture_round_trips() {
        let fixture = serde_json::from_str::<Value>(FIXTURE).unwrap();
        assert_eq!(fixture["protocol"], 1);
        let cases = fixture["cases"].as_array().unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let message = serde_json::from_value::<Message>(
                case["message"].clone(),
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(valid_id(message.id()), "{name}");
            assert_eq!(
                serde_json::to_value(&message).unwrap(),
                case["message"],
                "{name}"
            );
            let reply = serde_json::from_value::<Reply>(
                case["reply"].clone(),
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                serde_json::to_value(&reply).unwrap(),
                case["reply"],
                "{name}"
            );
            if let Message::Execute { request } = message
                && let Operation::Write { data, .. } =
                    request.operation
            {
                let bytes = STANDARD.decode(data).unwrap();
                let expected = match (
                    case["content_utf8"].as_str(),
                    case["content_hex"].as_str(),
                ) {
                    (Some(text), None) => text.as_bytes().to_vec(),
                    (None, Some(hex)) => (0..hex.len())
                        .step_by(2)
                        .map(|at| {
                            u8::from_str_radix(&hex[at..at + 2], 16)
                                .unwrap()
                        })
                        .collect(),
                    _ => panic!("{name}: one declared content"),
                };
                assert_eq!(bytes, expected, "{name}");
            }
        }
    }

    #[test]
    fn omitted_read_only_is_writable() {
        let fixture = serde_json::from_str::<Value>(FIXTURE).unwrap();
        let mut message = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == "write_utf8")
            .unwrap()["message"]
            .clone();
        message["request"]
            .as_object_mut()
            .unwrap()
            .remove("read_only");
        let Message::Execute { request } =
            serde_json::from_value::<Message>(message).unwrap()
        else {
            panic!("not an execute message");
        };
        assert!(!request.read_only);
    }
}
