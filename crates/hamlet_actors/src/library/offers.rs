//! Offers and their verdicts: an offer's files kept part by part,
//! then on `offers/<offer>`; an accepted one shelved on `main`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_git::{Change, MAIN};
use hamlet_messages::address::Agent;
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::library::{
    BadOffer, BadSkill, DropCopy, Offer, OfferAnswer, OfferId,
    OfferRefusal, OnTheShelves, Verdict, VerdictRefusal,
};
use hamlet_messages::profile::{Piece, TooLarge};
use hamlet_messages::realm::Done;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::{MOST_PART, PIECE_FRAME};
use sha2::Digest as _;

use super::{
    Event, Events, Library, SKILL_FILE, field, is_skill_name, main,
    published, record, skill_paths,
};
use crate::repo::git::{By, Reads};
use crate::repo::store::{self, Rows, Writes};
use crate::repo::{Before, Call, Ended, Reply, Takes};

/// The most bytes the files of one skill hold.
const MOST_SKILL: usize = 4 << 20;

/// The token of the `offer` table.
struct Offers(());

impl store::Table for Offers {
    const NAME: &'static str = "offer";
}

/// The token of the `offer_piece` table.
struct Pieces(());

impl store::Table for Pieces {
    const NAME: &'static str = "offer_piece";
}

/// An offer, as its row holds it.
struct Row {
    id: u32,
    agent: AgentNumber,
    skill: String,
    reason: String,
    /// The commit `offers/<id>` names, once its last part came.
    commit: Option<hamlet_git::CommitId>,
    /// `None` until it is judged.
    accepted: Option<bool>,
}

const COLUMNS: &str = "id, agent, skill, reason, commit_id, accepted";

impl Row {
    fn of(row: &[ColumnValue]) -> Result<Self> {
        match row {
            [
                ColumnValue::Integer(id),
                ColumnValue::Integer(agent),
                ColumnValue::Text(skill),
                ColumnValue::Text(reason),
                commit,
                accepted,
            ] => Ok(Self {
                id: u32::try_from(*id)?,
                agent: AgentNumber(u32::try_from(*agent)?),
                skill: skill.clone(),
                reason: reason.clone(),
                commit: match commit {
                    ColumnValue::Text(commit) => Some(
                        commit.parse().map_err(anyhow::Error::msg)?,
                    ),
                    _ => None,
                },
                accepted: match accepted {
                    ColumnValue::Integer(accepted) => {
                        Some(*accepted != 0)
                    }
                    _ => None,
                },
            }),
            _ => bail!("an offer row of another shape: {row:?}"),
        }
    }
}

fn first(
    rows: &Rows<'_, Offers>,
    sql: &str,
    params: Vec<BindParam>,
) -> Result<Option<Row>> {
    rows.select(
        &format!("SELECT {COLUMNS} FROM offer {sql}"),
        params,
    )?
    .first()
    .map(|row| Row::of(row))
    .transpose()
}

/// The bytes a part counts for: each piece's bytes, its path and the
/// frame.
fn part_size(files: &[Piece]) -> usize {
    files
        .iter()
        .map(|piece| {
            piece.bytes.0.len() + piece.path.len() + PIECE_FRAME
        })
        .sum()
}

/// Whether `path` is a path inside a skill's folder.
fn is_inner_path(path: &str) -> bool {
    path.split('/')
        .all(|name| !name.is_empty() && name != "." && name != "..")
}

/// The files `pieces` make, each from its pieces end to end, the last
/// one marked, in path order.
fn whole(mut pieces: Vec<Piece>) -> Result<Vec<Change>> {
    pieces.sort_by(|a, b| {
        (&a.path, a.offset).cmp(&(&b.path, b.offset))
    });
    let mut files = BTreeMap::<String, (Vec<u8>, bool, bool)>::new();
    for piece in pieces {
        let (bytes, last, _) = files
            .entry(piece.path.clone())
            .or_insert((Vec::new(), false, piece.executable));
        ensure!(
            !*last && usize::try_from(piece.offset)? == bytes.len(),
            "a piece of {} out of place at {}",
            piece.path,
            piece.offset
        );
        bytes.extend(piece.bytes.0);
        *last = piece.last;
    }
    files
        .into_iter()
        .map(|(path, (bytes, last, executable))| {
            ensure!(last, "{path} has no last piece");
            Ok(if executable {
                Change::WriteExecutable { path, bytes }
            } else {
                Change::Write { path, bytes }
            })
        })
        .collect()
}

/// Whether `files` hold a `SKILL.md` that opens with a front matter
/// `name:` and `description:`.
fn has_front_matter(files: &[Change]) -> bool {
    files.iter().any(|change| match change {
        Change::Write { path, bytes }
        | Change::WriteExecutable { path, bytes }
            if path == SKILL_FILE =>
        {
            let text = String::from_utf8_lossy(bytes);
            field(&text, "name").is_some()
                && field(&text, "description").is_some()
        }
        _ => false,
    })
}

/// The pieces the store keeps of the offer `offer`.
fn pieces(rows: &Rows<'_, Pieces>, offer: u32) -> Result<Vec<Piece>> {
    rows.select(
        "SELECT path, offset, bytes, last, executable FROM \
         offer_piece WHERE offer = ?1",
        vec![BindParam::Integer(i64::from(offer))],
    )?
    .into_iter()
    .map(|row| match row.as_slice() {
        [
            ColumnValue::Text(path),
            ColumnValue::Integer(offset),
            ColumnValue::Blob(bytes),
            ColumnValue::Integer(last),
            ColumnValue::Integer(executable),
        ] => Ok(Piece {
            path: path.clone(),
            offset: u32::try_from(*offset)?,
            bytes: Base64(bytes.clone()),
            last: *last != 0,
            executable: *executable != 0,
        }),
        _ => bail!("a piece row of another shape: {row:?}"),
    })
    .collect()
}

impl Takes<Offer> for Library {
    async fn take(call: Call<Self, Offer>) -> Result<Reply<Offer>> {
        if !is_skill_name(&call.skill) {
            let skill = call.skill.clone();
            let why = BadOffer::NotASkillName { skill };
            return call.refuse(OfferRefusal::Shape(why)).await;
        }
        if part_size(&call.files) > MOST_PART {
            let why = TooLarge::PartTooLarge {
                most: u32::try_from(MOST_PART)?,
            };
            return call.refuse(OfferRefusal::Size(why)).await;
        }
        call.once(offer).await
    }
}

/// Keeps one part. The first part of a call drops the caller's other
/// offers that have no verdict and either never came whole or offer
/// the same skill. The last commits the offer's files to
/// `offers/<offer>`.
fn offer(unit: Before<'_, Library, Offer>) -> Result<Ended<Offer>> {
    let Offer {
        call,
        skill,
        reason,
        part,
        last,
        files,
    } = unit.message().clone();
    let agent = unit.from().0;
    let by_call = vec![
        BindParam::Integer(i64::from(agent.0)),
        BindParam::Text(call.0.clone()),
    ];
    let row = first(
        &unit.rows(Offers(())),
        "WHERE agent = ?1 AND call = ?2",
        by_call.clone(),
    )?;
    ensure!(
        row.is_some() || part == 0,
        "the part {part} of an offer the library does not hold"
    );
    if let Some(row) = &row {
        ensure!(
            row.commit.is_none(),
            "the offer {} is whole",
            row.id
        );
        ensure!(
            row.skill == skill,
            "the parts of one offer name one skill"
        );
    }
    ensure!(
        files.iter().all(|piece| is_inner_path(&piece.path)),
        "a piece's path is not inside the skill's folder"
    );
    let main = main(unit.git()?)?;
    if !skill_paths(unit.git()?, main, &skill)?.is_empty() {
        let why = OnTheShelves::AlreadyShelved { skill };
        return unit.refuse(OfferRefusal::Shelves(why));
    }
    let judged = first(
        &unit.rows(Offers(())),
        "WHERE skill = ?1 AND agent != ?2 AND commit_id IS NOT NULL \
         AND accepted IS NULL",
        vec![
            BindParam::Text(skill.clone()),
            BindParam::Integer(i64::from(agent.0)),
        ],
    )?;
    if judged.is_some() {
        let why = OnTheShelves::BeingJudged { skill };
        return unit.refuse(OfferRefusal::Shelves(why));
    }
    let mut held = match &row {
        Some(row) => pieces(&unit.rows(Pieces(())), row.id)?,
        None => Vec::new(),
    };
    held.extend(files.iter().cloned());
    let size =
        held.iter().map(|piece| piece.bytes.0.len()).sum::<usize>();
    if size > MOST_SKILL {
        let most = u32::try_from(MOST_SKILL)?;
        let why = BadSkill::SkillTooLarge { skill, most };
        return unit.refuse(OfferRefusal::Skill(why));
    }
    let whole = if last {
        let whole = whole(held)?;
        if !has_front_matter(&whole) {
            let why = BadSkill::NoFrontMatter { skill };
            return unit.refuse(OfferRefusal::Skill(why));
        }
        Some(whole)
    } else {
        None
    };

    let w = unit.write();
    let offers = w.table(Offers(()));
    let id = match row {
        Some(row) => row.id,
        None => {
            let mine = "agent = ?1 AND accepted IS NULL AND \
                        (commit_id IS NULL OR skill = ?2)";
            let params = vec![
                BindParam::Integer(i64::from(agent.0)),
                BindParam::Text(skill.clone()),
            ];
            w.table(Pieces(())).execute(
                &format!(
                    "DELETE FROM offer_piece WHERE offer IN (SELECT \
                     id FROM offer WHERE {mine})"
                ),
                params.clone(),
            )?;
            offers.execute(
                &format!("DELETE FROM offer WHERE {mine}"),
                params,
            )?;
            offers.execute(
                "INSERT INTO offer (agent, call, skill, reason) \
                 VALUES (?1, ?2, ?3, ?4)",
                vec![
                    by_call[0].clone(),
                    by_call[1].clone(),
                    BindParam::Text(skill),
                    BindParam::Text(reason),
                ],
            )?;
            first(
                &offers.rows(),
                "WHERE agent = ?1 AND call = ?2",
                by_call,
            )?
            .context("the offer's row, just kept")?
            .id
        }
    };
    let Some(whole) = whole else {
        keep_pieces(&w.table(Pieces(())), id, files)?;
        return w.kept(OfferAnswer::More);
    };
    let branch = format!("offers/{id}");
    let was = w.git()?.tip(&branch)?;
    let changes = exactly(w.git()?, was, whole)?;
    let commit = w.writes()?.commit(
        &branch,
        was,
        &changes,
        "Offered",
        By::Library,
    )?;
    offers.execute(
        "UPDATE offer SET commit_id = ?1 WHERE id = ?2",
        vec![
            BindParam::Text(commit.to_string()),
            BindParam::Integer(i64::from(id)),
        ],
    )?;
    w.table(Pieces(())).execute(
        "DELETE FROM offer_piece WHERE offer = ?1",
        vec![BindParam::Integer(i64::from(id))],
    )?;
    w.kept(OfferAnswer::Kept { offer: OfferId(id) })
}

fn keep_pieces(
    table: &Writes<'_, Pieces>,
    offer: u32,
    files: Vec<Piece>,
) -> Result<()> {
    for piece in files {
        table.execute(
            "INSERT INTO offer_piece (offer, path, offset, bytes, \
             last, executable) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            vec![
                BindParam::Integer(i64::from(offer)),
                BindParam::Text(piece.path),
                BindParam::Integer(i64::from(piece.offset)),
                BindParam::Blob(piece.bytes.0),
                BindParam::Integer(i64::from(piece.last)),
                BindParam::Integer(i64::from(piece.executable)),
            ],
        )?;
    }
    Ok(())
}

/// The changes that make the files of `was` exactly `files`. A branch
/// an offer's id names already holds the files of a run a kill cut
/// off, which the store does not remember.
fn exactly(
    git: &Reads,
    was: Option<hamlet_git::CommitId>,
    mut files: Vec<Change>,
) -> Result<Vec<Change>> {
    if let Some(was) = was {
        let kept = files
            .iter()
            .filter_map(|change| match change {
                Change::Write { path, .. }
                | Change::WriteExecutable { path, .. } => Some(path),
                Change::Remove { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let gone = git
            .entries(was)?
            .into_iter()
            .filter(|entry| !kept.contains(&entry.path))
            .map(|entry| Change::Remove { path: entry.path })
            .collect::<Vec<_>>();
        files.extend(gone);
    }
    Ok(files)
}

/// The files of an offer's commit, in path order.
fn offered(
    git: &Reads,
    at: hamlet_git::CommitId,
) -> Result<Vec<(hamlet_git::Entry, Vec<u8>)>> {
    let mut entries = git.entries(at)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
        .into_iter()
        .map(|entry| {
            let bytes = git.blob(entry.blob)?;
            Ok((entry, bytes))
        })
        .collect()
}

/// The SHA-256 over each file's path, then its bytes, in path order:
/// what [`DropCopy`] carries.
fn digest(files: &[(hamlet_git::Entry, Vec<u8>)]) -> Sha256 {
    let mut digest = sha2::Sha256::new();
    for (entry, bytes) in files {
        digest.update(entry.path.as_bytes());
        digest.update(bytes);
    }
    Sha256(digest.finalize().into())
}

impl Takes<Verdict> for Library {
    async fn take(
        call: Call<Self, Verdict>,
    ) -> Result<Reply<Verdict>> {
        call.once(verdict).await
    }
}

/// Keeps the verdict. Accepted, the skill is committed to `main`
/// unless `main` holds it already, from a run a kill cut off before
/// the marker: no other offer of a skill comes whole while one waits.
fn verdict(
    unit: Before<'_, Library, Verdict>,
) -> Result<Ended<Verdict>> {
    let Verdict {
        offer,
        accepted,
        words,
        ..
    } = unit.message().clone();
    let found = first(
        &unit.rows(Offers(())),
        "WHERE id = ?1 AND commit_id IS NOT NULL",
        vec![BindParam::Integer(i64::from(offer.0))],
    )?;
    let Some(Row {
        agent,
        skill,
        reason,
        commit: Some(commit),
        accepted: None,
        ..
    }) = found
    else {
        return unit.refuse(match found {
            Some(_) => VerdictRefusal::Judged { offer },
            None => VerdictRefusal::NoSuchOffer { offer },
        });
    };
    let by = By::Agent(agent);
    let mut w = unit.write();
    w.table(Offers(())).execute(
        "UPDATE offer SET accepted = ?1, words = ?2 WHERE id = ?3",
        vec![
            BindParam::Integer(i64::from(accepted)),
            BindParam::Text(words.clone()),
            BindParam::Integer(i64::from(offer.0)),
        ],
    )?;
    if !accepted {
        let event = Event {
            kind: "refused",
            skill: &skill,
            by,
            why: &words,
            commit: None,
        };
        record(&w.table(Events(())), w.at(), event)?;
        return w.kept(Done::Done);
    }
    let files = offered(w.git()?, commit)?;
    let main = main(w.git()?)?;
    let shelved = if skill_paths(w.git()?, main, &skill)?.is_empty() {
        let changes = files
            .iter()
            .map(|(entry, bytes)| {
                let path = format!("{skill}/{}", entry.path);
                let bytes = bytes.clone();
                if entry.executable {
                    Change::WriteExecutable { path, bytes }
                } else {
                    Change::Write { path, bytes }
                }
            })
            .collect::<Vec<_>>();
        let message = format!("Shelved {skill}: {reason}");
        w.writes()?.commit(
            MAIN,
            Some(main),
            &changes,
            &message,
            by,
        )?
    } else {
        main
    };
    let event = Event {
        kind: "shelved",
        skill: &skill,
        by,
        why: &reason,
        commit: Some(shelved),
    };
    record(&w.table(Events(())), w.at(), event)?;
    published(&mut w)?;
    let copy = DropCopy {
        skill,
        digest: digest(&files),
    };
    w.tell(&Agent(agent), &copy)?;
    w.kept(Done::Done)
}

#[cfg(test)]
mod tests {
    use hamlet_host::Caller;
    use hamlet_messages::address::{Party, Realm};
    use hamlet_messages::library::{
        ShelfList, Shelves, ShelvesChanged, Skill,
    };
    use hamlet_messages::workspace::CallId;
    use hamlet_protocol::client::RequestId;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::repo::Told;
    use crate::repo::testing::Hostless;

    const SKILL_TEXT: &str =
        "---\nname: tidy\ndescription: Tidy a folder.\n---\nTidy.\n";

    fn piece(
        path: &str,
        offset: u32,
        bytes: &[u8],
        last: bool,
    ) -> Piece {
        Piece {
            path: path.into(),
            offset,
            bytes: Base64(bytes.to_vec()),
            last,
            executable: false,
        }
    }

    #[test]
    fn a_file_is_made_of_its_pieces_end_to_end() {
        let made = whole(vec![
            piece("b.md", 2, b"cd", true),
            piece("b.md", 0, b"ab", false),
            piece("a.md", 0, b"x", true),
        ])
        .unwrap();
        assert_eq!(
            made,
            [
                Change::Write {
                    path: "a.md".into(),
                    bytes: b"x".to_vec()
                },
                Change::Write {
                    path: "b.md".into(),
                    bytes: b"abcd".to_vec()
                },
            ]
        );
        assert!(whole(vec![piece("a.md", 1, b"x", true)]).is_err());
        assert!(whole(vec![piece("a.md", 0, b"x", false)]).is_err());
    }

    #[test]
    fn a_verdict_cut_off_before_its_marker_finds_its_own_shelving() {
        let dir = Scratch::new("library-verdict-cut");
        let mut library = Hostless::library(&dir);
        let agent = Agent(AgentNumber(7));
        let as_agent = Caller::Actor(agent.address().unwrap());
        let realm = Caller::Actor(Realm.address().unwrap());
        let (listening, _) = library
            .once::<Library, Shelves, _>(
                as_agent.clone(),
                Shelves { version_seen: None },
                super::super::shelves,
            )
            .unwrap();
        let asked = Offer {
            call: CallId("c1".into()),
            skill: "tidy".into(),
            reason: "folders get messy".into(),
            part: 0,
            last: true,
            files: vec![piece(
                SKILL_FILE,
                0,
                SKILL_TEXT.as_bytes(),
                true,
            )],
        };
        let (kept, _) = library
            .once::<Library, Offer, _>(as_agent, asked, offer)
            .unwrap();
        let OfferAnswer::Kept { offer } = kept else {
            panic!("{kept:?}")
        };
        let judged = Verdict {
            id: RequestId::new(),
            offer,
            accepted: true,
            words: "Useful.".into(),
        };
        let cut = library.once::<Library, Verdict, _>(
            realm.clone(),
            judged.clone(),
            |unit| {
                verdict(unit)?;
                bail!("killed before the marker")
            },
        );
        assert!(cut.is_err());
        let repository =
            hamlet_git::Repository::open(&dir.join("library.git"))
                .unwrap();
        let shelved = repository.tip(MAIN).unwrap().unwrap();

        let (repeat, told) = library
            .once::<Library, Verdict, _>(realm, judged, verdict)
            .unwrap();
        assert_eq!(repeat, Done::Done);
        assert_eq!(repository.tip(MAIN).unwrap(), Some(shelved));
        assert_eq!(repository.history(shelved, 10).unwrap().len(), 2);
        let mut skills = listening.skills;
        skills.push(Skill {
            name: "tidy".into(),
            description: "Tidy a folder.".into(),
            usage: String::new(),
        });
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        let list = ShelfList {
            version: listening.version + 1,
            skills,
        };
        assert_eq!(
            told[0],
            Told::of(&agent, &ShelvesChanged { list }).unwrap()
        );
        assert_eq!(told.len(), 2);
        let digest = digest(&[(
            hamlet_git::Entry {
                path: SKILL_FILE.into(),
                blob: hamlet_git::blob_id(SKILL_TEXT.as_bytes()),
                executable: false,
            },
            SKILL_TEXT.as_bytes().to_vec(),
        )]);
        let copy = DropCopy {
            skill: "tidy".into(),
            digest,
        };
        assert_eq!(told[1], Told::of(&agent, &copy).unwrap());
    }

    #[test]
    fn an_offer_holds_only_its_own_files_where_a_cut_off_one_was() {
        let dir = Scratch::new("library-offer-cut");
        let mut library = Hostless::library(&dir);
        let agent =
            Caller::Actor(Agent(AgentNumber(7)).address().unwrap());
        let asked = |call: &str, files| Offer {
            call: CallId(call.into()),
            skill: "tidy".into(),
            reason: String::new(),
            part: 0,
            last: true,
            files,
        };
        let first = vec![
            piece(
                SKILL_FILE,
                0,
                b"---\nname: a\ndescription: a\n---",
                true,
            ),
            piece("a.py", 0, b"a", true),
        ];
        let cut = library.once::<Library, Offer, _>(
            agent.clone(),
            asked("c1", first),
            |unit| {
                offer(unit)?;
                bail!("killed before the store's commit")
            },
        );
        assert!(cut.is_err());

        let text = SKILL_TEXT.as_bytes();
        let second = vec![piece(SKILL_FILE, 0, text, true)];
        let (kept, _) = library
            .once::<Library, Offer, _>(
                agent,
                asked("c2", second),
                offer,
            )
            .unwrap();
        // The store forgot the cut-off offer's id, and gave it again.
        assert_eq!(kept, OfferAnswer::Kept { offer: OfferId(1) });
        let repository =
            hamlet_git::Repository::open(&dir.join("library.git"))
                .unwrap();
        let tip = repository.tip("offers/1").unwrap().unwrap();
        assert_eq!(
            repository.files(tip).unwrap(),
            [(SKILL_FILE.to_owned(), text.to_vec())]
        );
    }
}
