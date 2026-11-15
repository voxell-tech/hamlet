//! A chief's changes to the profile's history and `main`: a tag, a
//! revert, an edit, a purge and the erase, and the reads of its
//! versions and its papers.

use anyhow::{Context, Result};
use hamlet_git::{Change, Joined, MAIN};
use hamlet_messages::address::Agent;
use hamlet_messages::profile::{
    BadCommit, BadPath, By as Who, CommitSeen, DreamId, DreamOpen,
    Edit, EditRefusal, Erase, MOST_COMMITS_SEEN, Missing, Paper,
    PaperList, Papers, Path, Protected, Purge, PurgeAnswer, Removal,
    RemovePath, Revert, RevertAnswer, RevertRefusal, Revision, Tag,
    TagAnswer, TagRefusal, TooLong, VersionList, VersionName,
    VersionSeen, Versions,
};
use hamlet_messages::realm::{Done, ProfileId};
use hamlet_messages::workspace::AgentNumber;
use hamlet_os::fs::TreePath;

use super::{
    CHARACTER, MOST_CHARACTER, Profile, commit_id, dreams,
    git_commit, hires,
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
        // Its own tag, from a run a kill cut off before the marker:
        // one at another commit, `main` moved since, is another's.
        Some(seen)
            if seen.note == tag.note
                && seen.by == by.name()
                && seen.commit == at =>
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
/// winning clashes. `own` is the profile's number.
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
    let message = format!(
        "Reverted {}: {}",
        &commit.to_string()[..8],
        reason.unwrap_or_default()
    );
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
    let commit = match what {
        Revision::Version { version } => {
            match hires::version_at(git, version)? {
                Some(commit) => commit,
                None => {
                    let version = version.clone();
                    let why = Missing::NoVersion { profile, version };
                    return Ok(Err(why.into()));
                }
            }
        }
        Revision::Commit { commit } => {
            let at = git_commit(*commit)?;
            if !git.has(at)? {
                let commit = *commit;
                return Ok(
                    Err(BadCommit::NoCommit { commit }.into()),
                );
            }
            at
        }
    };
    if !git.holds(MAIN, commit)? {
        let commit = commit_id(commit)?;
        return Ok(Err(BadCommit::NotOnMain { commit }.into()));
    }
    Ok(Ok(commit))
}

impl Takes<Edit> for Profile {
    async fn take(call: Call<Self, Edit>) -> Result<Reply<Edit>> {
        if let Err(why) = editable(&call) {
            return call.refuse(why).await;
        }
        call.once(edit).await
    }
}

/// A chief edits the character and memory; an agent, the Hand, any
/// file.
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
    let most = MOST_CHARACTER;
    if path == CHARACTER && edit.text.chars().count() > most as usize
    {
        return Err(TooLong::CharacterTooLong { most }.into());
    }
    Ok(())
}

/// Writes the file, or removes it on empty text, but for a chief's
/// character, which is written empty.
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
    let message = message.unwrap_or_else(|| words.to_owned());
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
                    why: TagRefusal::NoSuchCommit,
                })
            },
        );
        assert!(wrote.is_err());

        let again = Tag {
            version: VersionName("v2".into()),
            ..first
        };
        let (answer, _) = profile
            .once::<Profile, Tag, _>(realm, again, tag)
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
