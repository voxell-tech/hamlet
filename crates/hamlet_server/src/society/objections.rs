//! An instruction's one objection, held until its chief answers.

use anyhow::Result;
use hamlet_protocol::v0::Choice;
use serde_json::{Value, json};

use crate::OperationId;
use crate::chats::records;
use crate::harness::{Answer, Binding, InputMark};
use crate::oversight::held::{self, CallState};
use crate::oversight::{
    ChatPlace, Holding, Raising, Ruling, WaitKind,
};
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
    let id = OperationId {
        runtime: binding.runtime.clone(),
        key: key.into(),
    };
    match held::decision(db, &id)?.map(|call| call.state) {
        Some(CallState::Blocked(words)) => {
            return Ok(Answer::Refusal(words));
        }
        Some(CallState::Held { .. }) => {
            return Ok(super::letters::waiting());
        }
        Some(CallState::Ruled {
            ruling: Ruling::Go,
            text,
            ..
        }) => {
            return Ok(Answer::result(
                format!("The chief says go ahead: {text}"),
                false,
            ));
        }
        Some(CallState::Ruled { .. }) => {
            return Ok(Answer::result(
                "The chief withdrew the instruction.",
                true,
            ));
        }
        Some(CallState::Open | CallState::Allowed { .. }) | None => {}
    }
    let Some(kept) = records::kept(db, binding.id)? else {
        return Ok(Answer::result(
            "No chief oversees this instruction.",
            true,
        ));
    };
    let Some(InputMark::Chief {
        chief, text, task, ..
    }) = service.home.store().input_mark(binding.id, input)?
    else {
        return Ok(Answer::result(
            "No chief oversees this instruction.",
            true,
        ));
    };
    let db = service.home.store().connection();
    let task = task.as_str();
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
    let record = crate::record::read(db, &kept.place.agent.hamlet)?
        .expect("instruction's record");
    let name = record.name_of(
        kept.place.agent.number,
        crate::record::NameFallback::Recorded,
    );
    let tx = service.home.store().transaction()?;
    let wait = held::raise(
        &tx,
        Raising {
            kind: WaitKind::Objection,
            by: name.clone(),
            agent: Some(kept.place.agent.clone()),
            about: format!(
                "{chief} told {name}: {text}\n{name} objects: \
                 {reason}"
            ),
            change: None,
            answers: vec![Choice::Go, Choice::Withdraw],
            chat: Some(ChatPlace {
                hamlet: kept.place.agent.hamlet,
                number: kept.place.agent.number,
                chief,
                session: kept.place.session,
            }),
            call: Some(Holding {
                id,
                binding: binding.id,
                input: input.map(String::from),
                change: None,
            }),
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
    use rusqlite::params;

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
        held::reserve(
            store.connection(),
            &OperationId {
                runtime: runtime.clone(),
                key: "objection".into(),
            },
            id,
            b"objection",
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
        store
            .connection()
            .execute(
                "INSERT INTO harness_input \
                 (binding,request,ordinal,content,steer,state,\
                 submission,mark) VALUES \
                 (?1,'request',1,'Work',0,'admitted','input',?2)",
                params![
                    id.0,
                    serde_json::to_string(&InputMark::Chief {
                        chief: "chief".into(),
                        text: "Work".into(),
                        task: "task".into(),
                        at: 0
                    })
                    .unwrap()
                ],
            )
            .unwrap();
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
