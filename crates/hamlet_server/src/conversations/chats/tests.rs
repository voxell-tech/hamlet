use hamlet_protocol::client::{
    ImportSource, Imported, ImportedEntry,
};
use hamlet_store::ids::{Actor, AgentId, ConversationId};
use hamlet_store::schema::tool_call;

use super::*;
use crate::conversations::imports::{self, Project};
use crate::conversations::inputs::{Resend, Status};
use crate::conversations::messages::{self, MessageKind, Send};
use crate::realm::settings::{Overrides, Settings};
use crate::society::merges;
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

/// A hamlet's mayor and a blank hire `name` under it.
fn pair(realm: &mut Realm, name: &str) -> (AgentId, AgentId) {
    let mayor = realm.found("docs", "ada");
    let hire = realm.hire(&mayor, name);
    (mayor.agent, hire.agent)
}

/// `sender`'s notification `text` to `receiver`, sent by the call
/// `key`.
fn notification(
    realm: &mut Realm,
    sender: AgentId,
    receiver: AgentId,
    key: &str,
    text: &str,
) {
    message(
        realm,
        MessageKind::Notification,
        (sender, receiver),
        key,
        text,
    );
}

/// The message of `kind` with `text` from the first of `between` to
/// the second, sent by the call `key`.
fn message(
    realm: &mut Realm,
    kind: MessageKind,
    between: (AgentId, AgentId),
    key: &str,
    text: &str,
) {
    let (sender, receiver) = between;
    let env = env();
    realm
        .by(Actor::Agent(sender), |cx| {
            let from = current(cx, &env, sender)?;
            let task =
                tasks::mint(cx, Origin::Notice, sender, None, None)?;
            diesel::insert_into(tool_call::table)
                .values((
                    tool_call::runtime.eq("pi"),
                    tool_call::key.eq(key),
                    tool_call::conversation.eq(from),
                    tool_call::digest.eq(Vec::<u8>::new()),
                    tool_call::tool.eq("request"),
                ))
                .execute(cx.db)?;
            messages::send(
                cx,
                &env,
                Send {
                    kind,
                    runtime: "pi",
                    key,
                    sender,
                    receiver,
                    from,
                    address: "",
                    task,
                    text,
                    images: &[],
                },
            )?;
            Ok(Ok(()))
        })
        .unwrap();
}

fn say_to(realm: &mut Realm, agent: AgentId, id: &str) -> Said {
    let (env, chief) = (env(), realm.chief);
    realm
        .chief(|cx| {
            Ok(Ok(say(cx, &env, agent, chief, id, "Go on.")?))
        })
        .unwrap()
}

fn queued_ids(
    realm: &mut Realm,
    conversation: ConversationId,
) -> Vec<String> {
    crate::clients::fold::queued(realm.db(), conversation)
        .unwrap()
        .into_iter()
        .map(|item| item.id)
        .collect()
}

#[test]
fn a_chiefs_line_runs_before_the_notifications_queued_first() {
    let mut realm = Realm::new();
    let (mayor, bo) = pair(&mut realm, "bo");
    notification(&mut realm, bo, mayor, "k1", "The docs are done.");
    let said = say_to(&mut realm, mayor, "r1");
    let owed = inputs::owed(
        realm.db(),
        said.conversation,
        Resend::Nothing,
        |_| false,
    )
    .unwrap();
    assert_eq!(owed.len(), 1);
    assert_eq!(owed[0].request, said.request);
    // The model reads who a notification is from, as a summary of it
    // does.
    let queued =
        inputs::queued(realm.db(), said.conversation).unwrap();
    assert_eq!(queued[1].kind, InputKind::Notification);
    assert!(queued[1].content.starts_with(&format!(
        "[A notification from hire#{bo} (bo)]"
    )));
}

#[test]
fn a_stop_holds_the_queue_until_a_chiefs_line_runs_first_or_a_resume()
{
    let mut realm = Realm::new();
    let (mayor, bo) = pair(&mut realm, "bo");
    let running = say_to(&mut realm, mayor, "r1");
    let waiting = say_to(&mut realm, mayor, "r2");
    notification(&mut realm, bo, mayor, "k1", "Done.");
    let db = realm.db();
    conversations::bound(db, running.conversation, "c1", "pi", "1")
        .unwrap();
    inputs::admitted(
        db,
        running.conversation,
        &running.request,
        "s1",
        Status::Placed,
        None,
        None,
    )
    .unwrap();
    let stopped =
        realm.chief(|cx| Ok(Ok(stop(cx, mayor, &[])?))).unwrap();
    assert_eq!(stopped, 1);
    // Stopped again before its runtime reports the end, even while
    // the runtime still says it runs, it is not stopped twice.
    let again = realm
        .chief(|cx| Ok(Ok(stop(cx, mayor, &[running.conversation])?)))
        .unwrap();
    assert_eq!(again, 0);
    let conversation = running.conversation;
    inputs::settled(
        realm.db(),
        conversation,
        &running.request,
        Status::Unanswered,
        Some("stopped"),
        None,
    )
    .unwrap();
    let next = |realm: &mut Realm| {
        inputs::owed(
            realm.db(),
            conversation,
            Resend::Nothing,
            |_| false,
        )
        .unwrap()
        .into_iter()
        .map(|input| input.request)
        .collect::<Vec<_>>()
    };
    // The turn ended, and what waits stays queued and shown.
    assert!(next(&mut realm).is_empty());
    assert_eq!(queued_ids(&mut realm, conversation).len(), 2);
    // The chief's next line runs before the line queued before it.
    let after = say_to(&mut realm, mayor, "r3");
    assert_eq!(next(&mut realm), [after.request]);
    realm.chief(|cx| Ok(Ok(stop(cx, mayor, &[])?))).unwrap();
    assert!(next(&mut realm).is_empty());
    realm.chief(|cx| Ok(Ok(resume(cx, mayor)?))).unwrap();
    assert_eq!(next(&mut realm), [waiting.request]);
}

#[test]
fn a_queued_line_shows_until_the_chief_takes_it_back() {
    let mut realm = Realm::new();
    let (mayor, _) = pair(&mut realm, "bo");
    let said = say_to(&mut realm, mayor, "r1");
    let shown = queued_ids(&mut realm, said.conversation);
    assert_eq!(shown.len(), 1);
    let chief = realm.chief;
    realm
        .chief(|cx| withdraw(cx, mayor, chief, &shown[0]))
        .unwrap();
    assert!(queued_ids(&mut realm, said.conversation).is_empty());
    assert!(
        inputs::owed(
            realm.db(),
            said.conversation,
            Resend::Nothing,
            |_| false
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn an_imported_conversation_and_a_later_notification_share_one_context()
 {
    let mut realm = Realm::new();
    let (mayor, _) = pair(&mut realm, "bo");
    let (env, chief) = (env(), realm.chief);
    let imported = Imported {
        began: 1,
        entries: vec![ImportedEntry::User {
            at: 1,
            text: "Fix the parser.".into(),
        }],
        left_out: 0,
        model: None,
        session: "s1".into(),
        source: ImportSource::Pi,
        title: "Parser".into(),
    };
    let brought = realm
        .chief(|cx| {
            imports::bring_in_project(
                cx,
                &env,
                chief,
                &Project {
                    name: "docs",
                    folder: None,
                    conversations: std::slice::from_ref(&imported),
                },
            )
        })
        .unwrap();
    assert_eq!(brought.hired, 1);
    let lead =
        crate::society::hamlets::get(realm.db(), &brought.hamlet)
            .unwrap()
            .mayor;
    let hire = crate::society::tree::children(realm.db(), Some(lead))
        .unwrap()
        .into_iter()
        .max()
        .unwrap();
    let conversation = of(realm.db(), hire).unwrap().unwrap();
    assert!(
        !imports::held(realm.db(), conversation).unwrap().is_empty()
    );
    notification(&mut realm, mayor, hire, "k1", "How is the parser?");
    let queued = inputs::queued(realm.db(), conversation).unwrap();
    assert_eq!(queued[0].kind, InputKind::Notification);
}

#[test]
fn a_merge_then_a_split_keep_both_conversations() {
    let mut realm = Realm::new();
    let (mayor, bo) = pair(&mut realm, "bo");
    let env = env();
    let theirs = say_to(&mut realm, bo, "r1").conversation;
    let ours = say_to(&mut realm, mayor, "r2").conversation;
    let merge = realm
        .chief(|cx| {
            let id = merges::merge(cx, mayor, bo, "overlap")?;
            merged(cx, &env, mayor, bo)?;
            Ok(id)
        })
        .unwrap();
    let kept =
        inputs::owed(realm.db(), ours, Resend::Nothing, |_| false)
            .unwrap();
    let digest = kept
        .iter()
        .find(|input| input.kind == InputKind::Merged)
        .unwrap();
    assert!(matches!(
        digest.mark().unwrap(),
        Mark::Merged { from, .. } if from == bo
    ));
    realm
        .chief(|cx| merges::split(cx, merge, "apart again"))
        .unwrap();
    assert_eq!(of(realm.db(), bo).unwrap(), Some(theirs));
    assert_eq!(of(realm.db(), mayor).unwrap(), Some(ours));
}

#[test]
fn a_failed_request_comes_back_in_plain_words_without_a_turn() {
    let mut realm = Realm::new();
    let (mayor, bo) = pair(&mut realm, "bo");
    message(
        &mut realm,
        MessageKind::Request,
        (mayor, bo),
        "k1",
        "Carry on.",
    );
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
    let env = env();
    realm
        .chief(|cx| {
            conversations::completion::follow(
                cx, &env, theirs, &request,
            )?;
            Ok(Ok(()))
        })
        .unwrap();
    let ours = of(realm.db(), mayor).unwrap().unwrap();
    let owed =
        inputs::owed(realm.db(), ours, Resend::Nothing, |_| false)
            .unwrap();
    let [response] = &owed[..] else {
        panic!("one input: {owed:?}");
    };
    assert_eq!(response.kind, InputKind::ErrorResponse);
    assert!(!response.is_turn());
    assert!(response.content.contains("it has no model it can use"));
    assert!(!inputs::is_busy(realm.db(), ours).unwrap());
}
