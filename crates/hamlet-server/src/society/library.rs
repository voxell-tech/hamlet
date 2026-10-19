//! Durable librarian consultations and shared skills.

use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Caller, Deed, outcome};
use crate::harness::Harnesses;
use crate::papers::tree;
use crate::prompt::shelf;
use crate::protocol::Refusal;
use crate::service::Service;

const CAP: u64 = 4 * 1024 * 1024;

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

pub(super) async fn consult(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    tool: &str,
    args: &Value,
) -> Result<Option<Value>> {
    retire(service)?;
    let key = serde_json::to_string(&(
        caller.id.runtime.as_str(),
        &caller.id.key,
    ))?;
    let mut job = super::document::<Consult>(
        service.home.store().connection(),
        "library-consult",
        &key,
    )?;
    if let Some(result) = &job.result {
        if let Some(snapshot) = &job.snapshot {
            tree::erase(
                &tree::directory(
                    service.houses.root(),
                    ".society-files",
                    false,
                )?,
                snapshot,
            )?;
        }
        return Ok(Some(outcome(result, false)));
    }
    if job.prompt.is_empty() {
        let text = args[if tool == "submit_skill" {
            "reason"
        } else {
            "question"
        }]
        .as_str()
        .unwrap_or_default()
        .trim();
        if text.is_empty() {
            return Ok(Some(outcome(
                "Say what you are looking for or why the skill \
                 would help others.",
                true,
            )));
        }
        let record = service
            .home
            .store()
            .record(&caller.place.agent.hamlet)?
            .expect("caller's record");
        job.by = format!(
            "{}@{}",
            record.name_of(
                caller.place.agent.number,
                crate::record::NameFallback::Recorded
            ),
            record.id
        );
        let library = shelf::library(
            &service.home.layout().root().join("library"),
        )
        .unwrap_or_default();
        if tool == "submit_skill" {
            let name = args["skill"].as_str().unwrap_or_default();
            if !valid(name) {
                return Ok(Some(outcome(
                    "`skill` is its folder under skills/, in \
                     letters, digits, hyphens and underscores.",
                    true,
                )));
            }
            if views(service)?.iter().any(|s| s["name"] == name) {
                return Ok(Some(outcome(
                    &format!(
                        "the library already has a skill named \
                         `{name}`. Use that one, or submit yours \
                         under another name if it does something \
                         else"
                    ),
                    true,
                )));
            }
            let db = service.home.store().connection();
            let claimed = db.query_row(
                "SELECT EXISTS (SELECT 1 FROM society_document \
                 WHERE kind='library-consult' AND key != ?1 AND \
                 json_extract(value,'$.skill')=?2 AND \
                 json_extract(value,'$.result') IS NULL)",
                params![key, name],
                |row| row.get::<_, bool>(0),
            )?;
            if claimed {
                return Ok(Some(outcome(
                    "Another offer of that skill is being judged.",
                    true,
                )));
            }
            let house = service.houses.house(
                &record.id,
                caller.place.agent.number,
                false,
            )?;
            let skills = tree::directory(&house, "skills", false)?;
            let files = tree::read(
                &tree::directory(&skills, name, false)?,
                CAP,
                1024,
            )?;
            let paper = files
                .get("SKILL.md")
                .map(|f| {
                    String::from_utf8_lossy(&f.bytes).into_owned()
                })
                .unwrap_or_default();
            let skill = shelf::skill(name.into(), &paper);
            if skill.name.trim().is_empty()
                || skill.description.trim().is_empty()
            {
                return Ok(Some(outcome(
                    &format!(
                        "`{name}` has no SKILL.md that opens with a \
                         name and a description"
                    ),
                    true,
                )));
            }
            job.prompt = format!(
                "You are the librarian. {} offers a skill: {text}. \
                 Take it only if useful to others, distinct from \
                 the shelves, clear enough to run, and free of \
                 private names, keys or \
                 paths.\n\n{library}\n\nOffered files: \
                 {}\n{}\nAnswer with ACCEPT or REFUSE as your first \
                 word, then explain why.",
                job.by,
                files.keys().cloned().collect::<Vec<_>>().join(", "),
                crate::papers::cut(&paper, 6000, "")
            );
            job.skill = Some(name.into());
            job.snapshot =
                Some(tree::snapshot(service.houses.root(), &files)?);
        } else {
            job.prompt = format!(
                "You are the librarian. {} asks: \
                 {text}\n\n{library}\nName the skills that do it \
                 and their exact commands. Do not invent skills. If \
                 nothing fits, say so.",
                job.by
            );
        }
        let notes = tree::directory(
            service.houses.root(),
            "librarian",
            false,
        )
        .and_then(|dir| {
            tree::read_file(&dir, "NOTES.md", 4 * 1024 * 1024)
        })
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
        job.prompt = format!("{notes}\n\n{}", job.prompt);
        super::keep(
            service.home.store().connection(),
            "library-consult",
            &key,
            &job,
        )?;
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
    let result = if let Some(name) = &job.skill {
        let snapshots = tree::directory(
            service.houses.root(),
            ".society-files",
            false,
        )?;
        let files = tree::read(
            &tree::directory(
                &snapshots,
                job.snapshot
                    .as_deref()
                    .expect("offered skill snapshot"),
                false,
            )?,
            CAP,
            1024,
        )?;
        let accepted =
            answer.split_whitespace().next().is_some_and(|first| {
                first.eq_ignore_ascii_case("ACCEPT")
            });
        if accepted {
            let library = tree::directory(
                service.houses.root(),
                "library",
                false,
            )?;
            if !job.delivered {
                let staging = tree::directory(
                    service.houses.root(),
                    ".library-staging",
                    true,
                )?;
                tree::publish(&staging, &library, name, &files)?;
                crate::fault::point("after_library_publish");
                job.delivered = true;
                let tx = service.home.store().db.transaction()?;
                super::keep(&tx, "library-consult", &key, &job)?;
                super::keep(
                    &tx,
                    "library-shelved",
                    name,
                    &json!({"by":job.by,"reason":answer,"at":super::now()}),
                )?;
                tx.commit()?;
            }
            let house = service.houses.house(
                &caller.place.agent.hamlet,
                caller.place.agent.number,
                false,
            )?;
            let skills = tree::directory(&house, "skills", false)?;
            let unchanged = tree::directory(&skills, name, false)
                .and_then(|dir| tree::read(&dir, CAP, 1024))
                .is_ok_and(|present| present == files);
            if unchanged {
                tree::erase(&skills, name)?;
            }
            format!(
                "The librarian took `{name}`: {answer} It is in the \
                 library now. {} Use the library's, as every agent \
                 can.",
                if unchanged {
                    "It is no longer in your house."
                } else {
                    "Later changes stayed in your house."
                }
            )
        } else {
            format!(
                "The librarian did not take `{name}`: {answer} It \
                 stays in your house."
            )
        }
    } else {
        answer
    };
    job.result = Some(result.clone());
    super::keep(
        service.home.store().connection(),
        "library-consult",
        &key,
        &job,
    )?;
    if let Some(snapshot) = &job.snapshot {
        tree::erase(
            &tree::directory(
                service.houses.root(),
                ".society-files",
                false,
            )?,
            snapshot,
        )?;
    }
    Ok(Some(outcome(&result, false)))
}

pub(super) fn retire(service: &mut Service) -> Result<()> {
    let jobs = service
        .home
        .store()
        .connection()
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
            let active =
                service.home.store().connection().query_row(
                    "SELECT EXISTS(SELECT 1 FROM gate_call g JOIN \
                     harness_binding b ON b.id=g.binding JOIN \
                     harness_input i ON i.binding=g.binding AND \
                     i.submission=g.input WHERE g.runtime=?1 AND \
                     g.key=?2 AND b.cancelling=0 AND \
                     i.state!='dropped' AND \
                     coalesce(i.status,'queued') NOT IN \
                     ('done','unanswered'))",
                    params![runtime, call],
                    |row| row.get::<_, bool>(0),
                )?;
            if active {
                continue;
            }
            job.result = Some(
                "The librarian's caller stopped before the offer \
                 finished."
                    .into(),
            );
            let bindings = service
                .home
                .store()
                .connection()
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
            super::keep(
                service.home.store().connection(),
                "library-consult",
                &key,
                &job,
            )?;
        }
        if let Some(snapshot) = job.snapshot {
            tree::erase(
                &tree::directory(
                    service.houses.root(),
                    ".society-files",
                    false,
                )?,
                &snapshot,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn consulting(
    db: &Connection,
    agent: &crate::record::AgentRef,
) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS (SELECT 1 FROM society_binding s JOIN \
         harness_binding b ON b.id=s.binding JOIN harness_input i \
         ON i.binding=b.id WHERE s.errand='librarian' AND \
         b.hamlet=?1 AND b.number=?2 AND \
         coalesce(i.status,'queued') NOT IN ('done','unanswered'))",
        params![agent.hamlet, i64::try_from(agent.number)?],
        |row| row.get(0),
    )?)
}

pub(crate) fn views(service: &mut Service) -> Result<Vec<Value>> {
    let found =
        shelf::shelf(&service.home.layout().root().join("library"));
    found.into_iter().map(|skill| {
        let shelved = super::document::<Value>(service.home.store().connection(), "library-shelved", &skill.folder)?;
        Ok(json!({"name":skill.folder,"description":skill.description,"by":shelved["by"].as_str().unwrap_or("Hamlet")}))
    }).collect()
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
    if !views(service)?.iter().any(|s| s["name"] == name) {
        return Ok(Err(Refusal::Invalid(format!(
            "the library has no skill named `{name}`"
        ))));
    }
    let library =
        tree::directory(service.houses.root(), "library", false)?;
    tree::erase(&library, name)?;
    let chief = service.chief.clone();
    super::keep(
        service.home.store().connection(),
        "library-unshelved",
        name,
        &json!({"by":chief,"reason":rest,"at":super::now()}),
    )?;
    Ok(Ok(format!("Took `{name}` out of the library.")))
}
