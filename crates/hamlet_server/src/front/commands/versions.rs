//! A chief's commands on a profile's versions: the list, a tag, a
//! revert, a purge, and a hire held at one.

use anyhow::Result;
use hamlet_messages::address::Profile;
use hamlet_messages::bytes::Sha1;
use hamlet_messages::profile::{
    CommitId, Missing, PinAnswer, PinRefusal, PurgeAnswer,
    RevertAnswer, Revision, TagAnswer, VersionList, VersionName,
    Versions,
};
use hamlet_messages::realm::ProfileId;
use hamlet_messages::realm::profiles::{
    PinHire, PurgeProfile, RevertProfile, TagProfile,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{self as protocol, RequestId};

use super::{ask, refused};
use crate::front::call::Calls;
use crate::front::view::View;
use crate::front::{answers, words};
use crate::society::refusal::Refusal;

/// The versions and latest commits of `profile`.
pub(super) async fn list(
    calls: &Calls,
    profile: ProfileId,
) -> Result<Result<String, Refusal>> {
    if let Err(refusal) = label(calls, profile).await? {
        return Ok(Err(refusal));
    }
    let list = calls.call(&Profile(profile), &Versions).await?;
    Ok(Ok(words::versions(&list)))
}

/// Names a commit of `tag`'s profile as its version.
pub(super) async fn tag(
    calls: &Calls,
    id: &RequestId,
    tag: TagProfile,
) -> Result<Result<String, Refusal>> {
    let label = match label(calls, tag.profile).await? {
        Ok(label) => label,
        Err(refusal) => return Ok(Err(refusal)),
    };
    Ok(match ask(calls, id, tag).await? {
        TagAnswer::Tagged { version, .. } => {
            Ok(words::tagged(&label, &version.0))
        }
        TagAnswer::Refused { why } => Err(refused(why)),
    })
}

/// Undoes the commit `which` names on `profile`'s `main`.
pub(super) async fn revert(
    calls: &Calls,
    id: &RequestId,
    profile: ProfileId,
    which: &str,
    reason: Option<String>,
) -> Result<Result<String, Refusal>> {
    let label = match label(calls, profile).await? {
        Ok(label) => label,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let list = calls.call(&Profile(profile), &Versions).await?;
    let (what, undone) = revision(&list, which);
    let act = RevertProfile {
        profile,
        what,
        reason,
    };
    Ok(match ask(calls, id, act).await? {
        RevertAnswer::Reverted { clashes, .. } => {
            Ok(words::reverted(&undone, &label, &clashes))
        }
        RevertAnswer::Unchanged => {
            Ok(words::reverted(&undone, &label, &[]))
        }
        RevertAnswer::Refused { why } => Err(refused(why)),
    })
}

/// Takes `act`'s path out of every commit of its profile and out of
/// each of its hires' filesystems.
pub(super) async fn purge(
    calls: &Calls,
    id: &RequestId,
    act: PurgeProfile,
) -> Result<Result<String, Refusal>> {
    let label = match label(calls, act.profile).await? {
        Ok(label) => label,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let path = act.path.clone();
    Ok(match ask(calls, id, act).await? {
        PurgeAnswer::Purged { branches, .. } => {
            Ok(words::purged(&path, &label, branches))
        }
        PurgeAnswer::Refused { why } => Err(refused(why)),
    })
}

/// Holds `hire` at its profile's version `to`, or lets it follow
/// `main` again.
pub(super) async fn pin(
    calls: &Calls,
    view: &View,
    id: &RequestId,
    hire: AgentNumber,
    to: Option<VersionName>,
    reason: Option<String>,
) -> Result<Result<String, Refusal>> {
    let name = view.name(hire);
    let act = PinHire { hire, to, reason };
    Ok(match ask(calls, id, act).await? {
        PinAnswer::Pinned { version, .. } => {
            Ok(words::pinned(&name, &version.0))
        }
        PinAnswer::FollowsMain => Ok(words::follows_main(&name)),
        PinAnswer::Refused {
            why:
                PinRefusal::Missing(Missing::NoVersion {
                    profile,
                    version,
                }),
        } => match label(calls, profile).await? {
            Ok(label) => Err(Refusal::NoVersion {
                name: label,
                version: version.0,
            }),
            Err(refusal) => Err(refusal),
        },
        PinAnswer::Refused { why } => Err(refused(why)),
    })
}

/// The label of `profile`, refused when the realm keeps none.
async fn label(
    calls: &Calls,
    profile: ProfileId,
) -> Result<Result<String, Refusal>> {
    let profile = protocol::ProfileId(u64::from(profile.0));
    Ok(answers::profile(calls, profile)
        .await?
        .map(|(_, label)| label))
}

/// The commit `which` names among a profile's versions and `main`:
/// `main`, a version, or a commit's whole id.
fn commit_of(list: &VersionList, which: &str) -> Option<CommitId> {
    if which == hamlet_git::MAIN {
        return list.commits.first().map(|tip| tip.commit);
    }
    list.versions
        .iter()
        .find(|seen| seen.version.0 == which)
        .map(|seen| seen.commit)
        .or_else(|| Sha1::from_hex(which).map(CommitId))
}

/// The commit `/revert` undoes as the profile takes it and as a chief
/// reads it; a name of no version is given to the profile as a
/// branch's.
fn revision(list: &VersionList, which: &str) -> (Revision, String) {
    match commit_of(list, which) {
        Some(commit) => {
            let short = words::short(&commit);
            (Revision::Commit { commit }, short)
        }
        None => (
            Revision::Version {
                version: VersionName(which.into()),
            },
            which.into(),
        ),
    }
}
