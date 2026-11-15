//! A dream's two ends, and the `dream` table, one row per dream of
//! this profile or of one of its hires. Between the ends the dream's
//! agent writes the folders through the workspace; no profile code
//! runs then.

use std::path::Path;

use anyhow::{Context, Result, bail};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_git::{MAIN, Wins};
use hamlet_messages::Message;
use hamlet_messages::profile::{
    DreamEnd, DreamId, DreamOf, EndDream, PrepareDream, Prepared,
};
use hamlet_messages::workspace::AgentNumber;

use super::hires::{self, Hire};
use super::{CHARACTER, Profile, commit_id};
use crate::repo::git::By;
use crate::repo::{
    Admits, Before, Call, Ended, Look, Reply, Takes, Writer, store,
};

/// The folder of the dreams' checkouts, under the realm folder.
const DREAMS: &str = "dreams";

/// The checkout of `main`, in a dream's folder.
const MAIN_SIDE: &str = "main";

/// The checkout of the hire's branch, in a dream's folder.
const HIRE_SIDE: &str = "hire";

/// The token of the `dream` table.
struct Table(());

impl store::Table for Table {
    const NAME: &'static str = "dream";
}

/// A dream as its row holds it.
struct Dream {
    hire: Option<AgentNumber>,
    /// `main` when the checkouts were written.
    main: hamlet_git::CommitId,
    /// The hire's branch when the checkouts were written.
    hire_tip: Option<hamlet_git::CommitId>,
    /// `None` while the dream runs.
    outcome: Option<DreamEnd>,
}

/// The paths a checkout leaves out: a dream never changes the
/// character.
fn skip(path: &str) -> bool {
    path == CHARACTER
}

/// The dream's folder under the realm folder, or one of its
/// checkouts.
fn folder(dream: DreamId, side: Option<&str>) -> String {
    match side {
        Some(side) => format!("{DREAMS}/{}/{side}", dream.0),
        None => format!("{DREAMS}/{}", dream.0),
    }
}

fn ready(dream: DreamId, of_a_hire: bool) -> Prepared {
    Prepared::Ready {
        main: folder(dream, Some(MAIN_SIDE)),
        hire: of_a_hire.then(|| folder(dream, Some(HIRE_SIDE))),
    }
}

/// The dream that has not ended, when there is one.
pub(super) fn open<M>(
    look: &Look<'_, Profile, M>,
) -> Result<Option<DreamId>>
where
    M: Message,
    Profile: Admits<M::From>,
{
    let rows = look.rows(Table(())).select(
        "SELECT id FROM dream WHERE outcome IS NULL ORDER BY id \
         LIMIT 1",
        Vec::new(),
    )?;
    match rows.first().map(Vec::as_slice) {
        None => Ok(None),
        Some([ColumnValue::Integer(id)]) => {
            Ok(Some(DreamId(u32::try_from(*id)?)))
        }
        Some(row) => bail!("a dream row of another shape: {row:?}"),
    }
}

fn row<M>(
    look: &Look<'_, Profile, M>,
    dream: DreamId,
) -> Result<Option<Dream>>
where
    M: Message,
    Profile: Admits<M::From>,
{
    let rows = look.rows(Table(())).select(
        "SELECT hire, main, hire_tip, outcome FROM dream WHERE id = \
         ?1",
        vec![BindParam::Integer(i64::from(dream.0))],
    )?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let commit = |text: &str| {
        text.parse::<hamlet_git::CommitId>()
            .map_err(anyhow::Error::msg)
    };
    match row.as_slice() {
        [hire, ColumnValue::Text(main), hire_tip, outcome] => {
            Ok(Some(Dream {
                hire: match hire {
                    ColumnValue::Integer(agent) => {
                        Some(AgentNumber(u32::try_from(*agent)?))
                    }
                    _ => None,
                },
                main: commit(main)?,
                hire_tip: match hire_tip {
                    ColumnValue::Text(tip) => Some(commit(tip)?),
                    _ => None,
                },
                outcome: match outcome {
                    ColumnValue::Blob(bytes) => {
                        Some(ciborium::from_reader(bytes.as_slice())?)
                    }
                    _ => None,
                },
            }))
        }
        _ => bail!("a dream row of another shape: {row:?}"),
    }
}

/// The hire of `agent`, which a dream of it names.
fn hire_of<M>(
    look: &Look<'_, Profile, M>,
    agent: AgentNumber,
) -> Result<Hire>
where
    M: Message,
    Profile: Admits<M::From>,
{
    hires::find(look, agent)?.with_context(|| {
        format!("hire#{} is not a hire of this profile", agent.0)
    })
}

fn cbor(outcome: &DreamEnd) -> Result<BindParam> {
    let mut bytes = Vec::new();
    ciborium::into_writer(outcome, &mut bytes)?;
    Ok(BindParam::Blob(bytes))
}

impl Takes<PrepareDream> for Profile {
    async fn take(
        call: Call<Self, PrepareDream>,
    ) -> Result<Reply<PrepareDream>> {
        call.once(prepare).await
    }
}

/// Checks `main`, and the hire's branch for a hire's dream, out into
/// the dream's folders and keeps the dream's row. A repeat checks out
/// the commits its row holds again.
fn prepare(
    unit: Before<'_, Profile, PrepareDream>,
) -> Result<Ended<PrepareDream>> {
    let PrepareDream { dream, of } = unit.message().clone();
    match row(&unit, dream)? {
        Some(Dream {
            outcome: Some(_), ..
        }) => return unit.kept(Prepared::Nothing),
        Some(found) => {
            let mut writer = unit.write();
            check_out(
                &mut writer,
                dream,
                found.main,
                found.hire_tip,
            )?;
            return writer.kept(ready(dream, found.hire.is_some()));
        }
        None => {}
    }
    let main =
        unit.git()?.tip(MAIN)?.context("a profile has a main")?;
    let (hire, hire_tip) = match of {
        DreamOf::Profile => (None, None),
        DreamOf::Hire { agent } => {
            let hire = hire_of(&unit, agent)?;
            let tip = unit
                .git()?
                .tip(&hire.branch)?
                .context("a hire has a branch")?;
            (Some(agent), Some(tip))
        }
    };
    let nothing = match hire_tip {
        Some(tip) => unit.git()?.holds(MAIN, tip)?,
        None => false,
    };
    let mut writer = unit.write();
    let outcome = if nothing {
        cbor(&DreamEnd::Nothing)?
    } else {
        check_out(&mut writer, dream, main, hire_tip)?;
        BindParam::Null
    };
    writer.table(Table(())).execute(
        "INSERT INTO dream (id, hire, main, hire_tip, outcome) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        vec![
            BindParam::Integer(i64::from(dream.0)),
            hire.map_or(BindParam::Null, |agent| {
                BindParam::Integer(i64::from(agent.0))
            }),
            BindParam::Text(main.to_string()),
            hire_tip.map_or(BindParam::Null, |tip| {
                BindParam::Text(tip.to_string())
            }),
            outcome,
        ],
    )?;
    writer.kept(if nothing {
        Prepared::Nothing
    } else {
        ready(dream, hire.is_some())
    })
}

fn check_out(
    writer: &mut Writer<'_, Profile, PrepareDream>,
    dream: DreamId,
    main: hamlet_git::CommitId,
    hire_tip: Option<hamlet_git::CommitId>,
) -> Result<()> {
    let main_side = folder(dream, Some(MAIN_SIDE));
    writer.check_out(main, Path::new(&main_side), &skip)?;
    if let Some(tip) = hire_tip {
        let hire_side = folder(dream, Some(HIRE_SIDE));
        writer.check_out(tip, Path::new(&hire_side), &skip)?;
    }
    Ok(())
}

impl Takes<EndDream> for Profile {
    async fn take(
        call: Call<Self, EndDream>,
    ) -> Result<Reply<EndDream>> {
        call.once(end).await
    }
}

/// Commits what a done dream left in its folders, removes them and
/// keeps the outcome in the dream's row. A repeat answers the row's
/// outcome.
fn end(
    unit: Before<'_, Profile, EndDream>,
) -> Result<Ended<EndDream>> {
    let EndDream {
        dream,
        done,
        summary,
    } = unit.message().clone();
    let found = row(&unit, dream)?
        .with_context(|| format!("there is no dream {}", dream.0))?;
    if let Some(outcome) = found.outcome {
        return unit.kept(outcome);
    }
    let hire = match found.hire {
        Some(agent) => Some(hire_of(&unit, agent)?),
        None => None,
    };
    let mut writer = unit.write();
    let outcome = if done {
        dreamt(&mut writer, &found, hire.as_ref(), &summary)?
    } else {
        DreamEnd::Nothing
    };
    remove(&mut writer, dream)?;
    writer.table(Table(())).execute(
        "UPDATE dream SET outcome = ?1 WHERE id = ?2",
        vec![cbor(&outcome)?, BindParam::Integer(i64::from(dream.0))],
    )?;
    writer.kept(outcome)
}

/// Commits the `main` side onto `main` and the hire's side onto its
/// branch, then takes the new `main` into the branch and tells the
/// hire's agent.
fn dreamt(
    writer: &mut Writer<'_, Profile, EndDream>,
    found: &Dream,
    hire: Option<&Hire>,
    summary: &str,
) -> Result<DreamEnd> {
    let main =
        commit_side(writer, MAIN, found.main, MAIN_SIDE, summary)?;
    if let (Some(hire), Some(base)) = (hire, found.hire_tip) {
        commit_side(writer, &hire.branch, base, HIRE_SIDE, summary)?;
        hires::take_in_main(writer, hire)?;
        let tip = writer
            .git()?
            .tip(&hire.branch)?
            .context("a hire has a branch")?;
        // Told on a repeat too: the run a kill cut off told no one.
        hires::tell_moved(writer, hire, tip)?;
    }
    Ok(DreamEnd::Dreamt {
        main: main.map(commit_id).transpose()?,
    })
}

/// The dream's commit on `branch`: the one a run a kill cut off made,
/// else the changes of the checkout `side` since `base`, merged onto
/// the tip. Where both changed a file, the dream's side wins on
/// `main` and the hire's saves win on its branch. `None` when the
/// side changed nothing.
fn commit_side(
    writer: &Writer<'_, Profile, EndDream>,
    branch: &str,
    base: hamlet_git::CommitId,
    side: &str,
    summary: &str,
) -> Result<Option<hamlet_git::CommitId>> {
    let dream = writer.message().dream;
    let by = By::Dream(dream.0);
    let made = writer.git()?.made_by(branch, base, by)?;
    let under = folder(dream, Some(side));
    // A folder a run a kill cut off removed held nothing to commit:
    // it had made its commit first.
    if made.is_some() || !writer.home().join(&under).is_dir() {
        return Ok(made);
    }
    let changes = writer.changes(Path::new(&under), base, &skip)?;
    let wins = if branch == MAIN {
        Wins::From
    } else {
        Wins::Onto
    };
    let message = match summary {
        "" => "Dreamt".to_owned(),
        summary => format!("Dreamt: {summary}"),
    };
    writer.writes()?.commit_checkout(
        branch, base, &changes, &message, by, wins,
    )?;
    writer.git()?.made_by(branch, base, by)
}

/// Removes the dream's folder: first moved aside by one rename, so a
/// kill part-way through never leaves a checkout a repeat would read
/// as files the dream removed.
fn remove(
    writer: &mut Writer<'_, Profile, EndDream>,
    dream: DreamId,
) -> Result<()> {
    let at = folder(dream, None);
    let ended = format!("{at}.ended");
    let (at, ended_at) =
        (writer.home().join(&at), writer.home().join(&ended));
    if at.is_dir() {
        writer.remove_folder(Path::new(&ended))?;
        std::fs::rename(at, ended_at)?;
    }
    writer.remove_folder(Path::new(&ended))
}

#[cfg(test)]
mod tests {
    use hamlet_git::{Author, Change, Repository};
    use hamlet_host::Caller;
    use hamlet_messages::address::{Party, Realm};
    use hamlet_testing::Scratch;

    use super::*;
    use crate::repo::testing::Hostless;

    fn realm() -> Caller {
        Caller::Actor(Realm.address().unwrap())
    }

    fn write(path: &str, text: &str) -> Change {
        Change::Write {
            path: path.into(),
            bytes: text.into(),
        }
    }

    /// A profile with no host whose hire 7 saved `learned.md`, and
    /// dream 3 of that hire prepared.
    fn dreaming(dir: &Scratch) -> (Hostless, Repository) {
        let mut profile = Hostless::profile(dir, "Careful.");
        let repository =
            Repository::open(&dir.join("scholars/1.git")).unwrap();
        let first = repository.tip(MAIN).unwrap().unwrap();
        repository.branch("hires/7", first).unwrap();
        profile
            .store()
            .execute(
                "INSERT INTO hire (agent, branch, character, \
                 character_blob) VALUES (7, 'hires/7', 1, '')",
                [],
            )
            .unwrap();
        let by = Author {
            name: "agent:7",
            at: 5000,
        };
        let learned = write("learned.md", "learned");
        repository
            .commit("hires/7", &[learned], "saved", by)
            .unwrap();
        let asked = PrepareDream {
            dream: DreamId(3),
            of: DreamOf::Hire {
                agent: AgentNumber(7),
            },
        };
        profile
            .once::<Profile, _, _>(realm(), asked, prepare)
            .unwrap();
        (profile, repository)
    }

    fn end_3() -> EndDream {
        EndDream {
            dream: DreamId(3),
            done: true,
            summary: String::new(),
        }
    }

    /// Ends dream 3 as far as its folder's removal, then fails as a
    /// kill would, before the row's outcome is kept.
    fn end_cut_off(profile: &mut Hostless) {
        let cut =
            profile.once::<Profile, _, _>(realm(), end_3(), |unit| {
                end(unit)?;
                bail!("killed")
            });
        assert!(cut.is_err());
    }

    fn dreamt(repository: &Repository, branch: &str) -> usize {
        let tip = repository.tip(branch).unwrap().unwrap();
        let commits =
            repository.first_parents(tip, None, 10).unwrap();
        commits.iter().filter(|c| c.author == "dream:3").count()
    }

    #[test]
    fn an_end_cut_off_before_its_row_makes_no_second_commit() {
        let dir = Scratch::new("profile-dream-cut");
        let (mut profile, repository) = dreaming(&dir);
        let checkouts = dir.join("dreams/3");
        std::fs::write(checkouts.join("main/home.md"), "home")
            .unwrap();
        std::fs::write(checkouts.join("hire/kept.md"), "kept")
            .unwrap();

        end_cut_off(&mut profile);
        assert!(!checkouts.exists());
        let main = repository.tip(MAIN).unwrap().unwrap();
        let hire = repository.tip("hires/7").unwrap().unwrap();

        let (ended, told) = profile
            .once::<Profile, _, _>(realm(), end_3(), end)
            .unwrap();
        assert_eq!(
            ended,
            DreamEnd::Dreamt {
                main: Some(commit_id(main).unwrap())
            }
        );
        assert_eq!(repository.tip(MAIN).unwrap(), Some(main));
        assert_eq!(repository.tip("hires/7").unwrap(), Some(hire));
        assert_eq!(told.len(), 1);
        let made = (
            dreamt(&repository, MAIN),
            dreamt(&repository, "hires/7"),
        );
        assert_eq!(made, (1, 1));
        for (at, path) in
            [(main, "home.md"), (hire, "kept.md"), (hire, "home.md")]
        {
            assert!(
                repository.read(at, path).unwrap().is_some(),
                "{path}"
            );
        }
    }

    #[test]
    fn a_folder_a_kill_left_half_removed_is_not_read_as_removals() {
        let dir = Scratch::new("profile-dream-half-removed");
        let (mut profile, repository) = dreaming(&dir);
        std::fs::write(dir.join("dreams/3/main/home.md"), "home")
            .unwrap();
        end_cut_off(&mut profile);
        let hire = repository.tip("hires/7").unwrap().unwrap();
        // The hire's checkout, emptied part-way by the cut-off
        // removal.
        let left = dir.join("dreams/3.ended/hire");
        std::fs::create_dir_all(&left).unwrap();

        let (ended, _) = profile
            .once::<Profile, _, _>(realm(), end_3(), end)
            .unwrap();
        assert!(matches!(ended, DreamEnd::Dreamt { main: Some(_) }));
        assert_eq!(repository.tip("hires/7").unwrap(), Some(hire));
        assert_eq!(dreamt(&repository, "hires/7"), 0);
        assert!(
            repository.read(hire, "learned.md").unwrap().is_some()
        );
        assert!(!dir.join("dreams/3.ended").exists());
    }

    #[test]
    fn an_end_after_a_half_removed_folder_and_a_prepare_s_repeat_ends()
     {
        let dir = Scratch::new("profile-dream-ended-again");
        let (mut profile, _) = dreaming(&dir);
        end_cut_off(&mut profile);
        let left = dir.join("dreams/3.ended/hire");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::write(left.join("left.md"), "left").unwrap();
        let asked = PrepareDream {
            dream: DreamId(3),
            of: DreamOf::Hire {
                agent: AgentNumber(7),
            },
        };
        profile
            .once::<Profile, _, _>(realm(), asked, prepare)
            .unwrap();

        let (ended, _) = profile
            .once::<Profile, _, _>(realm(), end_3(), end)
            .unwrap();
        assert!(matches!(ended, DreamEnd::Dreamt { .. }));
        assert!(!dir.join("dreams/3").exists());
        assert!(!dir.join("dreams/3.ended").exists());
    }

    #[test]
    fn what_the_hire_saved_during_its_dream_wins_on_its_branch() {
        let dir = Scratch::new("profile-dream-hire-wins");
        let (mut profile, repository) = dreaming(&dir);
        let by = Author {
            name: "agent:7",
            at: 6000,
        };
        let saved = write("learned.md", "saved during the dream");
        repository.commit("hires/7", &[saved], "saved", by).unwrap();
        std::fs::write(
            dir.join("dreams/3/hire/learned.md"),
            "dreamt",
        )
        .unwrap();

        profile
            .once::<Profile, _, _>(realm(), end_3(), end)
            .unwrap();
        let hire = repository.tip("hires/7").unwrap().unwrap();
        let text = repository.read(hire, "learned.md").unwrap();
        assert_eq!(
            text.as_deref(),
            Some(&b"saved during the dream"[..])
        );
    }

    #[test]
    fn a_prepare_s_repeat_checks_out_the_commits_its_row_holds() {
        let dir = Scratch::new("profile-dream-prepare-again");
        let (mut profile, repository) = dreaming(&dir);
        let by = Author {
            name: "chief:3",
            at: 6000,
        };
        let later = write("memory/later.md", "later");
        repository.commit(MAIN, &[later], "edited", by).unwrap();
        let checkout = dir.join("dreams/3/hire");
        std::fs::remove_file(checkout.join("learned.md")).unwrap();

        let asked = PrepareDream {
            dream: DreamId(3),
            of: DreamOf::Hire {
                agent: AgentNumber(7),
            },
        };
        let (again, _) = profile
            .once::<Profile, _, _>(realm(), asked, prepare)
            .unwrap();
        assert_eq!(again, ready(DreamId(3), true));
        assert!(checkout.join("learned.md").exists());
        assert!(!dir.join("dreams/3/main/memory/later.md").exists());
    }
}
