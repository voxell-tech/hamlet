use hamlet_messages::realm::client::{Answer, AnswerRefusal};
use hamlet_messages::realm::{
    Asked, Done, End, Hire, HireAnswer, Named, NeedsChief,
    SecretList, Secrets,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::SecretsVersion;
use hamlet_messages::workspace::{
    self, AgentNumber, CallId, ChiefAnswer, ConversationId,
    ReachVersion,
};
use hamlet_protocol::client::{self as protocol, Choice, RequestId};

use super::{Realm, error_code, founded, until};

fn needs(agent: AgentNumber, call: &str) -> NeedsChief {
    NeedsChief {
        agent,
        call: CallId(call.to_owned()),
        conversation: ConversationId("c".to_owned()),
        realm_task: None,
        needs: Vec::new(),
        subject: "rm -rf build".to_owned(),
        change: None,
        reach_version: ReachVersion(1),
    }
}

fn answer(wait: &str, choice: Choice, text: Option<&str>) -> Answer {
    Answer(protocol::Answer {
        choice,
        id: RequestId::new(),
        picks: None,
        text: text.map(str::to_owned),
        wait: wait.to_owned(),
    })
}

fn nothing_waits(wait: &str) -> Done<AnswerRefusal> {
    Done::Refused {
        why: AnswerRefusal::NothingWaits {
            wait: wait.to_owned(),
        },
    }
}

/// `agent`'s waits: each id, whether it is open, and its choice.
async fn waits_of(
    realm: &Realm,
    agent: AgentNumber,
) -> Vec<(i64, bool, Option<String>)> {
    realm
        .rows(|db| {
            db.prepare(
                "SELECT id, closed_at IS NULL, choice FROM wait \
                 WHERE agent = ?1 ORDER BY id",
            )?
            .query_map([agent.0], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect()
        })
        .await
}

fn chief_answers(realm: &Realm) -> Vec<ChiefAnswer> {
    realm.seen.chief_answers.lock().unwrap().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wait_opens_once_and_its_answer_reaches_the_workspace() {
    // NeedsChief twice for one call of the Hand, as the workspace:
    // one open wait; the chief allows it: Done, and the workspace
    // stand-in got one ChiefAnswer with Allow for that agent and
    // call.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let asked = needs(Hand::NUMBER, "c1");

    realm.call_as_workspace(&asked).await.unwrap();
    realm.call_as_workspace(&asked).await.unwrap();
    let waits = waits_of(&realm, Hand::NUMBER).await;
    let [(wait, true, None)] = waits[..] else {
        panic!("one open wait: {waits:?}");
    };
    let allowed = realm
        .ask(&answer(&wait.to_string(), Choice::Allow, None))
        .await
        .unwrap();
    until("the workspace has the chief's answer", || {
        !chief_answers(&realm).is_empty()
    })
    .await;

    assert_eq!(allowed, Done::Done);
    assert_eq!(
        chief_answers(&realm),
        [ChiefAnswer {
            agent: Hand::NUMBER,
            call: asked.call,
            answer: workspace::Answer::Allow,
        }]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_answer_calls_the_workspace_again() {
    // the chief refuses a wait with a reason, then sends the same
    // answer again: Done twice, and the workspace got the refusal
    // with its reason twice. An answer with another id finds nothing
    // waiting, and so does one to a wait no row can have.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    realm
        .call_as_workspace(&needs(Hand::NUMBER, "c1"))
        .await
        .unwrap();
    let refused = answer("#1", Choice::Deny, Some("not now"));

    let first = realm.ask(&refused).await.unwrap();
    let again = realm.ask(&refused).await.unwrap();
    let other =
        realm.ask(&answer("1", Choice::Allow, None)).await.unwrap();
    let past_any = "9223372036854775808";
    let unnumbered =
        realm.ask(&answer(past_any, Choice::Allow, None)).await;
    until("the workspace has both answers", || {
        chief_answers(&realm).len() == 2
    })
    .await;

    assert_eq!((first, again), (Done::Done, Done::Done));
    assert_eq!(other, nothing_waits("1"));
    assert_eq!(unnumbered.unwrap(), nothing_waits(past_any));
    let refusal = workspace::Answer::Refuse {
        reason: Some("not now".to_owned()),
    };
    assert!(
        chief_answers(&realm)
            .iter()
            .all(|answer| answer.answer == refusal),
        "{:?}",
        chief_answers(&realm)
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_end_closes_open_waits() {
    // two waits for a live hire; end it: both rows are closed as
    // lapsed, a wait asked for after the end opens nothing, and a
    // later Answer to one is refused NothingWaits.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    let hired = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: Hire {
                hamlet: Some(hamlet),
                under: None,
                profile: None,
                purpose: None,
            },
        })
        .await
        .unwrap();
    let HireAnswer::Hired { agent: hire } = hired else {
        panic!("the chief hires: {hired:?}");
    };
    realm.live(hire).await;
    for call in ["c1", "c2"] {
        realm.call_as_workspace(&needs(hire, call)).await.unwrap();
    }

    let ended = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: End {
                hire: Named::Agent { agent: hire },
                why: None,
            },
        })
        .await
        .unwrap();
    realm.call_as_workspace(&needs(hire, "c3")).await.unwrap();
    let waits = waits_of(&realm, hire).await;
    let late =
        realm.ask(&answer("1", Choice::Allow, None)).await.unwrap();

    assert_eq!(ended, Done::Done);
    let lapsed = Some("lapsed".to_owned());
    assert_eq!(
        waits,
        [(1, false, lapsed.clone()), (2, false, lapsed)],
        "both waits closed as lapsed, and none opened after the end"
    );
    assert_eq!(late, nothing_waits("1"));
    assert!(chief_answers(&realm).is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wait_from_another_sender_than_the_workspace_is_refused() {
    let realm = Realm::start().await;

    let called =
        realm.tool(Hand::NUMBER, &needs(Hand::NUMBER, "c1")).await;

    assert_eq!(error_code(called), "message.wrong_caller");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_secrets_are_version_1_with_no_value() {
    let realm = Realm::start().await;

    let secrets = realm.call_as_workspace(&Secrets).await.unwrap();

    assert_eq!(
        secrets,
        SecretList {
            version: SecretsVersion(1),
            values: Vec::new(),
        }
    );
    realm.host.shutdown().await.unwrap();
}
