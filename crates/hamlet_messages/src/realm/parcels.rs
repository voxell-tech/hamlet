//! Parcels: the files a letter or a question sends, which the
//! receiver fetches from the realm part by part.

use std::path::PathBuf;

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::Refusable;
use crate::address::{Agent, Realm};
use crate::bytes::Base64;
use crate::{MOST_PART, Message, worded};

/// The most bytes of a file one part holds. Base64 makes four bytes
/// of three, so a part's answer is under [`crate::MOST_CALL`].
pub const PARCEL_PART: usize = MOST_PART;

/// The realm's id of a parcel.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct ParcelId(pub u32);

/// A file of a parcel, as its receiver is told of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParcelFile {
    pub parcel: ParcelId,
    /// The file's place in the parcel, from 0.
    pub file: u32,
    /// Where the receiver keeps it, under its `inbox/`.
    pub path: PathBuf,
    pub size: u64,
}

/// How much a parcel holds.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct Measure {
    pub files: u32,
    pub bytes: u64,
}

/// Fetches the part `part` of a file of a parcel sent to the caller,
/// each part [`PARCEL_PART`] bytes from the file's start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchPart {
    pub parcel: ParcelId,
    pub file: u32,
    pub part: u32,
}

impl Action for FetchPart {
    type Output = Part<PartRefusal>;
    const NAME: &'static str = "fetch_part";
}

impl Message for FetchPart {
    type To = Realm;
    type From = Agent;
}

/// A part of a file, and whether it is the file's last.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    bound(
        serialize = "W: Serialize + std::fmt::Display",
        deserialize = "W: Deserialize<'de>"
    )
)]
pub enum Part<W> {
    Bytes {
        bytes: Base64,
        last: bool,
    },
    #[serde(serialize_with = "worded")]
    Refused {
        why: W,
    },
}

impl<W> Refusable for Part<W> {
    type Why = W;
    fn refused(why: W) -> Self {
        Self::Refused { why }
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartRefusal {
    #[error("there is no such parcel")]
    NoSuchParcel,
    #[error("the parcel was sent to another agent")]
    NotYours,
    #[error("the parcel has no such file or part")]
    NoSuchPart,
}
