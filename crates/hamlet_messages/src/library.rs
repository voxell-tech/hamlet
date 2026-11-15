//! The calls the `library` actor takes, and its answers, and the
//! calls it makes on an agent.
//!
//! The library keeps the skills every agent may use. An agent offers
//! one in parts ([`Offer`]), and the realm brings the verdict on it
//! ([`Verdict`]).

use hamlet_protocol::client::RequestId;
use serde::{Deserialize, Serialize};

use crate::Repeat;
use crate::address::{Agent, Library, Realm};
use crate::bytes::Sha256;
use crate::profile::{By, Piece, TooLarge};
use crate::realm::Done;
use crate::workspace::CallId;

/// The library's id of an offer.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct OfferId(pub u32);

/// The skills on the shelves, of one version: it counts up with every
/// skill shelved or taken off.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShelfList {
    pub version: u32,
    pub skills: Vec<Skill>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub usage: String,
}

/// Asks for the shelves, and to be called with [`ShelvesChanged`]
/// when they change. The caller's row is kept once, and the answer is
/// the list as it is now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shelves {
    /// The version the caller holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_seen: Option<u32>,
}
message!(Shelves, "shelves", Agent => Library, ShelfList, |_| {
    Repeat::Finds
});

/// Reads the shelves, for a librarian's instructions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadShelves;
message!(ReadShelves, "read_shelves", Realm => Library, ShelfList);

/// One part of the files of `skills/<skill>/` in the caller's
/// filesystem, offered to the library. The parts of one offer share
/// `call`, count `part` from 0 and come in order: a part whose
/// earlier part was not kept fails. A first part drops the caller's
/// offers that did not end. The `last` one keeps the offer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub call: CallId,
    pub skill: String,
    pub reason: String,
    pub part: u32,
    pub last: bool,
    pub files: Vec<Piece>,
}

family!(
    /// An offer whose shape is refused, from the part alone.
    BadOffer {
        #[error(
            "`skill` is its folder under skills/, in letters, digits, \
             hyphens and underscores."
        )]
        NotASkillName { skill: String },
        #[error(
            "`{path}` is no file path every system writes: names \
             between single slashes, none empty, `.` or `..`, none over \
             255 bytes"
        )]
        NotAPath { path: String },
    }
);

family!(
    /// What the library finds in the parts of an offer, once it holds
    /// them.
    BadSkill {
        #[error("`{skill}` holds more than the library takes, 4 MB.")]
        SkillTooLarge { skill: String },
        #[error(
            "skills/{skill}/SKILL.md must open with a front matter \
             `name:` and `description:`"
        )]
        NoFrontMatter { skill: String },
    }
);

family!(OnTheShelves {
    #[error(
        "The library has a skill named `{skill}` already. Use it, or \
         offer yours under another name if it does something else."
    )]
    AlreadyShelved { skill: String },
    /// A whole offer of it waits for its verdict, the caller's own
    /// too. An offer whose last part never came is not one.
    #[error("Another offer of that skill is being judged.")]
    BeingJudged { skill: String },
});

union!(OfferRefusal {
    Shape(BadOffer),
    Size(TooLarge),
    Skill(BadSkill),
    Shelves(OnTheShelves),
});

answer!(OfferAnswer<OfferRefusal> {
    /// The offer is kept, to be judged once the caller submits it.
    Kept { offer: OfferId },
    /// A part that is not the last was kept.
    More,
});
message!(Offer, "offer", Agent => Library, OfferAnswer, |offer| {
    Repeat::Part { id: &offer.call.0, part: offer.part }
});

/// The librarian's verdict on an offer. Accepted, the library shelves
/// the skill, tells every listener and asks the offerer to drop its
/// copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub id: RequestId,
    pub offer: OfferId,
    pub accepted: bool,
    pub words: String,
}

family!(VerdictRefusal {
    #[error("there is no offer {}", offer.0)]
    NoSuchOffer { offer: OfferId },
    #[error("the offer {} was judged already", offer.0)]
    Judged { offer: OfferId },
});
message!(Verdict, "verdict", Realm => Library, Done<VerdictRefusal>, |verdict| {
    Repeat::Id(verdict.id.as_str())
});

/// Takes a skill off the shelves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unshelve {
    pub id: RequestId,
    pub skill: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub by: By,
}

family!(NoSkill {
    #[error("no skill is named `{skill}`")]
    NoSuchSkill { skill: String },
});
message!(Unshelve, "unshelve", Realm => Library, Done<NoSkill>, |unshelve| {
    Repeat::Id(unshelve.id.as_str())
});

/// Gives the agent the shelves after they changed. The agent keeps
/// the list when its version is newer than the one it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShelvesChanged {
    pub list: ShelfList,
}
message!(ShelvesChanged, "shelves_changed", Library => Agent, ());

/// Asks the agent to remove `skills/<skill>/` now that the library
/// shelved it, if its files are still the ones it offered: `digest`
/// is the SHA-256 over each offered file in path order, each as its
/// path's length, its path, its bytes' length and its bytes, each
/// length eight bytes big-endian.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropCopy {
    pub skill: String,
    pub digest: Sha256,
}
message!(DropCopy, "drop_copy", Library => Agent, ());
