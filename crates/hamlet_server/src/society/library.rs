//! Durable librarian consultations and shared skills.

use anyhow::{Context, Result};
use hamlet_protocol::client::LibrarySkill;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Caller, Deed, outcome};
use crate::harness::Harnesses;
use crate::papers::{NOTES, tree};
use crate::prompt::shelf;
use crate::protocol::Refusal;
use crate::service::Service;

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

fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
        })
}

/// A librarian consultation's next step.
enum Step {
    /// The consultation goes on.
    Next,
    /// The call is answered with this outcome, or waits for `None`.
    Answer(Option<Value>),
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
) -> Result<Option<Value>> {
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
        return Ok(Some(outcome(result, false)));
    }
    if job.prompt.is_empty() {
        if let Step::Answer(answer) =
            begin(service, caller, skill, text, &key, &mut job)
                .await?
        {
            return Ok(answer);
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
    Ok(Some(outcome(&result, false)))
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
        return Ok(Step::Answer(Some(outcome(
            "Say what you are looking for or why the skill would \
             help others.",
            true,
        ))));
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
        if let Step::Answer(answer) =
            offered(service, caller, name, key, text, &library, job)
                .await?
        {
            return Ok(Step::Answer(answer));
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
    let notes =
        tree::directory(service.houses.root(), "librarian", false)
            .and_then(|dir| {
                tree::read_file(&dir, NOTES, 4 * 1024 * 1024)
            })
            .ok()
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
    let refuse =
        |text: &str| Ok(Step::Answer(Some(outcome(text, true))));
    if !valid(name) {
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
        return Ok(Step::Answer(None));
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
) -> Result<std::collections::BTreeMap<String, tree::Entry>> {
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
        let library =
            tree::directory(service.houses.root(), "library", false)?;
        let staging = tree::directory(
            service.houses.root(),
            ".library-staging",
            true,
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
    files: &std::collections::BTreeMap<String, tree::Entry>,
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
    if !valid(name) {
        return Ok(Err(Refusal::Invalid(
            "usage: /unshelve <skill>".into(),
        )));
    }
    if !views(service)?.iter().any(|s| s.name == name) {
        return Ok(Err(Refusal::Invalid(format!(
            "the library has no skill named `{name}`"
        ))));
    }
    let library =
        tree::directory(service.houses.root(), "library", false)?;
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
