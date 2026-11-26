//! The world as a client reads it, and the event that says it moved.

use std::time::Instant;

use hamlet_host::{Item, Subscription};
use hamlet_messages::address::{self, Party as _};
use hamlet_messages::realm::client::{
    ReadState, ReadWorld, RememberSecret, World,
};
use hamlet_messages::realm::hamlets::{ReadRecord, Record};
use hamlet_messages::realm::lines::Working;
use hamlet_messages::realm::models::{
    KeepCredential, Kept, Settings,
};
use hamlet_messages::realm::world::Revision;
use hamlet_messages::realm::{HamletId, NeedsChief};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{self as protocol, RequestId};

use super::chiefs::chief;
use super::world::seen_until;
use super::{
    DEADLINE, MAYOR, Realm, THREE, hamlet_with, needs, say,
    seen_submits, tell, to, tool,
};

const KEY: &str = "sk-made-up-0461";

async fn keep(realm: &Realm, key: &str) {
    let keep = KeepCredential {
        provider: "anthropic".to_owned(),
        value: Kept::Key {
            key: key.to_owned(),
            env: Settings::new(),
        },
    };
    realm.tool(Hand::NUMBER, &keep).await.unwrap();
}

async fn state(realm: &Realm) -> protocol::State {
    realm.ask(&ReadState).await.unwrap()
}

fn agent(
    state: &protocol::State,
    agent: AgentNumber,
) -> &protocol::Agent {
    state
        .agents
        .iter()
        .find(|shown| shown.id.0 == u64::from(agent.0))
        .expect("the agent is shown")
}

/// A wait of 3 for the command `subject`, asked in the call `call`.
async fn asks(realm: &Realm, call: &str, subject: &str) {
    realm
        .call_as_workspace(&NeedsChief {
            subject: subject.to_owned(),
            ..needs(THREE, call)
        })
        .await
        .unwrap();
}

/// Everything the realm answers a client, as one text: the state,
/// the world, with its waits, and the hamlet's record.
async fn shown(
    realm: &Realm,
    hamlet: &HamletId,
    waits: usize,
) -> String {
    let world = seen_until(realm, "the waits are open", |seen| {
        seen.waits.len() >= waits
    })
    .await;
    let record = chief(
        realm,
        ReadRecord {
            hamlet: Some(hamlet.clone()),
        },
    )
    .await;
    let Record::Read { lines } = record else {
        panic!("a record: {record:?}");
    };
    format!(
        "{}\n{world:?}\n{lines:?}",
        serde_json::to_string(&state(realm).await).unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_key_never_reaches_a_client() {
    let (realm, hamlet) = hamlet_with(1).await;
    let with_key = format!("use {KEY} for it");
    asks(&realm, "before", &format!("curl -H {KEY}")).await;
    realm
        .tool(MAYOR, &tool("t1", tell(to(THREE), &with_key)))
        .await
        .unwrap();
    keep(&realm, KEY).await;
    asks(&realm, "after", &format!("curl -H {KEY} again")).await;
    realm
        .ask(&say(&RequestId::new(), THREE, &with_key))
        .await
        .unwrap();

    let shown = shown(&realm, &hamlet, 2).await;

    assert!(shown.contains("curl -H [REDACTED]"), "{shown}");
    assert!(!shown.contains(KEY), "{shown}");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_typed_at_a_sign_in_is_scrubbed_from_what_the_realm_shows()
 {
    let (realm, hamlet) = hamlet_with(1).await;
    asks(&realm, "c1", &format!("curl -H {KEY}")).await;

    realm
        .ask(&RememberSecret {
            secret: KEY.to_owned(),
        })
        .await
        .unwrap();
    let shown = shown(&realm, &hamlet, 1).await;

    assert!(shown.contains("curl -H [REDACTED]"), "{shown}");
    assert!(!shown.contains(KEY), "{shown}");
    realm.host.shutdown().await.unwrap();
}

/// The first `World` of `events` past `after`.
async fn next_world(
    events: &mut Subscription,
    after: Revision,
) -> Revision {
    let begun = Instant::now();
    loop {
        let left = DEADLINE.saturating_sub(begun.elapsed());
        let item = tokio::time::timeout(left, events.next())
            .await
            .expect("a world event comes")
            .unwrap();
        let Item::Event(bytes) = item else {
            continue;
        };
        let (World { revision },) =
            ciborium::from_reader::<(World,), _>(bytes.as_slice())
                .unwrap();
        if revision > after {
            return revision;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_world_shows_a_busy_agent_until_its_turn_ends() {
    let (realm, _) = hamlet_with(1).await;
    let mut events = realm
        .host
        .subscribe_for_client(
            &address::Realm.address().unwrap(),
            "world",
        )
        .await
        .unwrap();
    let before = realm.ask(&ReadWorld).await.unwrap().revision;

    realm.tool(THREE, &Working { open: true }).await.unwrap();
    let opened = next_world(&mut events, before).await;
    let busy = state(&realm).await;
    realm.tool(THREE, &Working { open: false }).await.unwrap();
    next_world(&mut events, opened).await;
    let idle = state(&realm).await;

    assert!(agent(&busy, THREE).busy);
    assert!(!agent(&busy, MAYOR).busy);
    assert!(!agent(&idle, THREE).busy);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_shows_the_letters_sent_it_by_sender_and_kind() {
    let (realm, _) = hamlet_with(1).await;
    realm
        .tool(MAYOR, &tool("t1", tell(to(THREE), "dig the well")))
        .await
        .unwrap();
    seen_submits(&realm, THREE, 1).await;

    let kept = |shown: &protocol::State| {
        agent(shown, THREE)
            .conversations
            .iter()
            .map(|c| {
                (c.with, c.kind.clone(), c.count, c.last.is_some())
            })
            .collect::<Vec<_>>()
    };
    let given = [(
        protocol::AgentId(u64::from(MAYOR.0)),
        protocol::ConversationViewKind::Letters,
        1,
        true,
    )];
    let began = Instant::now();
    let mut shown = state(&realm).await;
    while kept(&shown) != given && began.elapsed() < DEADLINE {
        tokio::time::sleep(std::time::Duration::from_millis(20))
            .await;
        shown = state(&realm).await;
    }
    assert_eq!(kept(&shown), given);
    assert!(agent(&shown, MAYOR).conversations.is_empty());
    realm.host.shutdown().await.unwrap();
}
