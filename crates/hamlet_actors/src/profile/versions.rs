//! The profile's versions: a tag of its history, and the list.

use anyhow::{Context, Result};
use hamlet_git::MAIN;
use hamlet_messages::profile::{
    Tag, TagAnswer, TagRefusal, VersionList, VersionName,
    VersionSeen, Versions,
};

use super::{Profile, commit_id, git_commit};
use crate::repo::git::By;
use crate::repo::{Before, Call, Ended, Look, Reply, Takes};

/// Whether `version` is a version's name: 1 to 32 letters, digits,
/// dots and hyphens, a letter or a digit first, with no `..`, no dot
/// last and no `.lock` last.
fn is_version_name(version: &str) -> bool {
    (1..=32).contains(&version.len())
        && version.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b == b'.' || b == b'-'
        })
        && version.as_bytes()[0].is_ascii_alphanumeric()
        && !version.contains("..")
        && !version.ends_with('.')
        && !version.ends_with(".lock")
}

impl Takes<Tag> for Profile {
    async fn take(call: Call<Self, Tag>) -> Result<Reply<Tag>> {
        if !is_version_name(&call.version.0) {
            let version = call.version.clone();
            return Ok(call.refuse(TagRefusal::BadName { version }));
        }
        call.once(tag).await
    }
}

fn tag(unit: Before<'_, Profile, Tag>) -> Result<Ended<Tag>> {
    let tag = unit.message().clone();
    let by = By::from(tag.by);
    let (at, seen, taken) = {
        let git = unit.git()?;
        let at = match tag.at {
            Some(at) => Some(git_commit(at)?)
                .filter(|at| git.has(*at).unwrap_or(false)),
            None => {
                Some(git.tip(MAIN)?.context("a profile has a main")?)
            }
        };
        let seen = git
            .versions()?
            .into_iter()
            .find(|v| v.name == tag.version.0);
        // A branch's name or a commit's id is taken too.
        let taken = git.resolve(&tag.version.0)?.is_some();
        (at, seen, taken)
    };
    let Some(at) = at else {
        return unit.refuse(TagRefusal::NoSuchCommit);
    };
    match seen {
        // Its own tag, from a run a kill cut off before the marker.
        // With no `at`, `main` may have moved since.
        Some(seen)
            if seen.note == tag.note
                && seen.by == by.name()
                && tag.at.is_none_or(|_| seen.commit == at) =>
        {
            let tagged = TagAnswer::Tagged {
                version: tag.version,
                commit: commit_id(seen.commit)?,
            };
            return unit.kept(tagged);
        }
        Some(_) => {
            return unit.refuse(TagRefusal::Taken {
                version: tag.version,
            });
        }
        None if taken => {
            return unit.refuse(TagRefusal::Taken {
                version: tag.version,
            });
        }
        None => {}
    }
    let tagged = TagAnswer::Tagged {
        version: tag.version.clone(),
        commit: commit_id(at)?,
    };
    let writer = unit.write();
    writer.writes()?.tag(&tag.version.0, at, &tag.note, by)?;
    writer.kept(tagged)
}

impl Takes<Versions> for Profile {
    async fn take(
        call: Call<Self, Versions>,
    ) -> Result<Reply<Versions>> {
        call.read(versions).await
    }
}

fn versions(
    look: &Look<'_, Profile, Versions>,
) -> Result<VersionList> {
    let versions = look
        .git()?
        .versions()?
        .into_iter()
        .map(|v| {
            Ok(VersionSeen {
                version: VersionName(v.name),
                commit: commit_id(v.commit)?,
                note: v.note,
                by: v.by,
            })
        })
        .collect::<Result<_>>()?;
    Ok(VersionList { versions })
}

#[cfg(test)]
mod tests {
    use hamlet_git::{Author, Change, Repository};
    use hamlet_host::Caller;
    use hamlet_messages::address::{Party, Realm};
    use hamlet_messages::profile::By as Who;
    use hamlet_protocol::client::RequestId;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::repo::testing::Hostless;

    #[test]
    fn a_version_name_is_one_git_takes() {
        for bad in [
            "",
            ".v1",
            "-v1",
            "v1.",
            "a..b",
            "x.lock",
            "a/b",
            "v 1",
            &"v".repeat(33),
        ] {
            assert!(!is_version_name(bad), "{bad:?}");
        }
        for good in ["v1", "1.0", "v1.0-rc2", &"v".repeat(32)] {
            assert!(is_version_name(good), "{good:?}");
        }
    }

    #[test]
    fn a_tag_names_the_commit_it_is_given_and_only_one_the_profile_has()
     {
        let dir = Scratch::new("profile-tag-at");
        let mut profile = Hostless::profile(&dir, "Careful.");
        let realm = Caller::Actor(Realm.address().unwrap());
        let repository =
            Repository::open(&dir.join("scholars/1.git")).unwrap();
        let first = repository.tip(MAIN).unwrap().unwrap();
        let by = Author {
            name: "chief:3",
            at: 5000,
        };
        let write = Change::Write {
            path: "memory/a.md".into(),
            bytes: b"a".to_vec(),
        };
        repository.commit(MAIN, &[write], "a", by).unwrap();
        let asked = |at| Tag {
            id: RequestId::new(),
            version: VersionName("v1".into()),
            at,
            note: "the first".into(),
            by: Who::Chief { chief: 3 },
        };

        let elsewhere =
            Repository::create(&dir.join("elsewhere.git")).unwrap();
        let unknown =
            elsewhere.commit(MAIN, &[], "other", by).unwrap();
        let (refused, _) = profile
            .once::<Profile, Tag, _>(
                realm.clone(),
                asked(Some(commit_id(unknown).unwrap())),
                tag,
            )
            .unwrap();
        assert_eq!(
            refused,
            TagAnswer::Refused {
                why: TagRefusal::NoSuchCommit
            }
        );

        let at = commit_id(first).unwrap();
        let (tagged, _) = profile
            .once::<Profile, Tag, _>(
                realm.clone(),
                asked(Some(at)),
                tag,
            )
            .unwrap();
        let version = VersionName("v1".into());
        assert_eq!(
            tagged,
            TagAnswer::Tagged {
                version: version.clone(),
                commit: at
            }
        );
        let listed = profile
            .read::<Profile, Versions, _>(realm, Versions, versions)
            .unwrap();
        let seen = VersionSeen {
            version,
            commit: at,
            note: "the first".into(),
            by: "chief:3".into(),
        };
        assert_eq!(listed.versions, [seen]);
    }
}
