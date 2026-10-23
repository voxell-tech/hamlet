//! Workspace protocol v1, independent of Substrate identities: the
//! messages Hamlet posts, each operation with the reply it gets, and
//! the limits both sides hold to.

mod bytes;
mod walk;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use self::bytes::Bytes;
pub use self::walk::{Listed, WALK_BYTES, walk};

pub const VERSION: u8 = 1;
/// Largest file one operation carries, in bytes.
pub const MAX_FILE: usize = 4 * 1024 * 1024;
pub const MAX_BODY: usize = 12 * 1024 * 1024;
/// Most bytes of output a command keeps.
pub const MAX_OUTPUT: usize = 1024 * 1024;
/// Most bytes of file contents a tree listing reads.
pub const TREE_BYTES: u64 = 512 * 1024 * 1024;
/// Most entries a tree listing gives.
pub const TREE_ENTRIES: u64 = 20_000;
/// Most entries a folder listing gives.
pub const LIST_ENTRIES: usize = 10_000;
/// Most files of a library snapshot; its bytes are at most
/// [`MAX_FILE`].
pub const LIBRARY_FILES: usize = 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub home: String,
    pub actor_uid: String,
    pub message: Message,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub id: String,
    pub workspace: String,
    pub agent: u32,
    pub project: bool,
    pub network: bool,
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<Library>,
    pub operation: Operation,
}

/// A snapshot of Hamlet's authoritative shared shelf, named by its
/// [`Library::digest`]. The files may be left out once the worker
/// holds them.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<LibraryFile>>,
}

/// A regular file of the shared shelf.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryFile {
    pub path: String,
    pub data: Bytes,
    pub mode: u32,
}

impl Library {
    /// The snapshot of `files`, carrying them.
    pub fn of(files: Vec<LibraryFile>) -> Self {
        Self {
            sha256: Self::digest(&files),
            files: Some(files),
        }
    }

    /// The same snapshot named by its digest alone.
    pub fn named(&self) -> Self {
        Self {
            sha256: self.sha256.clone(),
            files: None,
        }
    }

    /// The SHA-256 of `files` in path order, which names a snapshot.
    pub fn digest(files: &[LibraryFile]) -> String {
        let mut sorted = files.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let mut digest = Sha256::new();
        for file in sorted {
            digest.update(file.path.as_bytes());
            digest.update([0]);
            digest.update(file.mode.to_le_bytes());
            digest.update((file.data.0.len() as u64).to_le_bytes());
            digest.update(&file.data.0);
        }
        format!("{:x}", digest.finalize())
    }
}

/// An operation, which [`Call::Reply`] answers.
pub trait Call: Into<Operation> {
    type Reply: Serialize + DeserializeOwned;
}

fn is_false(value: &bool) -> bool {
    !value
}

macro_rules! operations {
    ($(
        $(#[$meta:meta])*
        $name:ident { $($(#[$field_meta:meta])* $field:ident: $ty:ty),* $(,)? } -> $reply:ty;
    )*) => {
        $(
            $(#[$meta])*
            #[derive(Clone, Debug, Deserialize, Serialize)]
            #[serde(deny_unknown_fields)]
            pub struct $name { $($(#[$field_meta])* pub $field: $ty),* }

            impl From<$name> for Operation {
                fn from(call: $name) -> Self {
                    Self::$name(call)
                }
            }

            impl Call for $name {
                type Reply = $reply;
            }
        )*

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        pub enum Operation {
            $($name($name)),*
        }

        #[cfg(test)]
        impl Operation {
            /// Whether `value` is this operation's reply.
            fn is_answered_by(&self, value: Value) -> bool {
                match self {
                    $(Self::$name(_) => {
                        serde_json::from_value::<$reply>(value).is_ok()
                    })*
                }
            }
        }
    };
}

// `no_follow` refuses a link anywhere in the path. Tree and mode
// operations never follow one.
operations! {
    /// Makes the house's `skills`, `inbox` and `NOTES.md` where they
    /// are missing.
    InitHouse {} -> ();
    /// Lists a tree with each regular file's SHA-256.
    TreeList { path: String, entries: u64, bytes: u64 } -> Vec<TreeEntry>;
    /// Writes a new file with `mode`, making its missing folders.
    TreeWrite { path: String, data: Bytes, mode: u32 } -> ();
    /// Sets the permission bits of a file or folder.
    SetMode { path: String, mode: u32 } -> ();
    Exec { argv: Vec<String>, cwd: String, timeout_ms: u64 } -> Ran;
    Read {
        path: String,
        #[serde(default, skip_serializing_if = "is_false")]
        no_follow: bool,
    } -> Contents;
    Info {
        path: String,
        #[serde(default, skip_serializing_if = "is_false")]
        no_follow: bool,
    } -> Stat;
    List {
        path: String,
        #[serde(default, skip_serializing_if = "is_false")]
        no_follow: bool,
    } -> Vec<Stat>;
    Canonical { path: String } -> String;
    /// Writes a file, making its missing folders with `parents`.
    Write {
        path: String,
        data: Bytes,
        append: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        parents: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        no_follow: bool,
    } -> ();
    Mkdir {
        path: String,
        recursive: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        no_follow: bool,
    } -> ();
    Remove {
        path: String,
        recursive: bool,
        force: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        no_follow: bool,
    } -> ();
    /// The regular files under a folder in [`walk()`]'s order, passing
    /// over folders named in `skip`.
    Walk { path: String, skip: Vec<String>, files: u64 } -> Walked;
}

/// What a command printed and how it ended.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ran {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// A regular file's bytes and what `stat` saw of it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contents {
    pub data: Bytes,
    pub info: Stat,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    File,
    Directory,
    Symlink,
}

/// A path as `lstat` sees it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stat {
    pub name: String,
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub mtime_ms: u64,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TreeKind {
    File,
    Directory,
    Link,
    Special,
}

/// An entry of a listed tree, by its path under the tree's root,
/// which is `.`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    pub path: String,
    pub kind: TreeKind,
    pub mode: u32,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// The files a [`Walk`] found, and whether it stopped at its limit.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize,
)]
#[serde(deny_unknown_fields)]
pub struct Walked {
    pub files: Vec<Found>,
    pub capped: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Found {
    pub path: String,
    pub size: u64,
}

/// The answer to a status or a cancellation.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize,
)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub state: State,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Cancelling,
    Cancelled,
    Completed,
}

/// Why an operation failed.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    Aborted,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    NotDirectory,
    IsDirectory,
    Invalid,
    NotSupported,
    Timeout,
    OutputLimit,
    /// A tree with more entries than the request allowed.
    TooMany,
    /// A tree with more bytes than the request allowed.
    TooLarge,
    Unavailable,
    Confinement,
    Uncertain,
    /// The worker does not hold the library a request names.
    LibraryMissing,
    #[serde(other)]
    Unknown,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Aborted => "aborted",
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::PermissionDenied => "permission_denied",
            Self::NotDirectory => "not_directory",
            Self::IsDirectory => "is_directory",
            Self::Invalid => "invalid",
            Self::NotSupported => "not_supported",
            Self::Timeout => "timeout",
            Self::OutputLimit => "output_limit",
            Self::TooMany => "too_many",
            Self::TooLarge => "too_large",
            Self::Unavailable => "unavailable",
            Self::Confinement => "confinement",
            Self::Uncertain => "uncertain",
            Self::LibraryMissing => "library_missing",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Ok {
        value: Value,
    },
    Error {
        code: Code,
        message: String,
        uncertain: bool,
    },
}

impl Reply {
    pub fn error(code: Code, message: &str, uncertain: bool) -> Self {
        Self::Error {
            code,
            message: message.into(),
            uncertain,
        }
    }

    pub fn ok(value: impl Serialize) -> Self {
        Self::Ok {
            value: serde_json::to_value(value)
                .expect("a reply serializes"),
        }
    }

    pub fn uncertain() -> Self {
        Self::error(
            Code::Uncertain,
            "The workspace operation may have changed files. \
             Inspect its result before repeating it.",
            true,
        )
    }
}

pub fn is_valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
}

pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{Message, Operation, Reply, is_valid_id};

    /// Messages as Hamlet's server sends them, which its own tests
    /// build again from its requests.
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
            assert!(is_valid_id(message.id()), "{name}");
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
            if let Message::Execute { request } = &message
                && let Reply::Ok { value } = reply
            {
                assert!(
                    request.operation.is_answered_by(value),
                    "{name}: the reply is not the operation's"
                );
            }
            if let Message::Execute { request } = message
                && let Operation::Write(write) = request.operation
            {
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
                assert_eq!(write.data.0, expected, "{name}");
            }
        }
    }

    #[test]
    fn omitted_read_only_is_refused() {
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
        assert!(serde_json::from_value::<Message>(message).is_err());
    }
}
