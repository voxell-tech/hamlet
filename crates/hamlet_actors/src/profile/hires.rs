//! The `hire` table and the hires' branches: making one, pinning
//! one, and taking `main` into those that follow it.

use anyhow::{Context, Result, bail};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_git::{Joined, MAIN, blob_id};
use hamlet_messages::Message;
use hamlet_messages::address::Agent;
use hamlet_messages::profile::{
    Branch, Character, Missing, Moved, NotAHire, Pin, PinAnswer,
    VersionName,
};
use hamlet_messages::realm::{Done, ProfileId};
use hamlet_messages::workspace::AgentNumber;

use super::{CHARACTER, Profile, commit_id};
use crate::repo::git::Reads;
use crate::repo::store::{self, Rows, Store, Writes};
use crate::repo::{
    Admits, Before, Call, Ended, Look, Reply, Settle, Takes, Writer,
};

const TOOK_IN: &str = "Took in what its scholar learned";

pub(super) struct Table(());

impl store::Table for Table {
    const NAME: &'static str = "hire";
}

/// A hire of this profile, as its row holds it.
pub(crate) struct Hire {
    pub agent: AgentNumber,
    pub branch: String,
    /// The version the hire is pinned to; `None` while it follows
    /// `main`.
    pub pin: Option<VersionName>,
    /// The character the agent is given: its version and the blob of
    /// `CHARACTER.md` it counts changes of.
    pub character: u32,
    pub character_blob: String,
}

const COLUMNS: &str = "agent, branch, pin, character, character_blob";

fn hire(row: &[ColumnValue]) -> Result<Hire> {
    match row {
        [
            ColumnValue::Integer(agent),
            ColumnValue::Text(branch),
            pin,
            ColumnValue::Integer(character),
            ColumnValue::Text(character_blob),
        ] => Ok(Hire {
            agent: AgentNumber(u32::try_from(*agent)?),
            branch: branch.clone(),
            pin: match pin {
                ColumnValue::Text(pin) => {
                    Some(VersionName(pin.clone()))
                }
                _ => None,
            },
            character: u32::try_from(*character)?,
            character_blob: character_blob.clone(),
        }),
        _ => bail!("a hire row of another shape: {row:?}"),
    }
}

pub(super) fn of(
    rows: &Rows<'_, Table>,
    agent: AgentNumber,
) -> Result<Option<Hire>> {
    rows.select(
        &format!("SELECT {COLUMNS} FROM hire WHERE agent = ?1"),
        vec![BindParam::Integer(i64::from(agent.0))],
    )?
    .first()
    .map(|row| hire(row))
    .transpose()
}

impl Admits<Agent> for Profile {
    type Known = Hire;

    fn known(
        store: &Store<'_>,
        from: &Agent,
    ) -> Result<Option<Hire>> {
        of(&store.rows(Table(())), from.0)
    }
}

pub(super) fn every<M>(
    look: &Look<'_, Profile, M>,
) -> Result<Vec<Hire>>
where
    M: Message,
    Profile: Admits<M::From>,
{
    all(&look.rows(Table(())))
}

pub(super) fn find<M>(
    look: &Look<'_, Profile, M>,
    agent: AgentNumber,
) -> Result<Option<Hire>>
where
    M: Message,
    Profile: Admits<M::From>,
{
    of(&look.rows(Table(())), agent)
}

fn all(rows: &Rows<'_, Table>) -> Result<Vec<Hire>> {
    rows.select(
        &format!("SELECT {COLUMNS} FROM hire ORDER BY agent"),
        Vec::new(),
    )?
    .iter()
    .map(|row| hire(row))
    .collect()
}

/// The agent a hire's branch `hires/<n>` is named for.
fn hire_of(name: &str) -> Option<AgentNumber> {
    name.strip_prefix("hires/")
        .and_then(|n| {
            n.parse::<u32>().ok().filter(|m| m.to_string() == n)
        })
        .map(AgentNumber)
}

/// The character at `at` under the version the hire's row counts.
pub(super) fn character(
    git: &Reads,
    hire: &Hire,
    at: hamlet_git::CommitId,
) -> Result<Character> {
    Ok(Character {
        version: hire.character,
        text: character_at(git, at)?.1,
    })
}

/// The blob of `CHARACTER.md` at `at` as the `hire` row keeps it, and
/// its text; an empty blob for none.
fn character_at(
    git: &Reads,
    at: hamlet_git::CommitId,
) -> Result<(String, String)> {
    Ok(match git.read(at, CHARACTER)? {
        Some(bytes) => (
            blob_id(&bytes).to_string(),
            String::from_utf8_lossy(&bytes).into_owned(),
        ),
        None => (String::new(), String::new()),
    })
}

impl Takes<Branch> for Profile {
    async fn take(call: Call<Self, Branch>) -> Result<Reply<Branch>> {
        let Some(agent) = hire_of(&call.name) else {
            bail!("`{}` is not a hire's branch", call.name);
        };
        let own = call.owner().own.0;
        call.once(move |unit| branch(unit, agent, own)).await
    }
}

/// Makes the branch at the version `from` names, pinned to it, or at
/// `main`'s tip, with its `hire` row, or the half a run a kill cut
/// off did not make.
fn branch(
    unit: Before<'_, Profile, Branch>,
    agent: AgentNumber,
    own: ProfileId,
) -> Result<Ended<Branch>> {
    let Branch { name, from } = unit.message().clone();
    let has_row = of(&unit.rows(Table(())), agent)?.is_some();
    let (tip, at, character) = {
        let git = unit.git()?;
        let at = match start(git, from.as_ref(), own)? {
            Ok(at) => at,
            Err(why) => return unit.refuse(why),
        };
        let tip = git.tip(&name)?;
        (tip, at, character_at(git, tip.unwrap_or(at))?.0)
    };
    if tip.is_some() && has_row {
        return unit.kept(Done::Done);
    }
    let writer = unit.write();
    if tip.is_none() {
        writer.writes()?.branch(&name, at)?;
    }
    if !has_row {
        let [pin, pin_commit] = pinned(from.as_ref(), at);
        writer.table(Table(())).execute(
            "INSERT INTO hire (agent, branch, pin, pin_commit, \
             character, character_blob) VALUES (?1, ?2, ?3, ?4, 1, \
             ?5)",
            vec![
                BindParam::Integer(i64::from(agent.0)),
                BindParam::Text(name),
                pin,
                pin_commit,
                BindParam::Text(character),
            ],
        )?;
    }
    writer.kept(Done::Done)
}

/// The commit a hire made or pinned at `version` starts from, or
/// `main`'s tip when `None`.
fn start(
    git: &Reads,
    version: Option<&VersionName>,
    own: ProfileId,
) -> Result<Result<hamlet_git::CommitId, Missing>> {
    let Some(version) = version else {
        return Ok(Ok(git
            .tip(MAIN)?
            .context("a profile has a main")?));
    };
    Ok(version_at(git, version)?.ok_or_else(|| Missing::NoVersion {
        profile: own,
        version: version.clone(),
    }))
}

/// The `pin` and `pin_commit` of a hire pinned to `version` at `at`,
/// or of one that follows `main` when `None`.
fn pinned(
    version: Option<&VersionName>,
    at: hamlet_git::CommitId,
) -> [BindParam; 2] {
    match version {
        Some(version) => [
            BindParam::Text(version.0.clone()),
            BindParam::Text(at.to_string()),
        ],
        None => [BindParam::Null, BindParam::Null],
    }
}

/// Takes `main` into the branch of each hire that follows it, the
/// hire's side winning, and tells its agent when the branch moved.
pub(super) fn follow_main(
    settle: &mut Settle<'_, Profile>,
) -> Result<()> {
    let main =
        settle.git()?.tip(MAIN)?.context("a profile has a main")?;
    for hire in all(&settle.table(Table(())).rows())? {
        if hire.pin.is_some() {
            continue;
        }
        let merged =
            settle.writes()?.take_in(&hire.branch, main, TOOK_IN)?;
        if merged.joined == Joined::UpToDate {
            continue;
        }
        let moved = moved(
            &settle.table(Table(())),
            settle.git()?,
            &hire,
            merged.commit,
        )?;
        settle.tell(&Agent(hire.agent), &moved)?;
    }
    Ok(())
}

impl Takes<Pin> for Profile {
    async fn take(call: Call<Self, Pin>) -> Result<Reply<Pin>> {
        let own = call.owner().own.0;
        call.once(move |unit| pin(unit, own)).await
    }
}

/// Keeps the hire's pin and takes the pinned commit, or `main`'s tip,
/// into its branch, the hire's side winning.
fn pin(
    unit: Before<'_, Profile, Pin>,
    own: ProfileId,
) -> Result<Ended<Pin>> {
    let Pin { hire, to, .. } = unit.message().clone();
    let Some(row) = of(&unit.rows(Table(())), hire)? else {
        return unit.refuse(NotAHire::NoHire { hire }.into());
    };
    let at = match start(unit.git()?, to.as_ref(), own)? {
        Ok(at) => at,
        Err(why) => return unit.refuse(why.into()),
    };
    let mut writer = unit.write();
    let [pin, pin_commit] = pinned(to.as_ref(), at);
    writer.table(Table(())).execute(
        "UPDATE hire SET pin = ?1, pin_commit = ?2 WHERE agent = ?3",
        vec![pin, pin_commit, BindParam::Integer(i64::from(hire.0))],
    )?;
    let merged =
        writer.writes()?.take_in(&row.branch, at, TOOK_IN)?;
    if merged.joined != Joined::UpToDate {
        tell_moved(&mut writer, &row, merged.commit)?;
    }
    writer.kept(match to {
        Some(version) => PinAnswer::Pinned {
            version,
            commit: commit_id(at)?,
        },
        None => PinAnswer::FollowsMain,
    })
}

/// After a purge rewrote the history: each pin's commit as its
/// version names it now, and each agent told its branch's new tip.
pub(super) fn repin<M>(
    writer: &mut Writer<'_, Profile, M>,
) -> Result<()>
where
    M: Message,
    Profile: Admits<M::From>,
{
    for hire in every(writer)? {
        if let Some(version) = &hire.pin {
            let at = version_at(writer.git()?, version)?
                .context("a purge keeps every version")?;
            writer.table(Table(())).execute(
                "UPDATE hire SET pin_commit = ?1 WHERE agent = ?2",
                vec![
                    BindParam::Text(at.to_string()),
                    BindParam::Integer(i64::from(hire.agent.0)),
                ],
            )?;
        }
        let tip = writer
            .git()?
            .tip(&hire.branch)?
            .context("a hire has a branch")?;
        tell_moved(writer, &hire, tip)?;
    }
    Ok(())
}

pub(super) fn take_in_main<M>(
    writer: &Writer<'_, Profile, M>,
    hire: &Hire,
) -> Result<()>
where
    M: Message,
    Profile: Admits<M::From>,
{
    if hire.pin.is_none() {
        let main = writer
            .git()?
            .tip(MAIN)?
            .context("a profile has a main")?;
        writer.writes()?.take_in(&hire.branch, main, TOOK_IN)?;
    }
    Ok(())
}

pub(super) fn tell_moved<M>(
    writer: &mut Writer<'_, Profile, M>,
    hire: &Hire,
    tip: hamlet_git::CommitId,
) -> Result<()>
where
    M: Message,
    Profile: Admits<M::From>,
{
    let moved =
        moved(&writer.table(Table(())), writer.git()?, hire, tip)?;
    writer.tell(&Agent(hire.agent), &moved)
}

pub(super) fn version_at(
    git: &Reads,
    version: &VersionName,
) -> Result<Option<hamlet_git::CommitId>> {
    Ok(git
        .versions()?
        .into_iter()
        .find(|seen| seen.name == version.0)
        .map(|seen| seen.commit))
}

/// The [`Moved`] the agent of a hire whose branch moved to `tip` is
/// told.
fn moved(
    table: &Writes<'_, Table>,
    git: &Reads,
    hire: &Hire,
    tip: hamlet_git::CommitId,
) -> Result<Moved> {
    Ok(Moved {
        tip: commit_id(tip)?,
        character: next_character(table, git, hire, tip)?,
    })
}

/// The character at `tip` under the hire's next version, kept in its
/// row, when it is not the one the row counts.
fn next_character(
    table: &Writes<'_, Table>,
    git: &Reads,
    hire: &Hire,
    tip: hamlet_git::CommitId,
) -> Result<Option<Character>> {
    let (blob, text) = character_at(git, tip)?;
    if blob == hire.character_blob {
        return Ok(None);
    }
    let version = hire.character + 1;
    table.execute(
        "UPDATE hire SET character = ?1, character_blob = ?2 WHERE \
         agent = ?3",
        vec![
            BindParam::Integer(i64::from(version)),
            BindParam::Text(blob),
            BindParam::Integer(i64::from(hire.agent.0)),
        ],
    )?;
    Ok(Some(Character { version, text }))
}

#[cfg(test)]
mod tests {
    use hamlet_git::{Author, Change, Repository};
    use hamlet_host::Caller;
    use hamlet_messages::address::{Party, Realm};
    use hamlet_testing::Scratch;

    use super::*;
    use crate::repo::Told;
    use crate::repo::testing::Hostless;

    fn branch_of(profile: &mut Hostless, agent: u32) -> Vec<Told> {
        let realm = Caller::Actor(Realm.address().unwrap());
        let asked = Branch {
            name: format!("hires/{agent}"),
            from: None,
        };
        let unit = |unit: Before<'_, Profile, Branch>| {
            branch(unit, AgentNumber(agent), ProfileId(1))
        };
        profile.once(realm, asked, unit).unwrap().1
    }

    fn repository(dir: &Scratch) -> Repository {
        Repository::open(&dir.join("scholars/1.git")).unwrap()
    }

    /// Moves `main` as a chief's edit of the character does.
    fn edit_character(
        dir: &Scratch,
        text: &str,
    ) -> hamlet_git::CommitId {
        let character = Change::Write {
            path: CHARACTER.into(),
            bytes: text.into(),
        };
        let by = Author {
            name: "chief:3",
            at: 9000,
        };
        repository(dir)
            .commit(MAIN, &[character], "edited", by)
            .unwrap()
    }

    fn moved(
        agent: u32,
        tip: hamlet_git::CommitId,
        character: Option<Character>,
    ) -> Told {
        let moved = Moved {
            tip: commit_id(tip).unwrap(),
            character,
        };
        Told::of(&Agent(AgentNumber(agent)), &moved).unwrap()
    }

    #[test]
    fn a_kept_unit_takes_main_into_each_hire_that_follows_it() {
        let dir = Scratch::new("profile-follow-main");
        let mut profile = Hostless::profile(&dir, "Careful.");
        assert!(branch_of(&mut profile, 7).is_empty());
        branch_of(&mut profile, 8);
        let pinned = repository(&dir).tip("hires/8").unwrap();
        profile
            .store()
            .execute("UPDATE hire SET pin = 'v1' WHERE agent = 8", [])
            .unwrap();
        let main = edit_character(&dir, "Bolder.");

        let told = branch_of(&mut profile, 7);
        let character = Character {
            version: 2,
            text: "Bolder.".into(),
        };
        assert_eq!(told, [moved(7, main, Some(character))]);
        assert_eq!(
            repository(&dir).tip("hires/7").unwrap(),
            Some(main)
        );
        assert_eq!(repository(&dir).tip("hires/8").unwrap(), pinned);
        assert!(branch_of(&mut profile, 7).is_empty());
    }

    #[test]
    fn a_branch_cut_off_before_its_row_is_finished_by_its_repeat() {
        let dir = Scratch::new("profile-branch-row");
        let mut profile = Hostless::profile(&dir, "Careful.");
        let repository = repository(&dir);
        let first = repository.tip(MAIN).unwrap().unwrap();
        repository.branch("hires/7", first).unwrap();

        assert!(branch_of(&mut profile, 7).is_empty());
        let main = edit_character(&dir, "Bolder.");
        let told = branch_of(&mut profile, 9);
        let character = Character {
            version: 2,
            text: "Bolder.".into(),
        };
        assert_eq!(told, [moved(7, main, Some(character))]);
    }
}
