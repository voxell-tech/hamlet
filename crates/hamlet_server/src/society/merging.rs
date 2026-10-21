//! Merge manifests retained before house changes and applied by the
//! merge lane, or replayed at startup.

pub(crate) mod lane;

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Caller, outcome};
use crate::harness::Harnesses;
use crate::operation::Request;
use crate::papers::NOTES;
use crate::papers::files::Files;
use crate::papers::tree::{self, Entry};
use crate::record::{
    self, Actor, AgentRef, BroughtFile, Change, Departure,
    MergeFiles, MergeNotes, RecordOperation, RecordResult,
};
use crate::service::Service;
use crate::{OperationId, Outcome};

const CAP: u64 = 20 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Plan {
    operation: RecordOperation,
    snapshot: String,
    gone: AgentRef,
    kept: AgentRef,
    prompt: Option<String>,
}

fn substance(text: &str) -> String {
    text.lines()
        .filter(|line| line.trim() != "# Notes")
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .into()
}

/// Why a merge cannot be planned.
enum Unplanned {
    Refused(record::Refusal),
    /// The kept agent's notes in `hamlet` are not UTF-8 text.
    NotesNotText {
        hamlet: String,
        number: u64,
    },
    /// The kept house already has the absorbed agent's folder `base`.
    Merged {
        base: String,
    },
    /// The destination `target` of a brought file is taken.
    Taken {
        target: String,
    },
    /// Reading the houses failed.
    Failed(String),
}

impl Unplanned {
    fn text(&self) -> String {
        match self {
            Self::Refused(refusal) => {
                refusal.text(record::Audience::Chief)
            }
            Self::NotesNotText { hamlet, number } => format!(
                "The merge cannot use the kept agent's NOTES.md \
                 ({hamlet}#{number}): it is not UTF-8 text. \
                 NOTES.md was not replaced."
            ),
            Self::Merged { base } => format!(
                "The kept house already has `{base}`. Inspect it \
                 before merging again."
            ),
            Self::Taken { target } => format!(
                "The merge cannot safely deliver `{target}`; its \
                 destination is already taken."
            ),
            Self::Failed(error) => error.clone(),
        }
    }
}

/// A merge's plan, or why there is none.
type Planned = std::result::Result<Plan, Unplanned>;

struct Preparing {
    operation: RecordOperation,
    kept: record::Agent,
    gone: record::Agent,
    args: Merge,
    root: std::fs::File,
    mine: Files,
    theirs: Files,
}

/// The arguments of `merge_agents`, and of the chief's `merge`.
#[derive(Clone, Deserialize)]
pub(crate) struct Merge {
    keep: String,
    absorb: String,
    purpose: Option<String>,
    reason: String,
}

fn preparing(
    service: &mut Service,
    hamlet: &str,
    actor: Actor,
    args: &Merge,
    task: Option<String>,
) -> Result<std::result::Result<Preparing, Unplanned>> {
    let r = service
        .home
        .store()
        .record(hamlet)?
        .context("the merging hamlet has no record")?;
    let (keep, absorb) = (args.keep.as_str(), args.absorb.as_str());
    let refuse = |reason| {
        Ok(Err(Unplanned::Refused(record::Refusal {
            operation: record::Action::Merge,
            hamlet: hamlet.into(),
            reason,
        })))
    };
    if keep == absorb {
        return refuse(record::Reason::MergeSelf);
    }
    let missing = |name: &str| record::Reason::AgentMissing {
        name: name.into(),
        local: false,
    };
    let Some(kept) = r.named(keep).cloned() else {
        return refuse(missing(keep));
    };
    let Some(gone) = r.named(absorb).cloned() else {
        return refuse(missing(absorb));
    };
    let operation = RecordOperation {
        hamlet: hamlet.into(),
        actor,
        at: crate::clock::now(),
        reason: match args.reason.as_str() {
            "" => "a chief asked".into(),
            reason => reason.into(),
        },
        task,
        change: Change::Depart {
            name: absorb.into(),
            how: Departure::Merged {
                into: kept.number,
                purpose: args.purpose.clone(),
                notes: MergeNotes::Joined,
                files: MergeFiles {
                    notes_before: String::new(),
                    notes_after: String::new(),
                    brought: Vec::new(),
                    clashes: Vec::new(),
                },
            },
        },
    };
    let tx = service.home.store().transaction()?;
    super::leaving::guards(&tx, hamlet)?;
    let checked = crate::operation::change(&tx, &operation)?;
    tx.rollback()?;
    if let Outcome::Refused(refusal) = checked {
        return Ok(Err(Unplanned::Refused(refusal)));
    }
    Ok(Ok(Preparing {
        mine: service.houses.house(hamlet, kept.number)?,
        theirs: service.houses.house(hamlet, gone.number)?,
        root: service.houses.root().try_clone()?,
        operation,
        kept,
        gone,
        args: args.clone(),
    }))
}

/// The notes a merge starts from and the kept agent's once merged.
struct Notes {
    before: Entry,
    before_text: String,
    other: Entry,
    other_text: String,
    after: String,
}

impl Notes {
    /// The notes of the kept house `mine` and the absorbed house
    /// `theirs`, whose agent `absorb` was for `purpose`; `None` when
    /// the kept agent's are not UTF-8 text.
    fn of(
        mine: &BTreeMap<String, Entry>,
        theirs: &BTreeMap<String, Entry>,
        absorb: &str,
        purpose: &str,
    ) -> Option<Self> {
        let notes = |files: &BTreeMap<String, Entry>| {
            files.get(NOTES).cloned().unwrap_or(Entry {
                bytes: b"# Notes\n".to_vec(),
                mode: 0o600,
            })
        };
        let before = notes(mine);
        let before_text =
            String::from_utf8(before.bytes.clone()).ok()?;
        let other = notes(theirs);
        let other_text =
            String::from_utf8_lossy(&other.bytes).into_owned();
        let joined = if substance(&other_text).is_empty() {
            before_text.trim_end().to_owned()
        } else {
            format!(
                "{}\n\n## From {absorb}\n{}",
                before_text.trim_end(),
                substance(&other_text)
            )
        };
        let after = format!(
            "{}\n\n## Merged\n- `{absorb}` was merged into me. Its \
             purpose was: {purpose} Its notes as they were are in \
             merged/{absorb}/NOTES.md and mine in \
             merged/{absorb}/NOTES.before.md. Files of its that \
             differed from mine of the same name are under \
             merged/{absorb}/.\n",
            joined.trim_end(),
        );
        Some(Self {
            before,
            before_text,
            other,
            other_text,
            after,
        })
    }
}

impl Preparing {
    async fn run(self) -> Result<Planned> {
        let Self {
            mut operation,
            kept,
            gone,
            args,
            root,
            mine,
            theirs,
        } = self;
        let hamlet = operation.hamlet.clone();
        let (keep, absorb) =
            (args.keep.as_str(), args.absorb.as_str());
        let mine = mine.read(CAP, 2048).await?;
        let theirs = theirs.read(CAP, 2048).await?;
        let Some(notes) =
            Notes::of(&mine, &theirs, absorb, &gone.purpose)
        else {
            return Ok(Err(Unplanned::NotesNotText {
                hamlet,
                number: kept.number,
            }));
        };
        let (files, clashes) =
            match brought(&mine, theirs, absorb, &notes) {
                Ok(brought) => brought,
                Err(unplanned) => return Ok(Err(unplanned)),
            };
        let rewrite = matches!(operation.actor, Actor::Mayor(_))
            && !substance(&notes.before_text).is_empty()
            && !substance(&notes.other_text).is_empty();
        let prompt = rewrite.then(|| {
            format!(
                "You are {keep}. Merge these notes under purpose \
                 {}. Preserve every fact and disagreement with \
                 attribution. Reply only with NOTES.md beginning # \
                 Notes.\nYour notes:\n{}\nNotes from {absorb}:\n{}",
                args.purpose.as_deref().unwrap_or(&kept.purpose),
                notes.before_text,
                notes.other_text
            )
        });
        if let Change::Depart {
            how:
                Departure::Merged {
                    files: manifest, ..
                },
            ..
        } = &mut operation.change
        {
            *manifest = MergeFiles {
                brought: files
                    .iter()
                    .map(|(path, entry)| BroughtFile {
                        path: path.clone(),
                        sha256: format!(
                            "{:x}",
                            Sha256::digest(&entry.bytes)
                        ),
                    })
                    .collect(),
                notes_before: notes.before_text,
                notes_after: notes.after,
                clashes,
            };
        }
        Ok(Ok(Plan {
            operation,
            snapshot: tree::snapshot(&root, &files)?,
            gone: AgentRef {
                hamlet: hamlet.clone(),
                number: gone.number,
            },
            kept: AgentRef {
                hamlet,
                number: kept.number,
            },
            prompt,
        }))
    }
}

/// The files the absorbed house `theirs` brings into the kept house
/// `mine`, by their path there, and the clashes kept apart because
/// they differed.
fn brought(
    mine: &BTreeMap<String, Entry>,
    theirs: BTreeMap<String, Entry>,
    absorb: &str,
    notes: &Notes,
) -> std::result::Result<
    (BTreeMap<String, Entry>, Vec<String>),
    Unplanned,
> {
    let base = format!("merged/{absorb}");
    let nested = |a: &str, b: &str| {
        a == b
            || a.starts_with(&format!("{b}/"))
            || b.starts_with(&format!("{a}/"))
    };
    if mine.keys().any(|path| {
        path == &base || path.starts_with(&format!("{base}/"))
    }) {
        return Err(Unplanned::Merged { base });
    }
    let mut files = BTreeMap::new();
    files.insert(
        format!("{base}/NOTES.before.md"),
        notes.before.clone(),
    );
    files.insert(format!("{base}/{NOTES}"), notes.other.clone());
    let mut clashes = Vec::new();
    let renamed = renamed_skills(mine, &theirs, absorb, &mut clashes);
    for (path, mut entry) in theirs {
        if path == NOTES {
            continue;
        }
        let target = if let Some((skill, rest)) = path
            .strip_prefix("skills/")
            .and_then(|path| path.split_once('/'))
        {
            let name = &renamed[skill];
            if name.is_empty() {
                continue;
            }
            if name != skill {
                entry.bytes =
                    renamed_skill(&entry.bytes, skill, name);
            }
            format!("skills/{name}/{rest}")
        } else if let Some(current) = mine.get(&path) {
            if current.bytes == entry.bytes {
                continue;
            }
            clashes.push(format!(
                "file `{path}` (the absorbed one is `{base}/{path}`)"
            ));
            format!("{base}/{path}")
        } else {
            path
        };
        if mine
            .keys()
            .chain(files.keys())
            .any(|path| nested(path, &target))
        {
            return Err(Unplanned::Taken { target });
        }
        files.insert(target, entry);
    }
    if let Some(target) = files
        .keys()
        .find(|target| mine.keys().any(|path| nested(path, target)))
    {
        return Err(Unplanned::Taken {
            target: target.clone(),
        });
    }
    Ok((files, clashes))
}

/// The name each skill of the absorbed house `theirs` takes in the
/// kept house `mine`: its own, else one marked as `absorb`'s when the
/// kept house has a different skill of that name, kept in `clashes`.
/// Empty for a skill both houses have alike.
fn renamed_skills(
    mine: &BTreeMap<String, Entry>,
    theirs: &BTreeMap<String, Entry>,
    absorb: &str,
    clashes: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut renamed = BTreeMap::new();
    for path in theirs.keys() {
        let Some((skill, _)) = path
            .strip_prefix("skills/")
            .and_then(|path| path.split_once('/'))
        else {
            continue;
        };
        if renamed.contains_key(skill) {
            continue;
        }
        let prefix = format!("skills/{skill}/");
        let own = mine
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .collect::<Vec<_>>();
        let other = theirs
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .collect::<Vec<_>>();
        let same = own.len() == other.len()
            && other.iter().all(|(path, entry)| {
                mine.get(*path)
                    .is_some_and(|file| file.bytes == entry.bytes)
            });
        let name = if same {
            String::new()
        } else if own.is_empty() {
            skill.into()
        } else {
            let name = format!("{skill}-from-{absorb}");
            clashes.push(format!(
                "skill `{skill}` (the absorbed one is `{name}`)"
            ));
            name
        };
        renamed.insert(skill.to_string(), name);
    }
    renamed
}

fn renamed_skill(bytes: &[u8], old: &str, name: &str) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let mut named = false;
    text.replace(
        &format!("skills/{old}/"),
        &format!("skills/{name}/"),
    )
    .split('\n')
    .map(|line| {
        if !named
            && let Some((key, value)) = line.split_once(':')
            && key.trim() == "name"
            && value.trim().trim_matches('"') == old
        {
            named = true;
            format!("name: {name}")
        } else {
            line.into()
        }
    })
    .collect::<Vec<_>>()
    .join("\n")
    .into_bytes()
}

fn commit(
    service: &mut Service,
    id: &OperationId,
    request: &Request,
    plan: &Plan,
) -> Result<Value> {
    super::once(service.home.store(), id, request, |db| {
        super::leaving::guards(db, &plan.operation.hamlet)?;
        let mut operation = plan.operation.clone();
        operation.at = crate::clock::now();
        if let Change::Depart { name, .. } = &operation.change {
            let current = record::read(db, &operation.hamlet)?;
            if !current.as_ref().is_some_and(|record| {
                record.named(name).is_some_and(|agent| {
                    agent.number == plan.gone.number
                }) && record
                    .agents
                    .iter()
                    .any(|agent| agent.number == plan.kept.number)
            }) {
                return Ok(outcome(
                    "The agents changed while their merge was \
                     prepared. No replacement agent was merged.",
                    true,
                ));
            }
        }
        match crate::operation::change(db, &operation)? {
            Outcome::Refused(refusal) => Ok(outcome(
                &refusal.text(record::Audience::Agent),
                true,
            )),
            Outcome::Done(RecordResult::Departed { archive }) => {
                super::keep(
                    db,
                    "merge",
                    &format!("{}:{archive}", plan.operation.hamlet),
                    plan,
                )?;
                let r = record::read(db, &plan.operation.hamlet)?
                    .context("the merged hamlet has no record")?;
                let entry = r
                    .archive
                    .iter()
                    .find(|a| a.id == archive)
                    .expect("merge archive");
                let merge =
                    entry.merge.as_ref().expect("merge manifest");
                let mut text = format!(
                    "Merged `{}` into `{}`. Their notes were {}.",
                    entry.agent.name,
                    merge.kept.name,
                    if plan.prompt.is_some() {
                        "rewritten"
                    } else {
                        "joined, not rewritten"
                    }
                );
                if !merge.files.clashes.is_empty() {
                    text.push_str(&format!(
                        "\nKept apart because they differed:\n{}",
                        merge.files.clashes.join("\n")
                    ));
                }
                let mut value = super::outcome(&text, false);
                value["archive"] = json!(archive);
                Ok(value)
            }
            _ => anyhow::bail!("unexpected merge result"),
        }
    })
}

/// A merge an agent asked for: offered to the merge lane, then
/// committed once the lane has its plan, with its file work back on
/// the lane.
pub(super) async fn operation(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    given: Merge,
) -> Result<Option<Value>> {
    if !service.merges.owns(&caller.id) {
        lane::offer(service, caller, given)?;
        return Ok(None);
    }
    let ready = service.merges.take(&caller.id);
    let key = serde_json::to_string(&(
        caller.id.runtime.as_str(),
        &caller.id.key,
    ))?;
    let mut plan = match super::document::<Option<Plan>>(
        service.home.store().connection(),
        "merge-plan",
        &key,
    )? {
        Some(plan) => plan,
        None => match ready.context("completed merge preparation")? {
            Ok(plan) => {
                super::keep(
                    service.home.store().connection(),
                    "merge-plan",
                    &key,
                    &plan,
                )?;
                plan
            }
            Err(unplanned) => {
                lane::release(service, &caller.id);
                return Ok(Some(outcome(&unplanned.text(), true)));
            }
        },
    };
    if let Some(prompt) = &plan.prompt {
        let Some(answer) = super::errands::run(
            service, harnesses, caller, "notes", prompt,
        )
        .await?
        else {
            return Ok(None);
        };
        let limit = service
            .limits_of(&plan.kept.hamlet)?
            .leaving
            .merged_notes as usize;
        if answer.trim().starts_with("# Notes")
            && answer.chars().count() <= limit
            && let Change::Depart {
                how: Departure::Merged { files, notes, .. },
                ..
            } = &mut plan.operation.change
        {
            files.notes_after = answer;
            *notes = MergeNotes::Rewritten;
        }
    }
    let result = commit(service, &caller.id, &caller.request, &plan)?;
    if result["error"] != true {
        crate::chats::stop_agent(service, harnesses, &plan.kept)
            .await?;
        crate::chats::stop_agent(service, harnesses, &plan.gone)
            .await?;
        super::leaving::lapse(
            service.home.store().connection(),
            &plan.gone.hamlet,
            plan.gone.number,
        )?;
        if lane::apply_call(
            service,
            &caller.id,
            &plan.kept.hamlet,
            result["archive"].as_str().expect("merge archive"),
        )? {
            return Ok(None);
        }
    }
    lane::release(service, &caller.id);
    Ok(Some(result))
}

/// A merge's file work on its kept house: the files its snapshot
/// brings and its notes, or their undoing once the absorbed agent is
/// back.
pub(super) struct Applying {
    merge: Applied,
    root: std::fs::File,
    house: Files,
}

/// A merge whose file work ran: its archive in its hamlet and the
/// snapshot its plan kept.
pub(super) struct Applied {
    hamlet: String,
    entry: record::Archived,
    snapshot: String,
}

impl Applying {
    /// The work of the merge archived as `entry` in `hamlet`, whose
    /// plan kept `snapshot`.
    fn new(
        service: &Service,
        hamlet: &str,
        entry: record::Archived,
        snapshot: String,
    ) -> Result<Self> {
        let kept =
            entry.merge.as_ref().expect("merge manifest").kept.number;
        Ok(Self {
            root: service.houses.root().try_clone()?,
            house: service.houses.house(hamlet, kept)?,
            merge: Applied {
                hamlet: hamlet.into(),
                entry,
                snapshot,
            },
        })
    }

    /// Does the file work.
    async fn run(&self) -> Result<()> {
        let notes = async || {
            let bytes = self.house.read_file(NOTES, CAP).await?;
            anyhow::Ok(String::from_utf8(bytes)?)
        };
        let merge =
            self.merge.entry.merge.as_ref().expect("merge manifest");
        crate::fault::point("after_merge_record");
        if self.merge.entry.back.is_none() {
            let files = tree::read_snapshot(
                &self.root,
                &self.merge.snapshot,
                CAP * 2,
                4096,
            )?;
            for (path, file) in &files {
                self.house.put(path, file).await?;
                crate::fault::point("during_merge_files");
            }
            let current = notes().await?;
            if current == merge.files.notes_before {
                self.house
                    .replace(
                        NOTES,
                        merge.files.notes_after.as_bytes(),
                    )
                    .await?;
                crate::fault::point("after_merge_notes");
            } else {
                anyhow::ensure!(
                    current == merge.files.notes_after,
                    "The kept notes changed before the merge \
                     finished; inspect both houses."
                );
            }
        } else {
            let present = self.house.read(CAP, 4096).await?;
            for file in &merge.files.brought {
                if present.get(&file.path).is_some_and(|entry| {
                    format!("{:x}", Sha256::digest(&entry.bytes))
                        == file.sha256
                }) {
                    self.house.remove(&file.path, false).await?;
                }
            }
            if notes().await? == merge.files.notes_after {
                self.house
                    .replace(
                        NOTES,
                        merge.files.notes_before.as_bytes(),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    /// The merge, once its file work ran.
    pub(super) fn done(self) -> Applied {
        self.merge
    }
}

impl Applied {
    /// The society document key of the merge's attempt.
    fn key(&self) -> String {
        format!("{}:{}", self.hamlet, self.entry.id)
    }

    /// Marks the merge applied, unless its record changed while the
    /// work ran, and lets go of its attempt and its snapshot.
    fn applied(self, service: &mut Service) -> Result<()> {
        let record = service
            .home
            .store()
            .record(&self.hamlet)?
            .context("merge record")?;
        let entry = record
            .archive
            .iter()
            .find(|entry| entry.id == self.entry.id)
            .context("merge archive")?;
        anyhow::ensure!(
            entry.back == self.entry.back
                && entry.merge == self.entry.merge,
            "The merge changed while its files were being applied; \
             inspect both houses."
        );
        let operation = RecordOperation {
            hamlet: self.hamlet.clone(),
            actor: Actor::Realm,
            at: crate::clock::now(),
            reason: String::new(),
            task: None,
            change: Change::FinishMerge {
                archive: self.entry.id.clone(),
            },
        };
        let db = service.home.store().connection();
        if let Outcome::Refused(refusal) =
            crate::operation::change(db, &operation)?
        {
            anyhow::bail!(
                "{}",
                refusal.text(record::Audience::Chief)
            );
        }
        let_go(service, &self.hamlet, &self.entry.id, &self.snapshot)
    }
}

/// Lets go of the applied merge archived as `archive` in `hamlet`:
/// its attempt and its snapshot.
fn let_go(
    service: &mut Service,
    hamlet: &str,
    archive: &str,
    snapshot: &str,
) -> Result<()> {
    service.home.store().connection().execute(
        "DELETE FROM society_document WHERE \
         kind='remote-merge-attempt' AND key=?1",
        [format!("{hamlet}:{archive}")],
    )?;
    tree::erase_snapshot(service.houses.root(), snapshot)
}

/// Finishes the file work of every merge whose record is kept and
/// whose files are not applied yet, and lets go of what applied ones
/// kept.
pub(crate) async fn finish(service: &mut Service) -> Result<()> {
    super::library::retire(service)?;
    for r in
        crate::service::records(service.home.store().connection())?
    {
        for entry in &r.archive {
            let Some(merge) = &entry.merge else {
                continue;
            };
            let key = format!("{}:{}", r.id, entry.id);
            let Some(plan) = super::document::<Option<Plan>>(
                service.home.store().connection(),
                "merge",
                &key,
            )?
            else {
                continue;
            };
            if merge.applied {
                let_go(service, &r.id, &entry.id, &plan.snapshot)?;
                continue;
            }
            // An attempt a crash cut short is replayed only where
            // every house change is known to have happened or not.
            if !service.houses.confirms() {
                if super::document::<Option<bool>>(
                    service.home.store().connection(),
                    "remote-merge-attempt",
                    &key,
                )?
                .is_some()
                {
                    continue;
                }
                super::keep(
                    service.home.store().connection(),
                    "remote-merge-attempt",
                    &key,
                    &true,
                )?;
            }
            let work = Applying::new(
                service,
                &r.id,
                entry.clone(),
                plan.snapshot,
            )?;
            work.run().await?;
            work.done().applied(service)?;
        }
    }
    let kept = service
        .home
        .store()
        .connection()
        .prepare(
            "SELECT json_extract(value,'$.snapshot') FROM \
             society_document WHERE kind IN \
             ('merge','merge-plan','library-consult') AND \
             json_type(value,'$.snapshot')='text'",
        )?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    tree::sweep_snapshots(service.houses.root(), &kept)?;
    Ok(())
}
