//! An instruction's one objection, held until its chief answers.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::chats::records;
use crate::harness::{Answer, Binding};
use crate::oversight::{self, ChatPlace, Raising};
use crate::service::Service;

pub(crate) fn object(
    service: &mut Service,
    binding: &Binding,
    key: &str,
    input: Option<&str>,
    args: &Value,
) -> Result<Answer> {
    if service.home.store().cancelling(binding.id)? {
        return Ok(Answer::Refusal(
            crate::harness::answers::STOPPED_WORDS.into(),
        ));
    }
    let db = service.home.store().connection();
    let ruled = db
        .query_row(
            "SELECT wait,how,text,block FROM gate_call WHERE \
             runtime=?1 AND key=?2",
            params![binding.runtime.as_str(), key],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some((_, _, _, Some(words))) = &ruled {
        return Ok(Answer::Refusal(words.clone()));
    }
    if let Some((Some(_), how, text, _)) = ruled {
        return Ok(match how.as_deref() {
            None => super::letters::waiting(),
            Some("go") => Answer::result(
                format!(
                    "The chief says go ahead: {}",
                    text.unwrap_or_default()
                ),
                false,
            ),
            Some(_) => Answer::result(
                "The chief withdrew the instruction.",
                true,
            ),
        });
    }
    let Some(kept) = records::kept(db, binding.id)? else {
        return Ok(Answer::result(
            "No chief oversees this instruction.",
            true,
        ));
    };
    let Some(request) = input
        .map(|input| {
            service.home.store().request_of(binding.id, input)
        })
        .transpose()?
        .flatten()
    else {
        return Ok(Answer::result(
            "No chief oversees this instruction.",
            true,
        ));
    };
    let db = service.home.store().connection();
    let mark = records::mark_of(db, binding.id, &request)?;
    let Some(mark) = mark.filter(|mark| mark.kind == "hamlet.chief")
    else {
        return Ok(Answer::result(
            "No chief oversees this instruction.",
            true,
        ));
    };
    let task = mark
        .data
        .get("task")
        .and_then(Value::as_str)
        .unwrap_or(&request);
    let owner = format!("{}:{task}", binding.id.0);
    let prior = super::document::<Value>(db, "objection", &owner)?;
    let mut instruction = super::document::<
        super::reviews::Instruction,
    >(db, "instruction-ruling", task)?;
    if !prior.is_null() || instruction.how.is_some() {
        return Ok(Answer::result(
            "You have already objected to this instruction. Do the \
             work the chief confirmed.",
            true,
        ));
    }
    let reason = args["reason"].as_str().unwrap_or_default().trim();
    if reason.is_empty() {
        return Ok(Answer::result("Say why in `reason`.", true));
    }
    let chief = mark
        .data
        .get("chief")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = mark
        .data
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let record = crate::record::read(db, &kept.place.agent.hamlet)?
        .expect("instruction's record");
    let name = record.name_of(
        kept.place.agent.number,
        crate::record::NameFallback::Recorded,
    );
    let tx = service.home.store().transaction()?;
    tx.execute(
        "INSERT INTO gate_call (runtime,key,binding,input) VALUES \
         (?1,?2,?3,?4) ON CONFLICT DO NOTHING",
        params![binding.runtime.as_str(), key, binding.id.0, input],
    )?;
    let wait = oversight::raise(
        &tx,
        Raising {
            kind: "objection",
            danger: None,
            by: name.clone(),
            agent: Some(kept.place.agent.clone()),
            about: format!(
                "{chief} told {name}: {text}\n{name} objects: \
                 {reason}"
            ),
            change: None,
            questions: None,
            answers: vec!["go".into(), "withdraw".into()],
            chat: Some(ChatPlace {
                hamlet: kept.place.agent.hamlet,
                number: kept.place.agent.number,
                chief: chief.into(),
                session: kept.place.session,
            }),
            call: Some((binding.runtime.clone(), key.into())),
        },
        super::now(),
    )?;
    instruction.binding = binding.id.0;
    instruction.waits.extend(wait);
    super::keep(&tx, "instruction-ruling", task, &instruction)?;
    super::keep(&tx, "objection", &owner, &json!({"key":key}))?;
    tx.commit()?;
    Ok(super::letters::waiting())
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::harness::link::ModelRef;
    use crate::record::AgentRef;

    #[test]
    fn a_stopped_objection_stays_refused_after_cancel_acknowledgment()
    {
        let scratch = Scratch::new("stopped-objection");
        let home = crate::Home::open(&scratch).unwrap();
        let mut service = Service::new(home, true).unwrap();
        let store = service.home.store();
        let runtime = store.runtime_for("pi").unwrap();
        let id = store
            .add_binding(
                &AgentRef {
                    hamlet: "home".into(),
                    number: 0,
                },
                false,
                &ModelRef {
                    provider: "faux".into(),
                    id: "faux-1".into(),
                    base_url: None,
                },
                &runtime,
            )
            .unwrap();
        store
            .connection()
            .execute(
                "INSERT INTO gate_call \
                 (runtime,key,binding,input,net) VALUES \
                 (?1,'objection',?2,'input',0)",
                params![runtime.as_str(), id.0],
            )
            .unwrap();
        let binding = store.binding(id).unwrap().unwrap();
        records::keep(
            store.connection(),
            id,
            &records::Place {
                agent: binding.agent.clone(),
                chief: Some("chief".into()),
                session: 1,
            },
            None,
            0,
        )
        .unwrap();
        store.connection().execute(
            "INSERT INTO harness_input (binding,request,ordinal,content,steer,state,submission,mark) VALUES (?1,'request',1,'Work',0,'admitted','input',?2)",
            params![id.0, json!({"kind":"hamlet.chief", "data":{"task":"task", "chief":"chief", "text":"Work"}}).to_string()],
        ).unwrap();
        store.begin_cancel(id).unwrap();
        for acknowledged in [false, true] {
            if acknowledged {
                service.home.store().cancelled(id).unwrap();
            }
            let answer = object(
                &mut service,
                &binding,
                "objection",
                Some("input"),
                &json!({"reason": "This work should wait."}),
            )
            .unwrap();
            assert!(
                matches!(answer, Answer::Refusal(_)),
                "{answer:?}"
            );
        }
    }
}
