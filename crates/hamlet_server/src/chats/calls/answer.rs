//! A chief's answer to what waits: checked, kept with every change it
//! makes in one transaction, then told to the calls it ruled on.

use anyhow::Result;
use hamlet_protocol::{Choice, Permission, Pick};

use super::super::{asking, records};
use super::decide::standing;
use crate::gate::{self, Asked, TrustKey};
use crate::harness::{BindingId, Harnesses};
use crate::oversight::{
    self, Danger, Ruling, Wait, WaitKind, Woken, held,
};
use crate::protocol::{Refusable, Refusal};
use crate::service::Service;

const NOTHING: &str = "Nothing waits for you.";

/// A chief's `choice` on the wait `id`, in the words a reply gives.
pub(in crate::chats) async fn answer_words(
    service: &mut Service,
    harnesses: &mut Harnesses,
    id: &str,
    choice: &Choice,
    text: &str,
    picks: Option<&[Pick]>,
) -> Result<Refusable<String>> {
    let db = service.home.store().connection();
    let Some(wait) = oversight::find(db, id)? else {
        return Ok(Ok(NOTHING.into()));
    };
    let hamlet = wait
        .agent
        .as_ref()
        .map(|agent| agent.hamlet.clone())
        .or_else(|| {
            wait.chat.as_ref().map(|chat| chat.hamlet.clone())
        });
    if let Some(hamlet) = hamlet
        && let Err(refusal) =
            service.access(&hamlet, "answer wait")?
    {
        return Ok(Err(refusal));
    }
    let text = match words(&wait, choice, text, picks) {
        Ok(text) => text,
        Err(why) => return Ok(Err(Refusal::Invalid(why))),
    };
    let text =
        service.credentials.scrub_text(&text).trim().to_owned();
    let ruling = Ruling::from(choice);
    let Some((settled, woken)) = rule(service, &wait, ruling, &text)?
    else {
        return Ok(Ok(NOTHING.into()));
    };
    let objection = settled.kind == WaitKind::Objection;
    if objection && ruling == Ruling::Withdraw {
        crate::fault::point("after_instruction_withdrawal");
    }
    crate::society::reviews::sweep(service, harnesses).await?;
    for (runtime, key) in woken {
        harnesses.wake(&runtime, &key).await;
    }
    if objection {
        harnesses.wake_turns().await;
    }
    Ok(Ok(format!(
        "Answered /{}: {}",
        ruling.as_str(),
        oversight::line(&settled, 80)
    )))
}

/// The words of `choice` on `wait`: a reply's text, or the answer its
/// picks make; else why it cannot be answered so.
fn words(
    wait: &Wait,
    choice: &Choice,
    text: &str,
    picks: Option<&[Pick]>,
) -> std::result::Result<String, String> {
    if !wait.answers.contains(choice) {
        return Err(format!(
            "That cannot be answered that way:\n{}",
            oversight::strip(wait)
        ));
    }
    if let Some(picks) = picks {
        if *choice != Choice::Reply || !text.is_empty() {
            return Err(
                "Use picks only for a reply, without text.".into()
            );
        }
        let WaitKind::Question(questions) = &wait.kind else {
            return Err("This wait does not take structured \
                        answers."
                .into());
        };
        return asking::answer(questions, picks);
    }
    if *choice != Choice::Reply {
        return Ok(text.into());
    }
    if text.trim().is_empty() {
        return Err("Say what to reply.".into());
    }
    let WaitKind::Question(questions) = &wait.kind else {
        return Err("This wait does not take a reply.".into());
    };
    let words = Pick::Words { text: text.into() };
    asking::answer(questions, &[words])?;
    Ok(format!("{} {}", asking::ANSWERS, text.trim()))
}

/// A call `wait` holds that a chief's answer changes.
struct Ruled {
    call: held::Unruled,
    place: records::Place,
    /// The network it runs with when allowed.
    net: bool,
}

/// Settles `wait` with `ruling`, in one transaction with what the
/// ruling gives or counts: a grant, the network or trust of the calls
/// it allows, a refusal's strike and an instruction's ruling. Gives
/// the settled wait and the calls to tell, or `None` when it no
/// longer waits.
fn rule(
    service: &mut Service,
    wait: &Wait,
    ruling: Ruling,
    text: &str,
) -> Result<Option<(Wait, Woken)>> {
    let chief = service.chief.clone();
    let now = crate::clock::now();
    let danger = match wait.kind {
        WaitKind::Approval(danger) => Some(danger),
        _ => None,
    };
    let db = service.home.store().connection();
    let mut ruled = Vec::new();
    for call in held::unruled(db, &wait.id)? {
        let db = service.home.store().connection();
        let Some(conversation) = records::kept(db, call.binding)?
        else {
            continue;
        };
        let place = conversation.place;
        let mut net = false;
        if ruling.allows() {
            let Some(record) =
                service.home.store().record(&place.agent.hamlet)?
            else {
                continue;
            };
            let standing = standing(service, &place, &record)?;
            net = danger == Some(Danger::Network)
                || (matches!(
                    danger,
                    Some(Danger::Edit | Danger::Command)
                ) && standing.net);
        }
        ruled.push(Ruled { call, place, net });
    }
    let tx = service.home.store().transaction()?;
    if ruling == Ruling::Always
        && danger == Some(Danger::Network)
        && let Some(agent) = &wait.agent
    {
        gate::give(
            &tx,
            agent,
            &wait.by,
            &Permission::Network,
            &chief,
            now,
        )?;
    }
    let mut struck = Vec::<BindingId>::new();
    for Ruled { call, place, net } in &ruled {
        if ruling.allows() {
            held::set_net(&tx, &call.id, *net)?;
        }
        if ruling == Ruling::Always
            && let Some(danger @ (Danger::Edit | Danger::Command)) =
                danger
        {
            gate::trust(
                &tx,
                &TrustKey {
                    agent: &place.agent,
                    chief: place.chief.as_deref().unwrap_or_default(),
                    session: place.session,
                },
                danger,
            )?;
        }
        if ruling == Ruling::Deny
            && let Some(danger) = danger
            && !struck.contains(&call.binding)
        {
            let mut asked =
                Asked::of(&tx, call.binding, call.input.as_deref())?;
            asked.refuse(
                &wait.by,
                danger,
                &wait.about,
                if text.is_empty() {
                    "no reason given"
                } else {
                    text
                },
            );
            asked.keep(&tx, call.binding)?;
            struck.push(call.binding);
        }
    }
    let Some((settled, mut woken)) = oversight::settle(
        &tx,
        &wait.id,
        ruling,
        text,
        Some(&chief),
        now,
    )?
    else {
        return Ok(None);
    };
    if settled.kind == WaitKind::Objection {
        woken.extend(crate::society::reviews::settle(
            &tx, &wait.id, ruling, text, &chief,
        )?);
    }
    tx.commit()?;
    Ok(Some((settled, woken)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use hamlet_runtime_protocol::ModelRef;
    use hamlet_testing::Scratch;
    use serde_json::json;

    use super::super::decide::{Decision, Gate, allow};
    use super::*;
    use crate::record::AgentRef;

    #[test]
    fn an_answer_that_fails_to_settle_grants_nothing() {
        let scratch = Scratch::new("answer-atomic");
        let home = crate::Home::open(&scratch.join("home")).unwrap();
        let mut service = Service::new(home, true).unwrap();
        let agent = AgentRef {
            hamlet: "home".into(),
            number: 0,
        };
        let store = service.home.store();
        let runtime = store.runtime_for("pi").unwrap();
        let model = ModelRef {
            provider: "faux".into(),
            id: "faux-1".into(),
            base_url: None,
        };
        let binding = store
            .add_binding(&agent, false, &model, &runtime)
            .unwrap();
        let place = records::Place {
            agent: agent.clone(),
            chief: Some(service.chief.clone()),
            session: 1,
        };
        records::keep(store.connection(), binding, &place, None, 0)
            .unwrap();
        let args = BTreeMap::from([
            ("command".to_owned(), json!("curl example.com")),
            ("network".to_owned(), json!(true)),
        ]);
        let gate = allow(
            &mut service,
            binding,
            "k",
            None,
            "bash",
            &args,
            None,
        )
        .unwrap();
        assert!(matches!(gate, Gate::Decided(Decision::Wait(_))));
        let db = service.home.store().connection();
        let wait = oversight::open(db).unwrap().remove(0);
        db.execute_batch(
            "CREATE TEMP TRIGGER crash BEFORE UPDATE OF open ON \
             wait BEGIN SELECT RAISE(ABORT, 'crash'); END;",
        )
        .unwrap();
        assert!(
            rule(&mut service, &wait, Ruling::Always, "").is_err()
        );
        let db = service.home.store().connection();
        assert!(
            gate::grants(db).unwrap().is_empty(),
            "a grant stands for a wait that is still open"
        );
        assert!(oversight::find(db, &wait.id).unwrap().is_some());
        db.execute_batch("DROP TRIGGER crash;").unwrap();
        assert!(
            rule(&mut service, &wait, Ruling::Always, "")
                .unwrap()
                .is_some()
        );
        let db = service.home.store().connection();
        assert_eq!(gate::grants(db).unwrap().len(), 1);
    }
}
