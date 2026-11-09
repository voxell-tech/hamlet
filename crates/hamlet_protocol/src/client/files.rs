//! Files and folders a chief attaches to a line: sent in parts, kept
//! by an id of their bytes, and named with the line (C33).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::RequestId;
use crate::client::hash_id::hash_id;

hash_id! {
    /// A file's id: the SHA-256 of its bytes, as 64 lowercase
    /// hexadecimal digits.
    FileId, InvalidFileId,
    "a file id is 64 lowercase hexadecimal digits"
}

/// A file a line carries, sent before it: the bytes the server keeps
/// under `id`, and the name the agent finds them under.
#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct AttachedFile {
    /// The byte offset into the line's text, on a character
    /// boundary and at most the text's length, where the file sits.
    /// Without one it goes after the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "u64", range(max = 9_007_199_254_740_991_u64))]
    pub at: Option<u64>,
    pub id: FileId,
    /// Its path in the folder it was attached in, or its name alone,
    /// with `/` between the parts: no empty part, no `.` or `..`.
    #[schemars(length(max = 1024))]
    pub name: String,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub size: u64,
}

impl AttachedFile {
    /// The bytes and the files of one send together (a chat line's
    /// attachments, or an agent's `tell` or `ask`), over which a
    /// chief says it may go on.
    pub const CAP_BYTES: u64 = 32 << 20;
    pub const CAP_FILES: usize = 1000;
    /// The bytes of one file, and the bytes and files of one send,
    /// over which none is taken, whatever a chief says.
    pub const MOST_FILE: u64 = 256 << 20;
    pub const MOST_BYTES: u64 = 1 << 30;
    pub const MOST_FILES: usize = 10_000;

    /// `bytes` as words: `3 bytes`, `12.3 KiB`.
    pub fn size_words(bytes: u64) -> String {
        const UNITS: [&str; 3] = ["KiB", "MiB", "GiB"];
        if bytes < 1024 {
            return format!("{bytes} bytes");
        }
        let mut size = bytes as f64 / 1024.0;
        let mut unit = 0;
        while size >= 1024.0 && unit + 1 < UNITS.len() {
            size /= 1024.0;
            unit += 1;
        }
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Begins sending one file of `size` bytes; the reply names the
/// upload its parts are sent under.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PutFile {
    pub id: RequestId,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub size: u64,
}

/// Sends base64 bytes of a file at a byte offset. The part that
/// completes the file is answered with the file's id.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PutFilePart {
    pub data: String,
    pub id: RequestId,
    pub offset: u64,
    pub upload: String,
}
