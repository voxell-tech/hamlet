//! The calls a `profile` actor takes, and its answers, and the calls
//! it makes on an agent.
//!
//! The realm sends every change a chief or an agent asks for, under
//! the id of the request that asked. An agent sends two: [`Files`],
//! which reads its own branch, and [`Save`], which commits to it.
//! Neither names a branch: an agent's branch is the one the profile
//! keeps for the agent the host's stamp names.

use hamlet_protocol::client::RequestId;
use serde::{Deserialize, Deserializer, Serialize};

use crate::Repeat;
use crate::address::{Agent, Profile, Realm};
use crate::bytes::Base64;
pub use crate::bytes::Sha1;
use crate::realm::{Done, ProfileId};
use crate::sender::Reader;
use crate::workspace::{AgentNumber, CallId};

/// A commit of a profile's repository.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct CommitId(pub Sha1);

/// A version's name: a tag of the profile's history.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct VersionName(pub String);

/// A commit as a chief or an agent names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Revision {
    /// `main`'s tip.
    Main,
    /// A version, or a branch by its name.
    Version {
        version: VersionName,
    },
    Commit {
        commit: CommitId,
    },
}

/// The realm's id of a dream.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct DreamId(pub u32);

/// Who a change was made for, as the realm passes it on.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum By {
    Chief { chief: u32 },
    Agent { agent: AgentNumber },
}

/// A character as an agent is given it. The agent keeps one whose
/// `version` is newer than the one it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Character {
    pub version: u32,
    pub text: String,
}

/// A file, or a piece of one too large for a part: its bytes from
/// `offset`. A whole file is one piece at 0 that is `last`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Piece {
    pub path: String,
    pub offset: u32,
    pub bytes: Base64,
    pub last: bool,
    #[serde(
        default,
        deserialize_with = "null_is_false",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub executable: bool,
}

fn null_is_false<'de, D: Deserializer<'de>>(
    from: D,
) -> Result<bool, D::Error> {
    Ok(Option::<bool>::deserialize(from)?.unwrap_or(false))
}

/// A path, in a list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Path {
    pub path: String,
}

/// What `profile/<n>` is created with. The host keeps it and gives it
/// at every start, which makes the repository from it unless the
/// repository exists or was erased. A character of more than 4000
/// characters fails the start, as no input does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileInput {
    /// A new profile whose first commit holds `CHARACTER.md`.
    Character { text: String },
    /// A new profile whose first commit holds the files of another
    /// profile's commit `at`.
    Fork { profile: ProfileId, at: CommitId },
}

family!(BadPath {
    #[error("there is no file `{path}` here")]
    NotAPath { path: String },
});

family!(Protected {
    #[error("`{path}` is not a file this call may change")]
    Withheld { path: String },
});

family!(TooLong {
    #[error("a character may have at most {most} characters")]
    CharacterTooLong { most: u32 },
});

family!(TooLarge {
    #[error("a part holds at most {most} bytes")]
    PartTooLarge { most: u32 },
});

family!(Missing {
    #[error("`scholar#{}` has no version `{}`", profile.0, version.0)]
    NoVersion { profile: ProfileId, version: VersionName },
});

family!(NotAHire {
    #[error("`hire#{}` is not a hire of this profile", hire.0)]
    NoHire { hire: AgentNumber },
});

family!(BadCommit {
    #[error("`scholar#{}` has no version `{}`", profile.0, commit.0)]
    NoCommit { profile: ProfileId, commit: CommitId },
    #[error("the commit `{}` is not on main", commit.0)]
    NotOnMain { commit: CommitId },
});

family!(
    /// A commit an agent names that the profile no longer has, as
    /// after a purge. The agent fills again from the branch's tip. After
    /// a refused save it keeps the files it changed, but for those the
    /// purge asked it to remove, and saves them on the new tip.
    Gone {
        #[error("the commit `{}` is gone", commit.0)]
        CommitGone { commit: CommitId },
    }
);

family!(DreamOpen {
    #[error("the dream {} of this profile is open; ask once it ends", dream.0)]
    Dreaming { dream: DreamId },
});

union!(PinRefusal { Missing(Missing), Hire(NotAHire) });
union!(RevertRefusal { Missing(Missing), Commit(BadCommit) });
union!(EditRefusal {
    Path(BadPath),
    Protected(Protected),
    Size(TooLong),
});
union!(PurgeRefusal { Path(BadPath), Dream(DreamOpen) });
union!(SaveRefusal {
    Path(BadPath),
    Protected(Protected),
    Size(TooLarge),
    Gone(Gone),
});

/// Makes the branch `name` when the profile does not have it, at the
/// version `from`, or at `main` when absent. A repeat finds the
/// branch and answers the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<VersionName>,
}
message!(Branch, "branch", Realm => Profile, Done<Missing>, |_| {
    Repeat::Finds
});

/// Tags a commit as a version: `at`, or `main`'s tip when absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: RequestId,
    pub version: VersionName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<CommitId>,
    pub note: String,
    pub by: By,
}

family!(TagRefusal {
    #[error(
        "`{}` is not a version's name: use at most 32 letters, digits, \
         dots and hyphens",
        version.0
    )]
    BadName { version: VersionName },
    #[error("the version `{}` exists already; choose another", version.0)]
    Taken { version: VersionName },
    #[error("`scholar#{}` has no version `{}`", profile.0, commit.0)]
    NoSuchCommit { profile: ProfileId, commit: CommitId },
});

answer!(TagAnswer<W = TagRefusal> {
    Tagged { version: VersionName, commit: CommitId },
});
message!(Tag, "tag", Realm => Profile, TagAnswer, |tag| {
    Repeat::Id(tag.id.as_str())
});

/// Pins a hire to the version `to`, or to `main` again when absent,
/// and takes it into the hire's branch, the hire's side winning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub id: RequestId,
    pub hire: AgentNumber,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<VersionName>,
    pub by: By,
}

answer!(PinAnswer<PinRefusal> {
    Pinned { version: VersionName, commit: CommitId },
    FollowsMain,
});
message!(Pin, "pin", Realm => Profile, PinAnswer, |pin| {
    Repeat::Id(pin.id.as_str())
});

/// Undoes `what` with one commit on `main`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revert {
    pub id: RequestId,
    pub what: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub by: By,
}

answer!(RevertAnswer<W = RevertRefusal> {
    /// `clashes` changed again since and kept their later lines.
    Reverted { commit: CommitId, clashes: Vec<Path> },
    /// `main` holds nothing of it any more.
    Unchanged,
});
message!(Revert, "revert", Realm => Profile, RevertAnswer, |revert| {
    Repeat::Id(revert.id.as_str())
});

/// Takes a file or a folder out of every commit of every branch and
/// version, and asks every hire's agent to remove it too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Purge {
    pub id: RequestId,
    pub path: String,
    pub by: By,
}

answer!(PurgeAnswer<W = PurgeRefusal> {
    /// `still_hold` are the hires whose agents remove the path once
    /// idle, or did not answer.
    Purged { branches: u32, still_hold: Vec<AgentNumber> },
});
message!(Purge, "purge", Realm => Profile, PurgeAnswer, |purge| {
    Repeat::Id(purge.id.as_str())
});

/// Writes one file on `main`, or removes it when `text` is empty; a
/// chief's empty character is written empty. A chief writes the
/// character or a file of memory, and an agent, the Hand, any file.
/// `message` names the commit; absent or empty, the edit's own words
/// do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub id: RequestId,
    pub path: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub by: By,
}
message!(Edit, "edit", Realm => Profile, Done<EditRefusal>, |edit| {
    Repeat::Id(edit.id.as_str())
});

/// Whose learning a dream goes over.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DreamOf {
    Hire { agent: AgentNumber },
    Profile,
}

/// Checks `main` out, and a hire's branch for a hire's dream, into
/// the dream's folders. The dream's row tells its repeat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareDream {
    pub dream: DreamId,
    pub of: DreamOf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Prepared {
    /// The folders, under the realm folder, that the dream's agent is
    /// given reach of.
    Ready {
        main: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hire: Option<String>,
    },
    /// The hire's branch holds nothing `main` lacks.
    Nothing,
}
message!(PrepareDream, "prepare_dream", Realm => Profile, Prepared, |_| {
    Repeat::Finds
});

/// Ends a dream: commits its folders when `done`, and removes them.
/// The dream's row tells its repeat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndDream {
    pub dream: DreamId,
    pub done: bool,
    pub summary: String,
    /// The hire's name as the realm gives it, for a hire's dream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hire_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DreamEnd {
    Dreamt {
        /// The commit on `main`, when the dream changed it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        main: Option<CommitId>,
    },
    Nothing,
}
message!(EndDream, "end_dream", Realm => Profile, DreamEnd, |_| {
    Repeat::Finds
});

/// Drops the parts kept of `agent`'s save whose last part has not
/// come, once the agent has ended. A repeat finds none and answers
/// the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropSaveParts {
    pub agent: AgentNumber,
}
message!(DropSaveParts, "drop_save_parts", Realm => Profile, (), |_| {
    Repeat::Finds
});

/// Reads the profile's versions and the latest commits of `main`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionList {
    pub versions: Vec<VersionSeen>,
    /// The last [`MOST_COMMITS_SEEN`] commits of `main`, its tip
    /// first.
    pub commits: Vec<CommitSeen>,
}

/// The most commits a [`VersionList`] holds.
pub const MOST_COMMITS_SEEN: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitSeen {
    pub commit: CommitId,
    /// The first line of its message.
    pub message: String,
    /// Its author: `chief:<n>`, `agent:<n>`, `dream:<n>` or `realm`.
    pub by: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionSeen {
    pub version: VersionName,
    pub commit: CommitId,
    pub note: String,
    /// The record's name of who made it: `chief:<n>`, `agent:<n>`.
    pub by: String,
}
message!(Versions, "versions", Reader => Profile, VersionList);

/// Reads the profile's papers at `main`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Papers;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperList {
    pub character: String,
    pub memory: Vec<Paper>,
    pub skills: Vec<Paper>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paper {
    pub path: String,
    pub description: String,
}
message!(Papers, "papers", Reader => Profile, PaperList);

/// Lists the files of `main`, or of the commit `at` names, or reads
/// the file `path` there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Read {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Revision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// The most bytes of a file a [`Read`] gives.
pub const MOST_READ: u32 = 50 * 1024;

union!(ReadFileRefusal {
    Missing(Missing),
    Commit(BadCommit),
    Path(BadPath),
});

answer!(ReadAnswer<W = ReadFileRefusal> {
    Listed { paths: Vec<Path> },
    /// The file's text, cut after [`MOST_READ`] bytes when `cut`.
    Text { text: String, cut: bool },
});
message!(Read, "read", Reader => Profile, ReadAnswer);

/// Removes the repository. The actor ends after it answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Erase {
    pub id: RequestId,
}
message!(Erase, "erase", Realm => Profile, Done<DreamOpen>, |erase| {
    Repeat::Id(erase.id.as_str())
});

/// Where the next part of a fill starts. It names the commit the
/// first part read, so every part of one fill reads one commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub commit: CommitId,
    /// The item of the fill, counting its removed paths first and
    /// then its files in path order, and the byte within that file.
    pub file: u32,
    pub offset: u32,
}

/// Reads one part of the caller's branch: every file at its tip, or
/// what changed from `since` to the tip. `CHARACTER.md` and `inbox/`
/// are never sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Files {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<CommitId>,
    /// The `next` of the part before; absent for the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Cursor>,
}

answer!(FilesAnswer<Gone> {
    Part {
        /// The commit this fill reads.
        tip: CommitId,
        files: Vec<Piece>,
        /// What `since` held that the tip does not.
        removed: Vec<Path>,
        /// Absent on the last part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<Cursor>,
        /// On the first part: the character at the tip.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        character: Option<Character>,
    },
});
message!(Files, "files", Agent => Profile, FilesAnswer);

/// A change to one file of the caller's filesystem.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileChange {
    Write(Piece),
    Remove { path: String },
}

/// One part of what the caller's filesystem changed since `base`, the
/// commit it holds. The parts of one save share `call`, count `part`
/// from 0 and come in order: a part other than the next one fails.
/// The parts kept outlive a sleep and a restart. A first part drops
/// the caller's save that did not end. The `last` one commits them
/// all on `base` as one commit and joins it to the branch, the hire's
/// side winning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Save {
    pub call: CallId,
    pub base: CommitId,
    pub part: u32,
    pub last: bool,
    pub changes: Vec<FileChange>,
}

answer!(SaveAnswer<SaveRefusal> {
    /// The branch's tip, which the caller now pulls from `base`.
    Saved { tip: CommitId },
    /// A part that is not the last was kept.
    Kept,
});
message!(Save, "save", Agent => Profile, SaveAnswer, |save| {
    Repeat::Part { id: &save.call.0, part: save.part }
});

/// Tells the agent that its profile moved its branch, or that the
/// character at its tip changed. Between turns the agent saves what
/// its filesystem changed, then pulls [`Files`] `since` the commit it
/// holds. A call that fails is not made again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    pub tip: CommitId,
    /// The character, when it changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<Character>,
}
message!(Moved, "moved", Profile => Agent, ());

/// Asks the agent to remove a file or a folder of its filesystem, for
/// a purge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovePath {
    pub path: String,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Removal {
    Removed,
    /// The agent is in a turn, and removes it when the turn ends.
    Later,
}
message!(RemovePath, "remove_path", Profile => Agent, Removal);

/// A path another branch removed, with the blob it held there, so the
/// taker removes it only where it holds that blob.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub path: String,
    pub blob: Sha1,
}

/// What a merge takes in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// A branch of this profile.
    Branch { name: String },
    /// One part of another profile's branch, as [`Bundle`] read it.
    /// The parts of one merge share its `id`, count `part` from 0 and
    /// come in order; the `last` one merges them all.
    Part {
        part: u32,
        last: bool,
        files: Vec<Piece>,
        removed: Vec<Removed>,
    },
}

/// Merges `from` into the branch `into`, the kept hire's, as one
/// commit by the realm with `message`. Where both changed a file the
/// kept hire's lines win. From another profile's parts, the other's
/// file is also set aside beside it under `absorbed`'s name:
/// `<path>.from-hire-<n>`, or `skills/<skill>-from-hire-<n>/...` for
/// a skill's. `CHARACTER.md` and `inbox/` are not merged. The kept
/// hire is told the branch moved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeBranch {
    pub id: RequestId,
    pub into: String,
    pub absorbed: AgentNumber,
    pub message: String,
    pub from: Source,
}

answer!(MergeAnswer<SaveRefusal> {
    /// `tip` is the merge's commit; `set_aside` the paths it wrote
    /// aside.
    Merged { tip: CommitId, set_aside: Vec<String> },
    /// A part that is not the last was kept.
    Kept,
});
message!(MergeBranch, "merge_branch", Realm => Profile, MergeAnswer, |merge| {
    match &merge.from {
        Source::Branch { .. } => Repeat::Id(merge.id.as_str()),
        Source::Part { part, .. } => Repeat::Part {
            id: merge.id.as_str(),
            part: *part,
        },
    }
});

/// Reads one part of what `branch` changed since it left `main`, for
/// a merge into another profile. `CHARACTER.md` and `inbox/` are
/// never sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub branch: String,
    /// The `next` of the part before; absent for the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Cursor>,
}

answer!(BundleAnswer<Gone> {
    Part {
        files: Vec<Piece>,
        removed: Vec<Removed>,
        /// Absent on the last part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<Cursor>,
    },
});
message!(Bundle, "bundle", Realm => Profile, BundleAnswer);

/// Takes the merge `merge` out of the branch `branch` again, as one
/// commit, and keeps what the branch did after it. The branch's hire
/// is told the branch moved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unmerge {
    pub id: RequestId,
    pub branch: String,
    pub merge: CommitId,
}

answer!(UnmergeAnswer<Gone> {
    Unmerged { tip: CommitId },
});
message!(Unmerge, "unmerge", Realm => Profile, UnmergeAnswer, |unmerge| {
    Repeat::Id(unmerge.id.as_str())
});

/// Removes the branch `name`; answers the same when it is gone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropBranch {
    pub name: String,
}
message!(DropBranch, "drop_branch", Realm => Profile, (), |_| {
    Repeat::Finds
});
