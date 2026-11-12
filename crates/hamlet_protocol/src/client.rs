//! The client protocol: messages exchanged by the window or CLI and
//! the server. `docs/protocol.md` describes it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Version;

pub mod discovery;
pub mod transcript;

mod chat;
mod documents;
mod files;
mod hash_id;
mod images;
mod imports;
mod limits;
mod request_id;
mod settings;
mod state;
mod waits;

pub use chat::*;
pub use documents::*;
pub use files::*;
pub use images::*;
pub use imports::*;
pub use limits::*;
pub use request_id::*;
pub use settings::*;
pub use state::*;
pub use waits::*;

impl ClientMessage {
    /// The id of this request, if it has one.
    pub fn id(&self) -> Option<&RequestId> {
        match self {
            Self::Hello(_) | Self::Close(_) => None,
            Self::Say(Say { id, .. })
            | Self::PutImage(PutImage { id, .. })
            | Self::PutImagePart(PutImagePart { id, .. })
            | Self::PutFile(PutFile { id, .. })
            | Self::PutFilePart(PutFilePart { id, .. })
            | Self::Answer(Answer { id, .. })
            | Self::Stop(Stop { id, .. })
            | Self::Rewind(Rewind { id, .. })
            | Self::New(New { id, .. })
            | Self::Resume(Resume { id, .. })
            | Self::ImportProject(ImportProject { id, .. })
            | Self::Retry(Retry { id, .. })
            | Self::Edit(
                Edit::Charter { id, .. }
                | Edit::Character { id, .. }
                | Edit::Memory { id, .. }
                | Edit::Limits(
                    EditLimits::Set { id, .. }
                    | EditLimits::Whole { id, .. },
                )
                | Edit::Settings { id, .. }
                | Edit::Catalogue { id, .. },
            )
            | Self::Open(Open { id, .. })
            | Self::Login(
                Login::Start { id, .. }
                | Login::Reply { id, .. }
                | Login::Cancel { id, .. },
            )
            | Self::Check(
                Check::Model { id, .. } | Check::Connection { id },
            )
            | Self::Read(
                Read::Prompt { id, .. }
                | Read::History { id, .. }
                | Read::Models { id, .. }
                | Read::Image { id, .. }
                | Read::Papers { id, .. }
                | Read::Charter { id },
            )
            | Self::MoveLodges(MoveLodges { id }) => Some(id),
        }
    }
}

impl ServerMessage {
    /// The request this message answers, if any.
    pub fn to(&self) -> Option<&RequestId> {
        match self {
            Self::Reply(reply) => Some(&reply.to),
            Self::Fault(fault) => fault.to.as_ref(),
            Self::Transcript(transcript) => transcript.to.as_ref(),
            Self::Login(login) => login.to.as_ref(),
            Self::Read(
                ReadResult::Prompt { to, .. }
                | ReadResult::History { to, .. }
                | ReadResult::Models { to, .. }
                | ReadResult::Image { to, .. }
                | ReadResult::Papers { to, .. }
                | ReadResult::Charter { to, .. },
            ) => Some(to),
            Self::State(_) | Self::Change(_) | Self::Turn(_) => None,
        }
    }
}

/// The client protocol version this server speaks.
pub const VERSION: Version = Version::new(0, 1, 0);

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ClientMessage {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "say")]
    Say(Say),
    #[serde(rename = "put-image")]
    PutImage(PutImage),
    #[serde(rename = "put-image-part")]
    PutImagePart(PutImagePart),
    #[serde(rename = "put-file")]
    PutFile(PutFile),
    #[serde(rename = "put-file-part")]
    PutFilePart(PutFilePart),
    #[serde(rename = "answer")]
    Answer(Answer),
    #[serde(rename = "stop")]
    Stop(Stop),
    #[serde(rename = "rewind")]
    Rewind(Rewind),
    #[serde(rename = "new")]
    New(New),
    #[serde(rename = "resume")]
    Resume(Resume),
    #[serde(rename = "import-project")]
    ImportProject(ImportProject),
    #[serde(rename = "retry")]
    Retry(Retry),
    #[serde(rename = "edit")]
    Edit(Edit),
    #[serde(rename = "open")]
    Open(Open),
    #[serde(rename = "close")]
    Close(Close),
    #[serde(rename = "login")]
    Login(Login),
    #[serde(rename = "check")]
    Check(Check),
    #[serde(rename = "read")]
    Read(Read),
    #[serde(rename = "move-lodges")]
    MoveLodges(MoveLodges),
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
#[expect(
    clippy::large_enum_variant,
    reason = "a wire type; a box would show in every pattern"
)]
pub enum ServerMessage {
    #[serde(rename = "state")]
    State(State),
    #[serde(rename = "change")]
    Change(Changes),
    #[serde(rename = "turn")]
    Turn(Turn),
    #[serde(rename = "transcript")]
    Transcript(Transcript),
    #[serde(rename = "reply")]
    Reply(Reply),
    #[serde(rename = "login")]
    Login(LoginProgress),
    #[serde(rename = "read")]
    Read(ReadResult),
    #[serde(rename = "fault")]
    Fault(Fault),
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub token: String,
    pub v: Version,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    /// The id a file is kept under once its last part arrived, or
    /// as it was begun when it has no bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "FileId")]
    pub file: Option<FileId>,
    /// The files of the line a rewind took back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<AttachedFile>,
    /// The id an image is kept under once its last part arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "ImageId")]
    pub image: Option<ImageId>,
    /// The images of the line a rewind took back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<AttachedImage>,
    pub kind: ReplyKind,
    pub text: String,
    pub to: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub upload: Option<String>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ReplyKind {
    #[serde(rename = "notice")]
    Notice,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "rewound")]
    Rewound,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Fault {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "RequestId")]
    pub to: Option<RequestId>,
}

pub(crate) fn present<'de, D, T>(
    deserializer: D,
) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
