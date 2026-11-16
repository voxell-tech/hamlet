use hamlet_messages::agent::Input;
use hamlet_messages::realm::client::AnswerRefusal;
use hamlet_messages::realm::workers::{StartWorker, Started};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{
    Asked, Done, End, Gate, HamletId, Hire, HireAnswer, LayRoad,
    LeadRefusal, Limit, ReservedAct, SetLimits, TakeUpRoad,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{Choice, RequestId};

use super::{
    FOUR, MAYOR, Realm, THREE, answer, hamlet_with, hire, hired,
    seen_submits, to, tool,
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
    assert!(
        subject.starts_with("hire {")
            && subject.contains("Check the links"),
        "the wait shows the act: {subject}"
    );
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
async fn an_allowed_reserved_end_ends_the_hire() {
    let (realm, hamlet) = hamlet_with(1).await;
    reserve(&realm, &hamlet, ReservedAct::End).await;

    let ended =
        realm.tool(MAYOR, &tool("e1", end(THREE))).await.unwrap();
    let allowed =
        realm.ask(&answer("#1", Choice::Allow, None)).await.unwrap();

    assert_eq!((ended, allowed), (kept(1), Done::Done));
    assert!(
        agents(&realm)
            .await
            .contains(&(THREE.0, "ended".to_owned())),
        "hire#3 ended"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn reserved_roads_are_laid_and_taken_up_once_allowed() {
    let (realm, hamlet) = hamlet_with(2).await;
    reserve(&realm, &hamlet, ReservedAct::Roads).await;

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
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_act_that_no_longer_holds_changes_nothing() {
    // the mayor's reserved end of hire#3 waits; meanwhile the chief
    // ends hire#3 itself. The allow closes the wait and the end is
    // not decided a second time.
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
