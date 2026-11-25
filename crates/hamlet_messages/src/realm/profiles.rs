//! The acts that make, read, change, purge and dream over a profile,
//! and an agent's acts on the library. A model calls them
//! `create_scholar`, `read_scholar`, `edit_scholar`, `tag`, `revert`,
//! `ask_librarian` and `submit_skill`; a chief, `/dream` and
//! `/purge`. A chief also forks, archives, restores and erases a
//! profile, and welcomes, turns away and moves its house.

use serde::{Deserialize, Serialize};

use super::{
    AgentRefusal, ChiefRequest, Done, HamletId, ProfileId,
    SocietyTool, Standing, ToolCall,
};
use crate::library::OfferId;
use crate::profile::{
    BadCommit, BadPath, DreamOpen, EditRefusal, Missing, Protected,
    PurgeAnswer, PurgeRefusal, ReadAnswer, ReadFileRefusal,
    RevertAnswer, RevertRefusal, Revision, TagAnswer, TagRefusal,
    TooLong, VersionName,
};
use crate::worded;
use crate::workspace::AgentNumber;

family!(
    /// Why an act on the profile it names was not carried out.
    ProfileRefusal {
        #[error("no scholar has the address `scholar#{}`", profile.0)]
        NoSuchProfile { profile: ProfileId },
        #[error("`{label}` is a blank hire's own, not a scholar of the city")]
        Apart { label: String },
    }
);

family!(CreateRefusal {
    #[error(
        "`{name}` is not a name: use one line of 1 to 40 characters \
         with no colon"
    )]
    BadName { name: String },
    #[error("a character may have at most 4000 characters")]
    LongCharacter,
});

/// Makes a profile of the city, from `character`, or from the realm's
/// character when absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProfile {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    bound(
        serialize = "W: Serialize + std::fmt::Display",
        deserialize = "W: Deserialize<'de>"
    )
)]
pub enum Made<W> {
    /// `label` is `scholar#<id> (<name>)`.
    Made { profile: ProfileId, label: String },
    #[serde(serialize_with = "worded")]
    Refused { why: W },
}

impl<W> super::Refusable for Made<W> {
    type Why = W;
    fn refused(why: W) -> Self {
        Self::Refused { why }
    }
}

impl ToolCall for CreateProfile {
    const TOOL: SocietyTool = SocietyTool::CreateProfile;
    type Answer = Made<AgentRefusal<CreateRefusal>>;
}

/// Lists the files of a profile at `main`, or at the commit `at`
/// names, or reads its file `path` there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadProfile {
    pub profile: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Revision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

union!(ReadProfileRefusal {
    Profile(ProfileRefusal),
    Missing(Missing),
    Commit(BadCommit),
    Path(BadPath),
});

impl From<ReadFileRefusal> for ReadProfileRefusal {
    fn from(why: ReadFileRefusal) -> Self {
        match why {
            ReadFileRefusal::Missing(why) => why.into(),
            ReadFileRefusal::Commit(why) => why.into(),
            ReadFileRefusal::Path(why) => why.into(),
        }
    }
}

impl ToolCall for ReadProfile {
    const TOOL: SocietyTool = SocietyTool::ReadProfile;
    type Answer = ReadAnswer<AgentRefusal<ReadProfileRefusal>>;
}

/// Writes one file of a profile's `main`, or removes it when `text`
/// is empty. An agent's waits for a chief to allow it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditProfile {
    pub profile: ProfileId,
    pub path: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

union!(EditProfileRefusal {
    Profile(ProfileRefusal),
    Path(BadPath),
    Protected(Protected),
    Size(TooLong),
});

impl From<EditRefusal> for EditProfileRefusal {
    fn from(why: EditRefusal) -> Self {
        match why {
            EditRefusal::Path(why) => why.into(),
            EditRefusal::Protected(why) => why.into(),
            EditRefusal::Size(why) => why.into(),
        }
    }
}

impl ChiefRequest for EditProfile {
    const NAME: &'static str = "chief_edit_profile";
    type Answer = Done<EditProfileRefusal>;
}

impl ToolCall for EditProfile {
    const TOOL: SocietyTool = SocietyTool::EditProfile;
    type Answer = Done<AgentRefusal<EditProfileRefusal>>;
}

/// Names a profile's `main` as the version `version`. An agent's
/// waits for a chief to allow it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagProfile {
    pub profile: ProfileId,
    pub version: VersionName,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

union!(TagProfileRefusal {
    Profile(ProfileRefusal),
    Tag(TagRefusal),
});

impl ChiefRequest for TagProfile {
    const NAME: &'static str = "chief_tag_profile";
    type Answer = TagAnswer<TagProfileRefusal>;
}

impl ToolCall for TagProfile {
    const TOOL: SocietyTool = SocietyTool::Tag;
    type Answer = TagAnswer<AgentRefusal<TagProfileRefusal>>;
}

/// Undoes `what` with one commit on a profile's `main`. An agent's
/// waits for a chief to allow it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevertProfile {
    pub profile: ProfileId,
    pub what: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

union!(RevertProfileRefusal {
    Profile(ProfileRefusal),
    Missing(Missing),
    Commit(BadCommit),
});

impl From<RevertRefusal> for RevertProfileRefusal {
    fn from(why: RevertRefusal) -> Self {
        match why {
            RevertRefusal::Missing(why) => why.into(),
            RevertRefusal::Commit(why) => why.into(),
        }
    }
}

impl ChiefRequest for RevertProfile {
    const NAME: &'static str = "chief_revert_profile";
    type Answer = RevertAnswer<RevertProfileRefusal>;
}

impl ToolCall for RevertProfile {
    const TOOL: SocietyTool = SocietyTool::Revert;
    type Answer = RevertAnswer<AgentRefusal<RevertProfileRefusal>>;
}

/// Asks the librarian which skills on the shelves help with
/// `question`. The librarian's answer reaches the caller later, as an
/// input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskLibrarian {
    pub question: String,
}

impl ToolCall for AskLibrarian {
    const TOOL: SocietyTool = SocietyTool::AskLibrarian;
    type Answer = Done<Standing>;
}

/// Asks the librarian to judge the offer the library kept as `offer`.
/// The verdict reaches the caller later, as an input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitSkill {
    pub offer: OfferId,
    pub skill: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ToolCall for SubmitSkill {
    const TOOL: SocietyTool = SocietyTool::SubmitSkill;
    type Answer = Done<Standing>;
}

/// Whose learning a chief's dream goes over.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Dreamer {
    Profile { profile: ProfileId },
    Hire { hire: AgentNumber },
}

/// Starts a dream. One dream runs at a time in the realm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dream {
    pub of: Dreamer,
}

family!(DreamRefusal {
    #[error("A dream is running; one dreams at a time.")]
    Running,
    #[error("no agent has the address `hire#{}`", hire.0)]
    NoSuchHire { hire: AgentNumber },
});

union!(DreamAskedRefusal {
    Profile(ProfileRefusal),
    Dream(DreamRefusal),
});

impl ChiefRequest for Dream {
    const NAME: &'static str = "chief_dream";
    type Answer = Done<DreamAskedRefusal>;
}

/// Takes `path` out of every commit of a profile and out of each of
/// its hires' filesystems.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeProfile {
    pub profile: ProfileId,
    pub path: String,
}

union!(PurgeProfileRefusal {
    Profile(ProfileRefusal),
    Path(BadPath),
    Dream(DreamOpen),
});

impl From<PurgeRefusal> for PurgeProfileRefusal {
    fn from(why: PurgeRefusal) -> Self {
        match why {
            PurgeRefusal::Path(why) => why.into(),
            PurgeRefusal::Dream(why) => why.into(),
        }
    }
}

impl ChiefRequest for PurgeProfile {
    const NAME: &'static str = "chief_purge_profile";
    type Answer = PurgeAnswer<PurgeProfileRefusal>;
}

/// Makes a new profile from a version of `from`, `main` when absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkProfile {
    pub from: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<VersionName>,
    pub name: String,
}

family!(
    /// Why a fork the realm took was not made. `label` is the label of
    /// the profile it was made from.
    ForkFailure {
        #[error("the scholar `{label}` could not be forked")]
        NotForked { label: String },
    }
);

union!(ForkRefusal {
    Profile(ProfileRefusal),
    Missing(Missing),
    Create(CreateRefusal),
    Failure(ForkFailure),
});

answer!(
    /// The answer of a [`ForkProfile`]: the new profile, and the label
    /// of the one it was made from, `scholar#<id> (<name>)`.
    Forked<ForkRefusal> {
        Forked { profile: ProfileId, from: String },
    }
);

impl ChiefRequest for ForkProfile {
    const NAME: &'static str = "chief_fork_profile";
    type Answer = Forked;
}

/// Archives a profile and ends its hires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveProfile {
    pub profile: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for ArchiveProfile {
    const NAME: &'static str = "chief_archive_profile";
    type Answer = Lifecycle;
}

/// Brings an archived profile back with the hires its archive ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreProfile {
    pub profile: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for RestoreProfile {
    const NAME: &'static str = "chief_restore_profile";
    type Answer = Lifecycle;
}

/// Erases an archived profile for good, with its hires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EraseProfile {
    pub profile: ProfileId,
}

impl ChiefRequest for EraseProfile {
    const NAME: &'static str = "chief_erase_profile";
    type Answer = Lifecycle;
}

/// Lets a profile waiting at the harbor into the city.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub profile: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for Welcome {
    const NAME: &'static str = "chief_welcome";
    type Answer = Lifecycle;
}

/// Keeps a profile at the harbor, or sends it back there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnAway {
    pub profile: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for TurnAway {
    const NAME: &'static str = "chief_turn_away";
    type Answer = Lifecycle;
}

/// Sets a profile's house down at `spot`. Without one, as for numbers
/// a chief wrote that do not read, the realm looks the profile up and
/// refuses `NoSpot`: today names a missing profile before them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveHouse {
    pub profile: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot: Option<Spot>,
}

/// A spot from the city's middle, in metres, turned `degrees`, or
/// facing the middle when absent.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub x: f64,
    pub z: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degrees: Option<f64>,
}

impl ChiefRequest for MoveHouse {
    const NAME: &'static str = "chief_move_house";
    type Answer = Lifecycle;
}

family!(
    /// Why an act on a profile's life in the city was not carried
    /// out. Each `label` is `scholar#<id> (<name>)`.
    LifeRefusal {
        #[error("the scholar `{label}` no longer lives in the city")]
        Gone { label: String },
        #[error("the scholar `{label}` was erased")]
        Erased { label: String },
        #[error("the scholar `{label}` is still living; remove it first")]
        Living { label: String },
        #[error(
            "the scholar `{label}` waits at the harbor; welcome it into \
             the city first"
        )]
        AtHarbor { label: String },
        #[error("the scholar `{label}` lives in the city already")]
        Housed { label: String },
        #[error("the scholar `{label}` was turned away already")]
        TurnedAway { label: String },
        /// A site that is not a spot in the city, as `x, z`.
        #[error("`{site}` is not an address")]
        BadSite { site: String },
        #[error("the house of `{label}` needs a spot, x and z")]
        NoSpot { label: String },
        #[error(
            "the house of `{label}` would stand on another house or the \
             city's middle"
        )]
        Crowded { label: String },
        #[error("a mayor cannot be ended; seat another mayor first")]
        TheMayor,
        #[error("the hamlet `{}` holds its most hires, {most}", hamlet.0)]
        Full { hamlet: HamletId, most: u32 },
        #[error(
            "the scholar `{label}` could not be erased, so it stays \
             archived"
        )]
        NotErased { label: String },
    }
);

union!(LifecycleRefusal {
    Profile(ProfileRefusal),
    Life(LifeRefusal),
});

answer!(
    /// The answer of an act on a profile's life in the city: the
    /// profile's label, `scholar#<id> (<name>)`.
    Lifecycle<LifecycleRefusal> {
        Done { label: String },
    }
);
