use std::fmt::Display;

use hamlet_messages::agent::Input;
use hamlet_messages::realm::client::AnswerRefusal;
use hamlet_messages::realm::oversight::{Lapse, LapseReason};
use hamlet_messages::realm::workers::{StartWorker, Started};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{
    Asked, Done, End, Gate, HamletId, Hire, HireAnswer, LayRoad,
    LeadRefusal, Limit, NeedsChief, ReservedAct, SetLimits,
    TakeUpRoad, Tool,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
use hamlet_protocol::client::{Choice, RequestId};

use super::asking::chief_task;
use super::{
    FOUR, MAYOR, Realm, THREE, answer, hamlet_with, hire, hired,
    needs, seen_submits, submits_to, to, tool,
};

async fn reserve(realm: &Realm, hamlet: &HamletId, act: ReservedAct) {
    let set = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: SetLimits {
                hamlet: Some(hamlet.clone()),
                budget: Vec::new(),
                limits: vec![Limit::Reserved {
                    act,
                    reserved: true,
                }],
            },
        })
        .await
        .unwrap();
    assert_eq!(set, Done::Done);
}

fn kept<R>(wait: u64) -> Done<LeadRefusal<R>> {
    Done::Refused {
        why: LeadRefusal::Gate(Gate::Reserved { wait: WaitId(wait) }),
    }
}

/// `act` called in a turn of conversation `c`, on `task`.
pub(super) fn in_turn<T>(
    call: &str,
    act: T,
    task: Option<&RealmTask>,
) -> Tool<T> {
    Tool {
        task: task.cloned(),
        conversation: Some(ConversationId("c".to_owned())),
        ..tool(call, act)
    }
}

/// The words a refused hire is told.
pub(super) fn refused_with<W: Display>(
    answer: HireAnswer<W>,
) -> String {
    match answer {
        HireAnswer::Refused { why } => why.to_string(),
        HireAnswer::Hired { agent } => panic!("{agent:?} is hired"),
    }
}

fn end(hire: AgentNumber) -> End {
    End {
        hire: to(hire),
        why: Some("done".to_owned()),
    }
}

fn lay(a: AgentNumber, b: AgentNumber) -> LayRoad {
    LayRoad {
        a: to(a),
        b: to(b),
        why: None,
    }
}

fn take_up(a: AgentNumber, b: AgentNumber) -> TakeUpRoad {
    TakeUpRoad {
        a: to(a),
        b: to(b),
        why: None,
    }
}

/// Each wait's id, kind, subject and choice.
async fn waits(
    realm: &Realm,
) -> Vec<(i64, String, String, Option<String>)> {
    realm
        .rows(|db| {
            db.prepare(
                "SELECT id, kind, subject, choice FROM wait ORDER \
                 BY id",
            )?
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })?
            .collect()
        })
        .await
}

async fn agents(realm: &Realm) -> Vec<(u32, String)> {
    realm
        .rows(|db| {
            db.prepare("SELECT id, status FROM agent ORDER BY id")?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect()
        })
        .await
}

/// Each road's ends and whether it is taken up.
async fn roads(realm: &Realm) -> Vec<(u32, u32, bool)> {
    realm
        .rows(|db| {
            db.prepare(
                "SELECT a, b, up_at IS NOT NULL FROM road ORDER BY \
                 a, b",
            )?
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect()
        })
        .await
}

/// The notices `agent` was given. Reading the rows stops the realm,
/// which first makes every delivery its acts so far have started.
async fn notices(realm: &Realm, agent: AgentNumber) -> Vec<String> {
    decisions(realm).await;
    submits_to(realm, agent)
        .into_iter()
        .filter_map(|submit| match submit.input {
            Input::Notice { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

async fn decisions(realm: &Realm) -> Vec<String> {
    realm
        .rows(|db| {
            db.prepare("SELECT kind FROM decision ORDER BY id")?
                .query_map([], |row| row.get(0))?
                .collect()
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reserved_hire_waits_and_runs_once_on_allow() {
    // a hamlet whose limits reserve hire. Its mayor's hire is kept in
    // a reserved wait that shows the act, and no hire is made; the
    // chief allows it: hire#3 is made and goes live. The same answer
    // again answers done, another answer finds nothing waiting, and
    // the same call again answers the same wait: one hire only.
    let (realm, hamlet) = hamlet_with(0).await;
    reserve(&realm, &hamlet, ReservedAct::Hire).await;
    let hiring = tool("c1", hire(None));

    let first = realm.tool(MAYOR, &hiring).await.unwrap();
    let before = agents(&realm).await;
    let allowed = answer("#1", Choice::Allow, None);
    let allow = realm.ask(&allowed).await.unwrap();
    realm.live(THREE).await;
    let again = realm.ask(&allowed).await.unwrap();
    let other = realm.ask(&answer("#1", Choice::Allow, None)).await;
    let repeat = realm.tool(MAYOR, &hiring).await.unwrap();

    let reserved = HireAnswer::Refused {
        why: LeadRefusal::Gate(Gate::Reserved { wait: WaitId(1) }),
    };
    assert_eq!(first, reserved);
    assert!(
        !before.iter().any(|(agent, _)| *agent == THREE.0),
        "no hire is made before the chief allows it: {before:?}"
    );
    let [(1, kind, subject, Some(choice))] = &waits(&realm).await[..]
    else {
        panic!("one wait, closed");
    };
    assert_eq!(
        (kind.as_str(), choice.as_str()),
        ("reserved", "allow")
    );
    assert_eq!(subject, r#"hire {"purpose":"Check the links"}"#);
    assert_eq!((allow, again), (Done::Done, Done::Done));
    assert_eq!(
        other.unwrap(),
        Done::Refused {
            why: AnswerRefusal::NothingWaits {
                wait: "#1".to_owned()
            }
        }
    );
    assert_eq!(repeat, reserved);
    let made = agents(&realm).await;
    assert_eq!(
        made.iter().map(|(agent, _)| *agent).collect::<Vec<_>>(),
        [Hand::NUMBER.0, MAYOR.0, THREE.0],
        "one hire is made"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_reserved_end_changes_nothing() {
    // a hamlet whose limits reserve end. Its mayor ends hire#3: the
    // act waits; the chief denies it with a reason. hire#3 stays
    // live, nothing is decided, and the mayor is told today's words.
    let (realm, hamlet) = hamlet_with(1).await;
    reserve(&realm, &hamlet, ReservedAct::End).await;
    let decided = decisions(&realm).await;

    let ended =
        realm.tool(MAYOR, &tool("e1", end(THREE))).await.unwrap();
    let denied = realm
        .ask(&answer("#1", Choice::Deny, Some("keep it")))
        .await
        .unwrap();
    let told = seen_submits(&realm, MAYOR, 1).await;

    assert_eq!(ended, kept(1));
    assert_eq!(denied, Done::Done);
    assert!(
        agents(&realm).await.contains(&(THREE.0, "live".to_owned())),
        "hire#3 stays live"
    );
    assert_eq!(
        decisions(&realm).await,
        decided,
        "nothing is decided"
    );
    let notices = told
        .iter()
        .filter_map(|submit| match &submit.input {
            Input::Notice { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(notices, ["the chief refused it: keep it"]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_reserved_call_answers_the_same_wait() {
    // a hamlet whose limits reserve roads. The mayor's lay_road with
    // one call id, sent twice, keeps one wait and answers it twice.
    let (realm, hamlet) = hamlet_with(2).await;
    reserve(&realm, &hamlet, ReservedAct::Roads).await;
    let laying = tool("r1", lay(THREE, FOUR));

    let first = realm.tool(MAYOR, &laying).await.unwrap();
    let again = realm.tool(MAYOR, &laying).await.unwrap();

    assert_eq!((first, again), (kept(1), kept(1)));
    let opened = waits(&realm).await;
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(roads(&realm).await.is_empty(), "no road is laid");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreserved_tool_runs_at_once() {
    // a hamlet whose limits reserve roads only: the mayor's hire runs
    // at once. With hire reserved too, the Hand's hire is not kept:
    // the Hand has no hamlet whose limits reserve its acts. No wait
    // opens.
    let (realm, hamlet) = hamlet_with(0).await;
    reserve(&realm, &hamlet, ReservedAct::Roads).await;

    let hiring = realm.tool(MAYOR, &tool("c1", hire(None))).await;
    reserve(&realm, &hamlet, ReservedAct::Hire).await;
    let by_hand = realm
        .tool(
            Hand::NUMBER,
            &tool(
                "c2",
                Hire {
                    hamlet: Some(hamlet.clone()),
                    ..hire(None)
                },
            ),
        )
        .await
        .unwrap();

    assert_eq!(hiring.unwrap(), hired(3));
    assert!(
        !matches!(
            by_hand,
            HireAnswer::Refused {
                why: LeadRefusal::Gate(Gate::Reserved { .. })
            }
        ),
        "{by_hand:?}"
    );
    assert!(waits(&realm).await.is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_reserved_hire_is_decided_and_told() {
    // a hamlet whose limits reserve hire. The mayor's hire, once
    // allowed, writes its decision, and the mayor is told the new
    // hire's number in today's words.
    let (realm, hamlet) = hamlet_with(0).await;
    reserve(&realm, &hamlet, ReservedAct::Hire).await;
    let decided = decisions(&realm).await;

    realm.tool(MAYOR, &tool("c1", hire(None))).await.unwrap();
    realm.ask(&answer("#1", Choice::Allow, None)).await.unwrap();

    assert_eq!(notices(&realm, MAYOR).await, ["Hired hire#3."]);
    assert_eq!(
        decisions(&realm).await[decided.len()..],
        ["hired".to_owned()]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_reserved_end_ends_the_hire() {
    // the wait shows today's tool and arguments; once allowed, hire#3
    // has ended, the end is decided, and the mayor is told.
    let (realm, hamlet) = hamlet_with(1).await;
    reserve(&realm, &hamlet, ReservedAct::End).await;
    let decided = decisions(&realm).await;

    let ended =
        realm.tool(MAYOR, &tool("e1", end(THREE))).await.unwrap();
    let subject = waits(&realm).await[0].2.clone();
    let allowed =
        realm.ask(&answer("#1", Choice::Allow, None)).await.unwrap();

    assert_eq!((ended, allowed), (kept(1), Done::Done));
    assert_eq!(
        subject,
        r#"end_hire {"hire":"hire#3","reason":"done"}"#
    );
    assert!(
        agents(&realm)
            .await
            .contains(&(THREE.0, "ended".to_owned())),
        "hire#3 ended"
    );
    assert_eq!(
        decisions(&realm).await[decided.len()..],
        ["ended".to_owned()]
    );
    assert_eq!(notices(&realm, MAYOR).await, ["Ended hire#3."]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn reserved_roads_are_laid_and_taken_up_once_allowed() {
    // each wait shows today's tool and arguments; each allowed act is
    // decided and told in today's words.
    let (realm, hamlet) = hamlet_with(2).await;
    reserve(&realm, &hamlet, ReservedAct::Roads).await;
    let decided = decisions(&realm).await;

    let laid = realm.tool(MAYOR, &tool("r1", lay(THREE, FOUR))).await;
    realm.ask(&answer("#1", Choice::Allow, None)).await.unwrap();
    let open = roads(&realm).await;
    let taken =
        realm.tool(MAYOR, &tool("r2", take_up(THREE, FOUR))).await;
    realm.ask(&answer("#2", Choice::Allow, None)).await.unwrap();

    assert_eq!(laid.unwrap(), kept(1));
    assert_eq!(open, [(THREE.0, FOUR.0, false)]);
    assert_eq!(taken.unwrap(), kept(2));
    assert_eq!(roads(&realm).await, [(THREE.0, FOUR.0, true)]);
    let subjects = waits(&realm)
        .await
        .into_iter()
        .map(|(_, _, subject, _)| subject)
        .collect::<Vec<_>>();
    assert_eq!(
        subjects,
        [
            r#"lay_road {"a":"hire#3","b":"hire#4"}"#,
            r#"take_up_road {"a":"hire#3","b":"hire#4"}"#,
        ]
    );
    assert_eq!(
        decisions(&realm).await[decided.len()..],
        ["road".to_owned(), "unroad".to_owned()]
    );
    assert_eq!(
        notices(&realm, MAYOR).await,
        [
            "Laid a road between hire#3 and hire#4.",
            "Took up the road between hire#3 and hire#4.",
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_act_that_no_longer_holds_changes_nothing() {
    // the mayor's reserved end of hire#3 waits; meanwhile the chief
    // ends hire#3 itself. The allow closes the wait, the end is not
    // decided a second time, and the mayor is told the refusal the
    // end met.
    let (realm, hamlet) = hamlet_with(1).await;
    reserve(&realm, &hamlet, ReservedAct::End).await;
    let ended =
        realm.tool(MAYOR, &tool("e1", end(THREE))).await.unwrap();
    let by_chief = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: end(THREE),
        })
        .await
        .unwrap();
    let decided = decisions(&realm).await;

    let allowed =
        realm.ask(&answer("#1", Choice::Allow, None)).await.unwrap();

    assert_eq!((ended, by_chief), (kept(1), Done::Done));
    assert_eq!(allowed, Done::Done);
    assert_eq!(decisions(&realm).await, decided);
    let [(1, _, _, Some(choice))] = &waits(&realm).await[..] else {
        panic!("the wait is closed");
    };
    assert_eq!(choice, "allow");
    let told = notices(&realm, MAYOR).await;
    assert!(
        told.iter()
            .any(|told| told.starts_with("`hire#3` has left")),
        "{told:?}"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workers_act_is_never_reserved() {
    // a hamlet whose limits reserve hire. The mayor starts worker#3;
    // the worker's hire is answered at once, and no wait opens.
    let (realm, hamlet) = hamlet_with(0).await;
    reserve(&realm, &hamlet, ReservedAct::Hire).await;
    let started = realm
        .tool(
            MAYOR,
            &tool(
                "w1",
                StartWorker {
                    brief: "count the files".to_owned(),
                    tools: Vec::new(),
                    task: None,
                    hamlet: None,
                },
            ),
        )
        .await
        .unwrap();

    let hiring = realm.tool(THREE, &tool("c1", hire(None))).await;

    assert_eq!(started, Started::Started { worker: THREE });
    let hiring = hiring.unwrap();
    assert!(
        !matches!(
            hiring,
            HireAnswer::Refused {
                why: LeadRefusal::Gate(Gate::Reserved { .. })
            }
        ),
        "{hiring:?}"
    );
    assert!(waits(&realm).await.is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reserved_call_the_chief_refused_is_refused_again_unasked()
{
    // a hamlet whose limits reserve hire. On a chief's task the
    // chief denies the mayor's hire with a reason. The same hire on
    // that task is refused at once with today's words and opens no
    // wait; a command on the task still waits for the chief.
    let (realm, hamlet) = hamlet_with(0).await;
    reserve(&realm, &hamlet, ReservedAct::Hire).await;
    let task = chief_task(&realm, MAYOR).await;
    let first = realm
        .tool(MAYOR, &in_turn("h1", hire(None), Some(&task)))
        .await
        .unwrap();
    realm
        .ask(&answer("#1", Choice::Deny, Some("not now")))
        .await
        .unwrap();

    let again = realm
        .tool(MAYOR, &in_turn("h2", hire(None), Some(&task)))
        .await
        .unwrap();
    realm
        .call_as_workspace(&NeedsChief {
            realm_task: Some(task),
            ..needs(MAYOR, "c1")
        })
        .await
        .unwrap();

    assert_eq!(
        first,
        HireAnswer::Refused {
            why: LeadRefusal::Gate(Gate::Reserved {
                wait: WaitId(1)
            }),
        }
    );
    assert_eq!(
        refused_with(again),
        "the chief already refused this in this turn (not now). Do \
         not ask again: go on without it, or report that you could \
         not."
    );
    let kinds = waits(&realm)
        .await
        .into_iter()
        .map(|(id, kind, _, choice)| (id, kind, choice))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            (1, "reserved".to_owned(), Some("refuse".to_owned())),
            (2, "tool".to_owned(), None),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rewind_lapses_the_reserved_wait_of_its_conversation() {
    // a hamlet whose limits reserve hire. The mayor's hire in
    // conversation c waits; a rewind of c closes the wait, so a later
    // allow finds nothing waiting and no hire is made.
    let (realm, hamlet) = hamlet_with(0).await;
    reserve(&realm, &hamlet, ReservedAct::Hire).await;
    realm
        .tool(MAYOR, &in_turn("h1", hire(None), None))
        .await
        .unwrap();

    realm
        .tool(
            MAYOR,
            &Lapse {
                conversation: Some(ConversationId("c".to_owned())),
                why: LapseReason::Rewound,
            },
        )
        .await
        .unwrap();
    let allowed =
        realm.ask(&answer("#1", Choice::Allow, None)).await.unwrap();

    assert_eq!(
        allowed,
        Done::Refused {
            why: AnswerRefusal::NothingWaits {
                wait: "#1".to_owned()
            }
        }
    );
    let [(1, kind, _, Some(choice))] = &waits(&realm).await[..]
    else {
        panic!("one wait, closed");
    };
    assert_eq!(
        (kind.as_str(), choice.as_str()),
        ("reserved", "lapsed")
    );
    assert!(
        !agents(&realm)
            .await
            .iter()
            .any(|(agent, _)| *agent == THREE.0),
        "no hire is made"
    );
    realm.host.shutdown().await.unwrap();
}
