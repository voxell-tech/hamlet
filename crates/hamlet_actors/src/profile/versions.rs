//! A chief's changes to the profile's history and `main`: a tag, a
//! revert, an edit, a purge and the erase, and the reads of its
//! versions, its papers and its files.

use anyhow::{Context, Result};
use hamlet_git::{Change, Joined, MAIN};
use hamlet_messages::address::Agent;
use hamlet_messages::profile::{
    BadCommit, BadPath, By as Who, CommitId, CommitSeen, DreamId,
    DreamOpen, Edit, EditRefusal, Erase, MOST_COMMITS_SEEN,
    MOST_READ, Missing, Paper, PaperList, Papers, Path, Protected,
    Purge, PurgeAnswer, Read, ReadAnswer, Removal, RemovePath,
    Revert, RevertAnswer, RevertRefusal, Revision, Tag, TagAnswer,
    TagRefusal, TooLong, VersionList, VersionName, VersionSeen,
    Versions,
};
use hamlet_messages::realm::{Done, ProfileId};
use hamlet_messages::workspace::AgentNumber;
use hamlet_os::fs::TreePath;

use super::{
    CHARACTER, CHARACTER_MOST, Profile, commit_id, dreams, files,
    git_commit, hires, merges,
};
use crate::library;
use crate::repo::git::{By, Reads};
use crate::repo::{
    Before, Call, Checked, Ended, Look, Reply, Takes, parts,
};

const MEMORY: &str = "memory/";

const SKILLS: &str = "skills/";

fn is_version_name(version: &str) -> bool {
    (1..=32).contains(&version.len())
        && version.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b == b'.' || b == b'-'
        })
        && version.as_bytes()[0].is_ascii_alphanumeric()
        && !version.contains("..")
        && !version.ends_with('.')
        && !version.ends_with(".lock")
        && version != "HEAD"
}

impl Takes<Tag> for Profile {
    async fn take(call: Call<Self, Tag>) -> Result<Reply<Tag>> {
        if !is_version_name(&call.version.0) {
            let version = call.version.clone();
            return call
                .refuse(TagRefusal::BadName { version })
                .await;
        }
        let own = call.owner().own.0;
        call.once(move |unit| tag(unit, own)).await
    }
}

fn tag(
    unit: Before<'_, Profile, Tag>,
    own: ProfileId,
) -> Result<Ended<Tag>> {
    let tag = unit.message().clone();
    let by = By::from(tag.by);
    let (at, seen, taken) = {
        let git = unit.git()?;
        let at =
            named(git, tag.at.as_ref().unwrap_or(&Revision::Main))?;
        let seen = git
            .versions()?
            .into_iter()
            .find(|v| v.name == tag.version.0);
        // A branch's name or a commit's id is taken too.
        let taken = git.resolve(&tag.version.0)?.is_some();
        (at, seen, taken)
    };
    let at = match at {
        Ok(at) => at,
        // A taken name is refused before what `at` names.
        Err(_) if taken => {
            return unit.refuse(TagRefusal::Taken {
                version: tag.version,
            });
        }
        Err(unnamed) => {
            let why = match unnamed {
                Unnamed::Version(version) => TagRefusal::NoVersion {
                    profile: own,
                    version,
                },
                Unnamed::Commit(commit) => TagRefusal::NoSuchCommit {
                    profile: own,
                    commit,
                },
            };
            return unit.refuse(why);
        }
    };
    let tagged_by_a_finished_run = unit.answered()?.into_iter().any(|answer| {
        matches!(answer, TagAnswer::Tagged { version, .. } if version == tag.version)
    });
    let left_by_a_cut_off_run = |seen: &hamlet_git::Version| {
        !tagged_by_a_finished_run
            && seen.note == tag.note
            && seen.by == by.name()
            && seen.commit == at
    };
    match seen {
        Some(seen) if left_by_a_cut_off_run(&seen) => {
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
    let git = look.git()?;
    let main = git.tip(MAIN)?.context("a profile has a main")?;
    let commits = git
        .history(main, MOST_COMMITS_SEEN)?
        .into_iter()
        .map(|commit| {
            Ok(CommitSeen {
                commit: commit_id(commit.id)?,
                message: commit
                    .message
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                by: commit.author,
            })
        })
        .collect::<Result<_>>()?;
    let versions = git
        .versions()?
        .into_iter()
        .map(|v| {
            Ok(VersionSeen {
                version: VersionName(v.name),
                commit: commit_id(v.commit)?,
                note: v.note,
                by: v.by,
                at: v.at,
            })
        })
        .collect::<Result<_>>()?;
    Ok(VersionList { versions, commits })
}

impl Takes<Revert> for Profile {
    async fn take(call: Call<Self, Revert>) -> Result<Reply<Revert>> {
        let own = call.owner().own.0;
        call.once(move |unit| revert(unit, own)).await
    }
}

/// Undoes a commit of `main` with one commit on `main`, the tip
/// winning clashes.
fn revert(
    unit: Before<'_, Profile, Revert>,
    own: ProfileId,
) -> Result<Ended<Revert>> {
    let Revert {
        what, reason, by, ..
    } = unit.message().clone();
    let commit = match undone(unit.git()?, &what, own)? {
        Ok(commit) => commit,
        Err(why) => return unit.refuse(why),
    };
    let reason = reason.unwrap_or_default();
    let message = match by {
        Who::Chief { .. } => {
            format!("Reverted {}: {reason}", &commit.to_string()[..8])
        }
        Who::Agent { .. } => format!("Reverted: {reason}"),
    };
    let writer = unit.write();
    let merged =
        writer.writes()?.revert(MAIN, commit, &message, by.into())?;
    // A repeat past a kill finds its own revert there.
    if merged.joined == Joined::UpToDate {
        return writer.kept(RevertAnswer::Unchanged);
    }
    writer.kept(RevertAnswer::Reverted {
        commit: commit_id(merged.commit)?,
        clashes: merged
            .clashes
            .into_iter()
            .map(|path| Path { path })
            .collect(),
    })
}

fn undone(
    git: &Reads,
    what: &Revision,
    profile: ProfileId,
) -> Result<Result<hamlet_git::CommitId, RevertRefusal>> {
    let commit = match commit_of(git, what, profile)? {
        Ok(commit) => commit,
        Err(why) => return Ok(Err(why)),
    };
    if !git.holds(MAIN, commit)? {
        let commit = commit_id(commit)?;
        return Ok(Err(BadCommit::NotOnMain { commit }.into()));
    }
    Ok(Ok(commit))
}

/// A name or a commit the profile does not have.
enum Unnamed {
    Version(VersionName),
    Commit(CommitId),
}

/// The commit `what` names. A name is a branch's, then a version's,
/// then a full commit id, as today's `resolve` takes it
/// (`crates/hamlet_git/src/lib.rs:270-286`).
fn named(
    git: &Reads,
    what: &Revision,
) -> Result<Result<hamlet_git::CommitId, Unnamed>> {
    Ok(match what {
        Revision::Main => {
            Ok(git.tip(MAIN)?.context("a profile has a main")?)
        }
        Revision::Version { version } => git
            .resolve(&version.0)?
            .ok_or_else(|| Unnamed::Version(version.clone())),
        Revision::Commit { commit } => {
            let at = git_commit(*commit)?;
            if git.has(at)? {
                Ok(at)
            } else {
                Err(Unnamed::Commit(*commit))
            }
        }
    })
}

/// The commit `what` names, or the refusal of a version or a commit
/// the profile does not have.
fn commit_of<W: From<Missing> + From<BadCommit>>(
    git: &Reads,
    what: &Revision,
    profile: ProfileId,
) -> Result<Result<hamlet_git::CommitId, W>> {
    Ok(named(git, what)?.map_err(|unnamed| match unnamed {
        Unnamed::Version(version) => {
            Missing::NoVersion { profile, version }.into()
        }
        Unnamed::Commit(commit) => {
            BadCommit::NoCommit { profile, commit }.into()
        }
    }))
}

impl Takes<Read> for Profile {
    async fn take(call: Call<Self, Read>) -> Result<Reply<Read>> {
        let own = call.owner().own.0;
        call.read(move |look| read(look, own)).await
    }
}

/// The paths of the files at the revision asked, `main` when none
/// is, or the text of the file asked there.
fn read(
    look: &Look<'_, Profile, Read>,
    own: ProfileId,
) -> Result<ReadAnswer> {
    let Read { at, path } = look.message();
    let git = look.git()?;
    let at = match at {
        Some(what) => match commit_of(git, what, own)? {
            Ok(at) => at,
            Err(why) => return Ok(ReadAnswer::Refused { why }),
        },
        None => git.tip(MAIN)?.context("a profile has a main")?,
    };
    let Some(path) = path else {
        let paths = git
            .entries(at)?
            .into_iter()
            .map(|entry| Path { path: entry.path })
            .collect();
        return Ok(ReadAnswer::Listed { paths });
    };
    let Some(bytes) = git.read(at, path)? else {
        let path = path.clone();
        let why = BadPath::NotAPath { path }.into();
        return Ok(ReadAnswer::Refused { why });
    };
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    let most = MOST_READ as usize;
    let cut = text.len() > most;
    text.truncate(text.floor_char_boundary(most));
    Ok(ReadAnswer::Text { text, cut })
}

impl Takes<Edit> for Profile {
    async fn take(call: Call<Self, Edit>) -> Result<Reply<Edit>> {
        if let Err(why) = editable(&call) {
            return call.refuse(why).await;
        }
        call.once(edit).await
    }
}

fn editable(edit: &Edit) -> Result<(), EditRefusal> {
    let path = &edit.path;
    if !parts::is_file_path(path) {
        let path = path.clone();
        return Err(BadPath::NotAPath { path }.into());
    }
    let chief = matches!(edit.by, Who::Chief { .. });
    if chief && path != CHARACTER && !path.starts_with(MEMORY) {
        let path = path.clone();
        return Err(Protected::Withheld { path }.into());
    }
    let most = CHARACTER_MOST;
    if path == CHARACTER && edit.text.chars().count() > most as usize
    {
        return Err(TooLong::CharacterTooLong { most }.into());
    }
    Ok(())
}

fn edit(unit: Before<'_, Profile, Edit>) -> Result<Ended<Edit>> {
    let Edit {
        path,
        text,
        message,
        by,
        ..
    } = unit.message().clone();
    let main =
        unit.git()?.tip(MAIN)?.context("a profile has a main")?;
    let (words, removes) = match by {
        Who::Chief { .. } if path == CHARACTER => {
            ("Edited the character", false)
        }
        Who::Chief { .. } => ("Edited the memory", text.is_empty()),
        Who::Agent { .. } => ("Edited by the Hand", text.is_empty()),
    };
    let message = message
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| words.to_owned());
    let change = if removes {
        Change::Remove { path }
    } else {
        Change::Write {
            path,
            bytes: text.into_bytes(),
        }
    };
    let writer = unit.write();
    writer.writes()?.commit(
        MAIN,
        Some(main),
        &[change],
        &message,
        by.into(),
    )?;
    writer.kept(Done::Done)
}

impl Takes<Purge> for Profile {
    async fn take(call: Call<Self, Purge>) -> Result<Reply<Purge>> {
        if TreePath::parse(&call.path).is_err() {
            let path = call.path.clone();
            return call
                .refuse(BadPath::NotAPath { path }.into())
                .await;
        }
        // The id is checked before any agent is asked: a repeat asks
        // none.
        let fresh = match call.fresh().await? {
            Checked::First(fresh) => fresh,
            Checked::Repeat(reply) => return Ok(reply),
        };
        let (dreaming, agents) = fresh
            .look(|look| {
                let agents = hires::every(look)?
                    .into_iter()
                    .map(|hire| Agent(hire.agent))
                    .collect::<Vec<_>>();
                Ok((dreams::open(look)?, agents))
            })
            .await?;
        let mut still_hold = Vec::new();
        if dreaming.is_none() {
            let asked = agents
                .into_iter()
                .map(|agent| {
                    let path = fresh.path.clone();
                    (agent, RemovePath { path })
                })
                .collect::<Vec<_>>();
            let answers = fresh.ask_all(&asked).await;
            for ((agent, _), removal) in asked.iter().zip(answers) {
                if !matches!(removal, Ok(Removal::Removed)) {
                    still_hold.push(agent.0);
                }
            }
        }
        fresh
            .once(move |unit| purge(unit, dreaming, still_hold))
            .await
    }
}

/// Takes the path out of every branch and version, then gives each
/// pin its rewritten commit and each agent its branch's new tip.
/// `dreaming` is the dream open when the agents would have been
/// asked: none was.
fn purge(
    unit: Before<'_, Profile, Purge>,
    dreaming: Option<DreamId>,
    still_hold: Vec<AgentNumber>,
) -> Result<Ended<Purge>> {
    if let Some(dream) = dreaming.or(dreams::open(&unit)?) {
        return unit.refuse(DreamOpen::Dreaming { dream }.into());
    }
    let path = unit.message().path.clone();
    let mut writer = unit.write();
    let branches = writer.purge(&path)?;
    merges::purged(&writer, branches.iter().map(|(_, _, tip)| *tip))?;
    files::drop_saves(&writer, None)?;
    // A repeat past a kill rewrites nothing, and still tells every
    // agent and keeps every pin.
    hires::repin(&mut writer)?;
    writer.kept(PurgeAnswer::Purged {
        branches: u32::try_from(branches.len())?,
        still_hold,
    })
}

impl Takes<Erase> for Profile {
    async fn take(call: Call<Self, Erase>) -> Result<Reply<Erase>> {
        call.once(erase).await
    }
}

fn erase(unit: Before<'_, Profile, Erase>) -> Result<Ended<Erase>> {
    if let Some(dream) = dreams::open(&unit)? {
        return unit.refuse(DreamOpen::Dreaming { dream });
    }
    unit.write().erase(Done::Done)
}

impl Takes<Papers> for Profile {
    async fn take(call: Call<Self, Papers>) -> Result<Reply<Papers>> {
        call.read(papers).await
    }
}

/// The character, the memory files and the skills at `main`, each
/// file with the `description` of its front matter.
fn papers(look: &Look<'_, Profile, Papers>) -> Result<PaperList> {
    let git = look.git()?;
    let main = git.tip(MAIN)?.context("a profile has a main")?;
    let mut list = PaperList {
        character: String::new(),
        memory: Vec::new(),
        skills: Vec::new(),
    };
    for entry in git.entries(main)? {
        let is_skill = entry
            .path
            .strip_prefix(SKILLS)
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(_, file)| file == "SKILL.md");
        let kept = if entry.path == CHARACTER {
            None
        } else if entry.path.starts_with(MEMORY) {
            Some(&mut list.memory)
        } else if is_skill {
            Some(&mut list.skills)
        } else {
            continue;
        };
        let bytes = git.blob(entry.blob)?;
        let text = String::from_utf8_lossy(&bytes);
        match kept {
            None => list.character = text.into_owned(),
            Some(kept) => kept.push(Paper {
                description: library::field(&text, "description")
                    .unwrap_or_default()
                    .to_owned(),
                path: entry.path,
            }),
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use hamlet_git::{Author, Repository};
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
            "HEAD",
            &"v".repeat(33),
        ] {
            assert!(!is_version_name(bad), "{bad:?}");
        }
        for good in ["v1", "1.0", "v1.0-rc2", &"v".repeat(32)] {
            assert!(is_version_name(good), "{good:?}");
        }
    }

    #[test]
    fn a_unit_that_wrote_cannot_answer_refused() {
        let dir = Scratch::new("profile-write-then-refuse");
        let mut profile = Hostless::profile(&dir, "Careful.");
        let realm = Caller::Actor(Realm.address().unwrap());
        let asked = |version: &str| Tag {
            id: RequestId::new(),
            version: VersionName(version.into()),
            at: None,
            note: String::new(),
            by: Who::Chief { chief: 3 },
        };
        let first = asked("v1");
        let wrote = profile.once::<Profile, Tag, _>(
            realm.clone(),
            first.clone(),
            |unit| {
                let main = unit.git()?.tip(MAIN)?.unwrap();
                let writer = unit.write();
                writer.writes()?.tag("v1", main, "", By::Chief(3))?;
                writer.kept(TagAnswer::Refused {
                    why: TagRefusal::Taken {
                        version: VersionName("v1".into()),
                    },
                })
            },
        );
        assert!(wrote.is_err());

        let again = Tag {
            version: VersionName("v2".into()),
            ..first
        };
        let (answer, _) = profile
            .once::<Profile, Tag, _>(realm, again, |unit| {
                tag(unit, ProfileId(1))
            })
            .unwrap();
        assert!(
            matches!(answer, TagAnswer::Tagged { .. }),
            "{answer:?}"
        );
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
                asked(Some(Revision::Commit {
                    commit: commit_id(unknown).unwrap(),
                })),
                |unit| tag(unit, ProfileId(1)),
            )
            .unwrap();
        let why = TagRefusal::NoSuchCommit {
            profile: ProfileId(1),
            commit: commit_id(unknown).unwrap(),
        };
        assert_eq!(
            why.to_string(),
            format!("`scholar#1` has no version `{unknown}`")
        );
        assert_eq!(refused, TagAnswer::Refused { why });

        let at = commit_id(first).unwrap();
        let (tagged, _) = profile
            .once::<Profile, Tag, _>(
                realm.clone(),
                asked(Some(Revision::Commit { commit: at })),
                |unit| tag(unit, ProfileId(1)),
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
        let made = listed.versions.first().map_or(0, |seen| seen.at);
        let seen = VersionSeen {
            version,
            commit: at,
            note: "the first".into(),
            by: "chief:3".into(),
            at: made,
        };
        assert_eq!(listed.versions, [seen]);
    }

    #[test]
    fn a_tag_at_a_branchs_name_names_its_tip() {
        let dir = Scratch::new("profile-tag-branch");
        let mut profile = Hostless::profile(&dir, "Careful.");
        let realm = Caller::Actor(Realm.address().unwrap());
        let repository =
            Repository::open(&dir.join("scholars/1.git")).unwrap();
        let by = Author {
            name: "agent:2",
            at: 5000,
        };
        let write = Change::Write {
            path: "memory/a.md".into(),
            bytes: b"a".to_vec(),
        };
        let tip =
            repository.commit("hires/2", &[write], "a", by).unwrap();
        let asked = |name: &str| Tag {
            id: RequestId::new(),
            version: VersionName("v1".into()),
            at: Some(Revision::Version {
                version: VersionName(name.into()),
            }),
            note: String::new(),
            by: Who::Chief { chief: 3 },
        };

        let (refused, _) = profile
            .once::<Profile, Tag, _>(
                realm.clone(),
                asked("hires/9"),
                |unit| tag(unit, ProfileId(1)),
            )
            .unwrap();
        let why = TagRefusal::NoVersion {
            profile: ProfileId(1),
            version: VersionName("hires/9".into()),
        };
        assert_eq!(
            why.to_string(),
            "`scholar#1` has no version `hires/9`"
        );
        assert_eq!(refused, TagAnswer::Refused { why });

        let (tagged, _) = profile
            .once::<Profile, Tag, _>(
                realm.clone(),
                asked("hires/2"),
                |unit| tag(unit, ProfileId(1)),
            )
            .unwrap();
        assert_eq!(
            tagged,
            TagAnswer::Tagged {
                version: VersionName("v1".into()),
                commit: commit_id(tip).unwrap(),
            }
        );

        let (taken, _) = profile
            .once::<Profile, Tag, _>(
                realm,
                asked("hires/9"),
                |unit| tag(unit, ProfileId(1)),
            )
            .unwrap();
        let why = TagRefusal::Taken {
            version: VersionName("v1".into()),
        };
        assert_eq!(taken, TagAnswer::Refused { why });
    }

    #[test]
    fn an_erase_cut_off_before_its_marker_makes_nothing_again() {
        let dir = Scratch::new("profile-erase-cut");
        let mut profile = Hostless::profile(&dir, "Careful.");
        let realm = Caller::Actor(Realm.address().unwrap());
        let asked = Erase {
            id: RequestId::new(),
        };
        let (done, _) = profile
            .once::<Profile, Erase, _>(realm, asked, erase)
            .unwrap();
        assert_eq!(done, Done::Done);
        let repository = dir.join("scholars/1.git");
        assert!(!repository.exists());
        // Left by a kill part-way through the removal.
        let erased = dir.join("scholars/1.git.erased");
        std::fs::create_dir(erased.join("objects")).unwrap();
        std::fs::write(erased.join("HEAD"), "ref: refs/heads/main")
            .unwrap();

        // A new store: the erase's marker was never kept.
        let again = Hostless::start_profile(&dir, "Careful.");
        assert!(again.is_err());
        assert!(!repository.exists());
        assert_eq!(std::fs::read_dir(&erased).unwrap().count(), 0);
    }

    #[test]
    fn a_purge_and_an_erase_are_refused_while_a_dream_is_open() {
        let dir = Scratch::new("profile-dreaming");
        let mut profile = Hostless::profile(&dir, "Careful.");
        let realm = Caller::Actor(Realm.address().unwrap());
        let refused = |dream| PurgeAnswer::Refused {
            why: DreamOpen::Dreaming {
                dream: DreamId(dream),
            }
            .into(),
        };
        let purge_in = |profile: &mut Hostless, seen| {
            let asked = Purge {
                id: RequestId::new(),
                path: "CHARACTER.md".into(),
                by: Who::Chief { chief: 3 },
            };
            profile
                .once::<Profile, Purge, _>(
                    realm.clone(),
                    asked,
                    |unit| purge(unit, seen, Vec::new()),
                )
                .unwrap()
        };
        // A dream that ended after the agents were not asked.
        let (purged, told) = purge_in(&mut profile, Some(DreamId(2)));
        assert_eq!((purged, told.len()), (refused(2), 0));

        profile
            .store()
            .execute(
                "INSERT INTO dream (id, main) VALUES (4, 'unused')",
                [],
            )
            .unwrap();
        let (purged, told) = purge_in(&mut profile, None);
        assert_eq!((purged, told.len()), (refused(4), 0));
        let asked = Erase {
            id: RequestId::new(),
        };
        let (erased, _) = profile
            .once::<Profile, Erase, _>(realm.clone(), asked, erase)
            .unwrap();
        let dreaming = DreamOpen::Dreaming { dream: DreamId(4) };
        assert_eq!(erased, Done::Refused { why: dreaming });
        let papers = profile
            .read::<Profile, Papers, _>(realm, Papers, papers)
            .unwrap();
        assert_eq!(papers.character, "Careful.");
    }
}
