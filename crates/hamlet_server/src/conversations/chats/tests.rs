use hamlet_store::ids::{Actor, AgentId, ConversationId};
use hamlet_store::schema::tool_call;

use super::*;
use crate::conversations::inputs::{Input, Resend, Status};
use crate::conversations::messages::{self, MessageKind, Send};
use crate::realm::settings::{Overrides, Settings};
use crate::testing::Realm;

fn env() -> EffectiveSettings {
    EffectiveSettings {
        settings: Settings::default(),
        overrides: Overrides {
            faux: true,
            ..Overrides::default()
        },
    }
}

fn say_to(realm: &mut Realm, agent: AgentId, id: &str) -> Said {
    let (env, chief) = (env(), realm.chief);
    realm
        .chief(|cx| {
            Ok(Ok(say(cx, &env, agent, chief, id, "Go on.")?))
        })
        .unwrap()
}

/// The requests of the turns `conversation` owes.
fn next(
    realm: &mut Realm,
    conversation: ConversationId,
) -> Vec<String> {
    inputs::owed(realm.db(), conversation, Resend::Nothing, |_| false)
        .unwrap()
        .into_iter()
        .filter(Input::is_turn)
        .map(|input| input.request)
        .collect()
}

#[test]
fn a_stop_holds_the_queue_until_a_chiefs_line_runs_first_or_a_resume()
{
    let mut realm = Realm::new();
    let lead = realm.found("docs", "ada").agent;
    let running = say_to(&mut realm, lead, "r1");
    let waiting = say_to(&mut realm, lead, "r2");
    let conversation = running.conversation;
    let db = realm.db();
    conversations::bound(db, conversation, "c1", "pi", "1").unwrap();
    inputs::admitted(
        db,
        conversation,
        &running.request,
        "s1",
        Status::Placed,
        None,
        None,
    )
    .unwrap();
    let stopped =
        realm.chief(|cx| Ok(Ok(stop(cx, lead, &[])?))).unwrap();
    assert_eq!(stopped, 1);
    // A stop before the runtime reports the end does not count twice.
    let again = realm
        .chief(|cx| Ok(Ok(stop(cx, lead, &[conversation])?)))
        .unwrap();
    assert_eq!(again, 0);
    inputs::settled(
        realm.db(),
        conversation,
        &running.request,
        Status::Unanswered,
        Some("stopped"),
        None,
    )
    .unwrap();
    assert!(next(&mut realm, conversation).is_empty());
    let after = say_to(&mut realm, lead, "r3");
    assert_eq!(next(&mut realm, conversation), [after.request]);
    realm.chief(|cx| Ok(Ok(stop(cx, lead, &[])?))).unwrap();
    assert!(next(&mut realm, conversation).is_empty());
    realm.chief(|cx| Ok(Ok(resume(cx, lead)?))).unwrap();
    assert_eq!(next(&mut realm, conversation), [waiting.request]);
}

#[test]
fn a_failed_request_comes_back_without_a_turn() {
    let mut realm = Realm::new();
    let founded = realm.found("docs", "ada");
    let (lead, bo) =
        (founded.agent, realm.add_agent(&founded, "bo").agent);
    let env = env();
    realm
        .by(Actor::Agent(lead), |cx| {
            let from = current(cx, &env, lead)?;
            let task =
                tasks::mint(cx, Origin::Notice, lead, None, None)?;
            diesel::insert_into(tool_call::table)
                .values((
                    tool_call::runtime.eq("pi"),
                    tool_call::key.eq("k1"),
                    tool_call::conversation.eq(from),
                    tool_call::digest.eq(Vec::<u8>::new()),
                    tool_call::tool.eq("request"),
                ))
                .execute(cx.db)?;
            let send = Send {
                kind: MessageKind::Request,
                runtime: "pi",
                key: "k1",
                sender: lead,
                receiver: bo,
                from,
                address: "",
                task,
                text: "Carry on.",
                images: &[],
            };
            messages::send(cx, &env, send)?;
            Ok(Ok(()))
        })
        .unwrap();
    let theirs = of(realm.db(), bo).unwrap().unwrap();
    let request = inputs::queued(realm.db(), theirs).unwrap()[0]
        .request
        .clone();
    inputs::settled(
        realm.db(),
        theirs,
        &request,
        Status::Unanswered,
        Some("no_model"),
        None,
    )
    .unwrap();
    realm
        .chief(|cx| {
            conversations::completion::follow(
                cx, &env, theirs, &request,
            )?;
            Ok(Ok(()))
        })
        .unwrap();
    let ours = of(realm.db(), lead).unwrap().unwrap();
    let owed =
        inputs::owed(realm.db(), ours, Resend::Nothing, |_| false)
            .unwrap();
    let [response] = &owed[..] else {
        panic!("one input: {owed:?}");
    };
    assert_eq!(response.kind, InputKind::ErrorResponse);
    assert!(!inputs::is_busy(realm.db(), ours).unwrap());
}
