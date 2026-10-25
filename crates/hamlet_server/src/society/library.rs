//! Durable librarian consultations and shared skills.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use hamlet_os::fs::{Missing, Mode, TreePath};
use hamlet_protocol::client::{LibrarySkill, TakeSkill};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Caller, Deed, SocietyOutcome};
use crate::harness::Harnesses;
use crate::operation::Request;
use crate::papers::{NOTES, tree};
use crate::prompt::shelf;
use crate::protocol::{Refusable, Refusal};
use crate::service::Service;
use crate::{OperationId, RuntimeInstance};

const CAP: u64 = 4 * 1024 * 1024;

/// Who shelved a skill in the library, why the librarian took it, and
/// when.
#[derive(Serialize, Deserialize)]
struct Shelved {
    by: String,
    reason: String,
    at: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct Consult {
    skill: Option<String>,
    snapshot: Option<String>,
    by: String,
    prompt: String,
    result: Option<String>,
    delivered: bool,
}

pub(crate) fn is_valid(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
        })
}

/// A librarian consultation's next step.
enum Step {
    /// The consultation goes on.
    Next,
    /// The call fails, saying why.
    Failed(String),
    /// The call waits.
    Waiting,
}

/// The arguments of `submit_skill`.
#[derive(Deserialize)]
pub(crate) struct SubmitSkill {
    pub skill: String,
    pub reason: String,
}

/// The arguments of `ask_librarian`.
#[derive(Deserialize)]
pub(crate) struct AskLibrarian {
    pub question: String,
}

/// The `submit_skill` or `ask_librarian` call of `caller`, which
/// offers `skill` or asks, saying why in `text`: the librarian's
/// answer, once its errand gives it, and for an offer the skill
/// shelved and taken out of the caller's house; `None` while it
/// waits.
pub(super) async fn consult(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    skill: Option<&str>,
    text: &str,
) -> Result<Option<SocietyOutcome>> {
    retire(service)?;
    let key = serde_json::to_string(&(
        caller.id.runtime.as_str(),
        &caller.id.key,
    ))?;
    let mut job = super::document::<Consult>(
        service.db(),
        "library-consult",
        &key,
    )?;
    if let Some(result) = &job.result {
        if let Some(snapshot) = &job.snapshot {
            tree::erase_snapshot(service.houses.root(), snapshot)?;
        }
        return Ok(Some(SocietyOutcome::answered(result)));
    }
    if job.prompt.is_empty() {
        match begin(service, caller, skill, text, &key, &mut job)
            .await?
        {
            Step::Next => {}
            Step::Failed(text) => {
                return Ok(Some(SocietyOutcome::failed(text)));
            }
            Step::Waiting => return Ok(None),
        }
        super::keep(service.db(), "library-consult", &key, &job)?;
    }
    let Some(answer) = super::errands::run(
        service,
        harnesses,
        caller,
        "librarian",
        &job.prompt,
    )
    .await?
    else {
        return Ok(None);
    };
    let result = match job.skill.clone() {
        Some(name) => {
            match judged(
                service, caller, &key, &mut job, &name, &answer,
            )
            .await?
            {
                Some(result) => result,
                None => return Ok(None),
            }
        }
        None => answer,
    };
    job.result = Some(result.clone());
    super::keep(service.db(), "library-consult", &key, &job)?;
    if let Some(snapshot) = &job.snapshot {
        tree::erase_snapshot(service.houses.root(), snapshot)?;
    }
    Ok(Some(SocietyOutcome::answered(result)))
}

/// Writes the librarian's prompt for the call of `caller` into `job`:
/// its question, or the skill it offers, whose files are kept in a
/// snapshot.
async fn begin(
    service: &mut Service,
    caller: &Caller<'_>,
    skill: Option<&str>,
    text: &str,
    key: &str,
    job: &mut Consult,
) -> Result<Step> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Step::Failed(
            "Say what you are looking for or why the skill would \
             help others."
                .into(),
        ));
    }
    let record = service
        .home
        .store()
        .record(&caller.place.agent.hamlet)?
        .context("the caller's hamlet has no record")?;
    job.by = format!(
        "{}@{}",
        record.name_of(
            caller.place.agent.number,
            crate::record::NameFallback::Recorded
        ),
        record.id
    );
    let library =
        shelf::library(&service.home.layout().root().join("library"))
            .unwrap_or_default();
    if let Some(name) = skill {
        if let step @ (Step::Failed(_) | Step::Waiting) =
            offered(service, caller, name, key, text, &library, job)
                .await?
        {
            return Ok(step);
        }
    } else {
        job.prompt = format!(
            "You are the librarian. {} asks: \
             {text}\n\n{library}\nName the skills that do it and \
             their exact commands. Do not invent skills. If nothing \
             fits, say so.",
            job.by
        );
    }
    let notes = tree::directory(
        service.houses.root(),
        "librarian",
        Missing::Fail,
    )
    .ok()
    .and_then(|dir| {
        let path = TreePath::parse(NOTES).ok()?;
        tree::read_file(&dir, &path, 4 * 1024 * 1024).ok()
    })
    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    .unwrap_or_default();
    job.prompt = format!("{notes}\n\n{}", job.prompt);
    Ok(Step::Next)
}

/// Writes the librarian's prompt for the skill the call of `caller`
/// offers into `job`, once its files are read from the caller's
/// house.
async fn offered(
    service: &mut Service,
    caller: &Caller<'_>,
    name: &str,
    key: &str,
    text: &str,
    library: &str,
    job: &mut Consult,
) -> Result<Step> {
    let refuse = |text: &str| Ok(Step::Failed(text.into()));
    if !is_valid(name) {
        return refuse(
            "`skill` is its folder under skills/, in letters, \
             digits, hyphens and underscores.",
        );
    }
    if views(service)?.iter().any(|s| s.name == name) {
        return refuse(&format!(
            "the library already has a skill named `{name}`. Use \
             that one, or submit yours under another name if it \
             does something else"
        ));
    }
    let claimed = service.db().query_row(
        "SELECT EXISTS (SELECT 1 FROM society_document WHERE \
         kind='library-consult' AND key != ?1 AND \
         json_extract(value,'$.skill')=?2 AND \
         json_extract(value,'$.result') IS NULL)",
        params![key, name],
        |row| row.get::<_, bool>(0),
    )?;
    if claimed {
        return refuse(
            "Another offer of that skill is being judged.",
        );
    }
    let Some(files) = service.house_work.take_offer(&caller.id)
    else {
        let house = service.houses.house(
            &caller.place.agent.hamlet,
            caller.place.agent.number,
        )?;
        let name = name.to_owned();
        service.house_work.offer(caller.id.clone(), async move {
            skill_files(&house, &name).await
        });
        return Ok(Step::Waiting);
    };
    let files = files?;
    let paper = files
        .get("SKILL.md")
        .map(|f| String::from_utf8_lossy(&f.bytes).into_owned())
        .unwrap_or_default();
    let skill = shelf::skill(name.into(), &paper);
    if skill.name.trim().is_empty()
        || skill.description.trim().is_empty()
    {
        return refuse(&format!(
            "`{name}` has no SKILL.md that opens with a name and a \
             description"
        ));
    }
    job.prompt = format!(
        "You are the librarian. {} offers a skill: {text}. Take it \
         only if useful to others, distinct from the shelves, clear \
         enough to run, and free of private names, keys or \
         paths.\n\n{library}\n\nOffered files: {}\n{}\nAnswer with \
         ACCEPT or REFUSE as your first word, then explain why.",
        job.by,
        files.keys().cloned().collect::<Vec<_>>().join(", "),
        crate::papers::cut(&paper, 6000, "")
    );
    job.skill = Some(name.into());
    job.snapshot =
        Some(tree::snapshot(service.houses.root(), &files)?);
    Ok(Step::Next)
}

/// The files of the skill `name` in `house`.
async fn skill_files(
    house: &crate::papers::files::Files,
    name: &str,
) -> Result<BTreeMap<String, tree::Entry>> {
    house
        .directory("skills", false)
        .await?
        .directory(name, false)
        .await?
        .read(CAP, 1024)
        .await
}

/// The words of the librarian's `answer` on the skill `name` offered
/// by `caller`: once taken, the skill is shelved and taken out of the
/// caller's house. `None` while the house is being cleaned.
async fn judged(
    service: &mut Service,
    caller: &Caller<'_>,
    key: &str,
    job: &mut Consult,
    name: &str,
    answer: &str,
) -> Result<Option<String>> {
    let accepted = answer
        .split_whitespace()
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case("ACCEPT"));
    if !accepted {
        return Ok(Some(format!(
            "The librarian did not take `{name}`: {answer} It stays \
             in your house."
        )));
    }
    let files = tree::read_snapshot(
        service.houses.root(),
        job.snapshot.as_deref().with_context(|| {
            format!("the offer of `{name}` kept no snapshot")
        })?,
        CAP,
        1024,
    )?;
    if !job.delivered {
        let library = tree::directory(
            service.houses.root(),
            "library",
            Missing::Fail,
        )?;
        let staging = tree::directory(
            service.houses.root(),
            ".library-staging",
            Missing::Make(Mode::FOLDER),
        )?;
        // Kept before the skill is published, so that every skill in
        // the library has its record, even after a crash between the
        // two.
        super::keep(
            service.db(),
            "library-shelved",
            name,
            &Shelved {
                by: job.by.clone(),
                reason: answer.into(),
                at: crate::clock::now(),
            },
        )?;
        tree::publish(&staging, &library, name, &files)?;
        crate::fault::point("after_library_publish");
        job.delivered = true;
        super::keep(service.db(), "library-consult", key, &job)?;
    }
    let Some(unchanged) = service.house_work.take_cleaned(&caller.id)
    else {
        let house = service.houses.house(
            &caller.place.agent.hamlet,
            caller.place.agent.number,
        )?;
        let name = name.to_owned();
        service.house_work.clean(caller.id.clone(), async move {
            clean(house, &name, &files).await
        });
        return Ok(None);
    };
    let unchanged = unchanged?;
    Ok(Some(format!(
        "The librarian took `{name}`: {answer} It is in the library \
         now. {} Use the library's, as every agent can.",
        if unchanged {
            "It is no longer in your house."
        } else {
            "Later changes stayed in your house."
        }
    )))
}

async fn clean(
    house: crate::papers::files::Files,
    name: &str,
    files: &BTreeMap<String, tree::Entry>,
) -> Result<bool> {
    let skills = house.directory("skills", false).await?;
    let unchanged = match skills.directory(name, false).await {
        Ok(dir) => dir
            .read(CAP, 1024)
            .await
            .is_ok_and(|present| present == *files),
        Err(_) => false,
    };
    if unchanged {
        skills.remove(name, true).await?;
    }
    Ok(unchanged)
}

pub(super) fn retire(service: &mut Service) -> Result<()> {
    let jobs = service
        .db()
        .prepare(
            "SELECT key,value FROM society_document WHERE \
             kind='library-consult'",
        )?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (key, value) in jobs {
        let mut job = serde_json::from_str::<Consult>(&value)?;
        if job.result.is_none() {
            let (runtime, call) =
                serde_json::from_str::<(String, String)>(&key)?;
            if crate::oversight::held::unfinished(
                service.db(),
                &runtime,
                &call,
            )? {
                continue;
            }
            job.result = Some(
                "The librarian's caller stopped before the offer \
                 finished."
                    .into(),
            );
            let bindings = service
                .db()
                .prepare(
                    "SELECT binding FROM society_binding WHERE \
                     errand='librarian' AND \
                     json_extract(owner,'$.runtime')=?1 AND \
                     json_extract(owner,'$.key')=?2",
                )?
                .query_map(params![runtime, call], |row| {
                    Ok(crate::harness::BindingId(row.get(0)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for binding in bindings {
                service.home.store().begin_cancel(binding)?;
            }
            super::keep(service.db(), "library-consult", &key, &job)?;
        }
        if let Some(snapshot) = job.snapshot {
            tree::erase_snapshot(service.houses.root(), &snapshot)?;
        }
    }
    Ok(())
}

pub(crate) fn consulting(
    db: &Connection,
    agent: &crate::record::AgentRef,
) -> Result<bool> {
    Ok(db.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM society_binding s JOIN \
             harness_binding b ON b.id=s.binding JOIN harness_input \
             i ON i.binding=b.id WHERE s.errand='librarian' AND \
             b.hamlet=?1 AND b.number=?2 AND NOT {})",
            crate::harness::ended("i.status")
        ),
        params![agent.hamlet, i64::try_from(agent.number)?],
        |row| row.get(0),
    )?)
}

pub(crate) fn views(
    service: &mut Service,
) -> Result<Vec<LibrarySkill>> {
    let found =
        shelf::shelf(&service.home.layout().root().join("library"));
    let db = service.db();
    found
        .into_iter()
        .map(|skill| {
            let shelved = super::document::<Option<Shelved>>(
                db,
                "library-shelved",
                &skill.folder,
            )?;
            // A shelved skill's record is kept before it is
            // published, so a skill without one was stocked by Hamlet
            // or put in the library by hand.
            let by = match shelved {
                Some(shelved) => shelved.by,
                None if crate::starters::names()
                    .any(|name| name == skill.folder) =>
                {
                    "Hamlet".into()
                }
                None => "added by hand".into(),
            };
            Ok(LibrarySkill {
                by,
                description: skill.description,
                name: skill.folder,
            })
        })
        .collect()
}

pub(super) fn unshelve(
    service: &mut Service,
    rest: &str,
) -> Result<Deed> {
    let name = rest.split_whitespace().next().unwrap_or_default();
    if !is_valid(name) {
        return Ok(Err(Refusal::Usage(
            "usage: /unshelve <skill>".into(),
        )));
    }
    if !views(service)?.iter().any(|s| s.name == name) {
        return Ok(Err(Refusal::Invalid(format!(
            "the library has no skill named `{name}`"
        ))));
    }
    let library = tree::directory(
        service.houses.root(),
        "library",
        Missing::Fail,
    )?;
    tree::erase(&library, name)?;
    let chief = service.chief.clone();
    super::keep(
        service.db(),
        "library-unshelved",
        name,
        &json!({"by":chief,"reason":rest,"at":crate::clock::now()}),
    )?;
    Ok(Ok(format!("Took `{name}` out of the library.")))
}

/// Why the chief's skill was not taken into the library.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error,
)]
pub(crate) enum Untaken {
    #[error(
        "`{0}` cannot name a skill: use letters, digits, hyphens \
         and underscores"
    )]
    Name(String),
    #[error(
        "`{0}` cannot be a file of the skill: give each file once, \
         by a path inside its folder"
    )]
    Path(String),
    #[error(
        "`{0}` has no SKILL.md that opens with a name and a \
         description"
    )]
    Paper(String),
    #[error("the library already has a skill named `{0}`")]
    Shelved(String),
}

/// The chief's `take-skill`: the skill shelved in the library, with
/// its record, as one marked operation of the request.
pub(crate) fn take(
    service: &mut Service,
    take: &TakeSkill,
) -> Result<Refusable<String>> {
    let name = take.name.as_str();
    let id = OperationId {
        runtime: RuntimeInstance::new("chief")?,
        key: take.id.to_string(),
    };
    let request = Request::new("take-skill", take)?;
    let shelved = views(service)?
        .into_iter()
        .map(|skill| skill.name)
        .collect::<Vec<_>>();
    let shelf = Shelved {
        by: service.chief.clone(),
        reason: format!("Brought from {}", take.from),
        at: crate::clock::now(),
    };
    let root = service.houses.root().try_clone()?;
    let outcome = service.home.store().once(&id, &request, |db| {
        let files = match taken(take) {
            Ok(files) => files,
            Err(untaken) => return Ok(Err(untaken)),
        };
        let library = tree::directory(
            &root,
            "library",
            Missing::Make(Mode::FOLDER),
        )?;
        // A request cut short by a crash between the shelving and its
        // marker left its files there with no record: it is taken
        // again.
        let recorded = super::document::<Option<Shelved>>(
            db,
            "library-shelved",
            name,
        )?
        .is_some();
        let unfinished = !recorded
            && tree::directory(&library, name, Missing::Fail)
                .map_err(anyhow::Error::from)
                .and_then(|dir| tree::read(&dir, CAP, 1024))
                .is_ok_and(|present| present == files);
        if shelved.iter().any(|shelf| shelf == name) && !unfinished {
            return Ok(Err(Untaken::Shelved(name.into())));
        }
        // Kept before the skill is published, as an offer the
        // librarian takes is.
        super::keep(db, "library-shelved", name, &shelf)?;
        let staging = tree::directory(
            &root,
            ".library-staging",
            Missing::Make(Mode::FOLDER),
        )?;
        tree::publish(&staging, &library, name, &files)?;
        Ok(Ok(()))
    })?;
    Ok(match outcome {
        None => Err(Refusal::Reused),
        Some(Err(untaken)) => Err(Refusal::Untaken(untaken)),
        Some(Ok(())) => Ok(format!(
            "Took `{name}` into the library. Every agent can use it."
        )),
    })
}

/// The files of the skill `take` brings, by their paths in its
/// folder, once they make a skill.
fn taken(
    take: &TakeSkill,
) -> Result<BTreeMap<String, tree::Entry>, Untaken> {
    let name = take.name.as_str();
    if !is_valid(name) {
        return Err(Untaken::Name(name.into()));
    }
    let mut files = BTreeMap::new();
    for file in &take.files {
        let inside = file.path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.contains(['\\', '\0'])
        });
        let entry = tree::Entry {
            bytes: file.text.as_bytes().to_vec(),
            mode: 0o644,
        };
        if !inside || files.insert(file.path.clone(), entry).is_some()
        {
            return Err(Untaken::Path(file.path.clone()));
        }
    }
    let paper = files
        .get("SKILL.md")
        .map(|f| String::from_utf8_lossy(&f.bytes).into_owned())
        .unwrap_or_default();
    let skill = shelf::skill(name.into(), &paper);
    if skill.name.trim().is_empty()
        || skill.description.trim().is_empty()
    {
        return Err(Untaken::Paper(name.into()));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client::{RequestId, SkillText};
    use hamlet_testing::Scratch;

    use super::*;
    use crate::{Home, Overrides};

    fn skill(id: &RequestId, name: &str, paper: &str) -> TakeSkill {
        TakeSkill {
            files: vec![
                SkillText {
                    path: "SKILL.md".into(),
                    text: paper.into(),
                },
                SkillText {
                    path: "scripts/run.sh".into(),
                    text: "echo hi\n".into(),
                },
            ],
            from: "skills.sh/acme/tools".into(),
            id: id.clone(),
            name: name.into(),
        }
    }

    const PAPER: &str =
        "---\nname: maps\ndescription: Draws maps.\n---\n\nDraw.\n";

    #[test]
    fn a_skill_the_chief_takes_is_shelved_once_with_its_record() {
        let scratch = Scratch::new("library-take");
        let mut service = Service::new(
            Home::open(&scratch).unwrap(),
            Overrides::none(true),
        )
        .unwrap();
        let library = scratch.join("library");
        let first = RequestId::new();
        let taking = skill(&first, "maps", PAPER);
        assert!(take(&mut service, &taking).unwrap().is_ok());
        assert_eq!(
            std::fs::read_to_string(
                library.join("maps/scripts/run.sh")
            )
            .unwrap(),
            "echo hi\n"
        );
        let shelved = views(&mut service).unwrap();
        let maps = shelved.iter().find(|s| s.name == "maps").unwrap();
        assert_eq!(maps.by, service.chief);
        assert_eq!(maps.description, "Draws maps.");
        // The same request again is the same answer; another under
        // its id is refused.
        assert!(take(&mut service, &taking).unwrap().is_ok());
        let changed = skill(&first, "other", PAPER);
        assert!(matches!(
            take(&mut service, &changed).unwrap(),
            Err(Refusal::Reused)
        ));
        // A name the library has, a file outside the skill's folder
        // and a skill with no description are refused, and leave
        // nothing behind.
        let again = skill(&RequestId::new(), "maps", PAPER);
        assert!(matches!(
            take(&mut service, &again).unwrap(),
            Err(Refusal::Untaken(Untaken::Shelved(_)))
        ));
        let mut climbing = skill(&RequestId::new(), "climb", PAPER);
        climbing.files[1].path = "../../escape.sh".into();
        assert!(matches!(
            take(&mut service, &climbing).unwrap(),
            Err(Refusal::Untaken(Untaken::Path(_)))
        ));
        let bare = skill(&RequestId::new(), "bare", "# Nothing\n");
        assert!(matches!(
            take(&mut service, &bare).unwrap(),
            Err(Refusal::Untaken(Untaken::Paper(_)))
        ));
        assert!(!library.join("climb").exists());
        assert!(!library.join("bare").exists());
        assert!(!scratch.join("escape.sh").exists());
    }
}
