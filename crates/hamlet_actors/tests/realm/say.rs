use std::time::Duration;

use hamlet_actors::realm;
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::realm::client::{SayAnswer, SayRefusal};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{Choice, RequestId};

use super::asking::{chief_task, given, objection, open_waits};
use super::{
    Callee, FOUR, MAYOR, Realm, Seen, THREE, answer, error_code,
    founded, hamlet_with, paid, say, shown, submits_to, tell, to,
    tool, until,
};

async fn mayor_held_at_its_branch(realm: &Realm) {
    realm.live(Hand::NUMBER).await;
    realm.seen.hold(Callee::Branch);
    founded(realm).await;
    until("the mayor's drive waits on the held branch", || {
        realm.seen.is_waiting(Callee::Branch, MAYOR)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_say_opens_one_task_and_submits_again() {
    // say twice with one id: one task; two Submits with one InputId.
    // The same id with another text is refused as reused.
    let (realm, _) = hamlet_with(0).await;
    let id = RequestId::new();
    let line = say(&id, MAYOR, "build the mill");

    let first = realm.ask(&line).await.unwrap();
    let again = realm.ask(&line).await.unwrap();
    let other = realm.ask(&say(&id, MAYOR, "build the barn")).await;
    let tasks = realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT id FROM task WHERE agent = 2 AND origin = \
                 'chief'",
            )?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await;

    let SayAnswer::Taken { task } = &first else {
        panic!("the mayor takes the line: {first:?}");
    };
    assert_eq!(again, first);
    assert_eq!(error_code(other), "message.id_reused");
    assert_eq!(tasks.len(), 1);
    assert_eq!(task.0, tasks[0].to_string());
    let submit = Submit {
        id: InputId(id.as_str().to_owned()),
        task: Some(task.clone()),
        input: Input::Line {
            text: "build the mill".to_owned(),
            images: Vec::new(),
            files: Vec::new(),
            note: None,
            chief: Some("chief".to_owned()),
        },
    };
    assert_eq!(submits_to(&realm, MAYOR), [submit.clone(), submit]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_say_to_a_hiring_hire_waits_for_its_steps() {
    // hold the mayor's branch; say to it while `hiring`; release
    // while the say waits: `Taken` once it is live. With a wait
    // shorter than the hold, `StillHiring`, and nothing submitted.
    let wait = |hiring_wait| {
        Some(realm::Options {
            hiring_wait,
            ..realm::Options::default()
        })
    };
    let realm = Realm::start_with(
        wait(Duration::from_secs(10)),
        Seen::default(),
    )
    .await;
    mayor_held_at_its_branch(&realm).await;
    let line = say(&RequestId::new(), MAYOR, "build the mill");

    let (answer, submitted_while_held) =
        tokio::join!(realm.ask(&line), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let submitted = !submits_to(&realm, MAYOR).is_empty();
            realm.seen.release(Callee::Branch);
            submitted
        });
    let answer = answer.unwrap();

    assert!(!submitted_while_held, "the say waits for the steps");
    assert!(
        matches!(answer, SayAnswer::Taken { .. }),
        "the mayor takes the line once live: {answer:?}"
    );
    assert_eq!(submits_to(&realm, MAYOR).len(), 1);
    realm.host.shutdown().await.unwrap();

    let realm = Realm::start_with(
        wait(Duration::from_millis(200)),
        Seen::default(),
    )
    .await;
    mayor_held_at_its_branch(&realm).await;

    let answer = realm
        .ask(&say(&RequestId::new(), MAYOR, "build the mill"))
        .await
        .unwrap();
    realm.seen.release(Callee::Branch);

    assert_eq!(
        answer,
        SayAnswer::Refused {
            why: SayRefusal::StillHiring {
                agent: shown(MAYOR)
            }
        }
    );
    assert!(submits_to(&realm, MAYOR).is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_to_no_agent_names_its_address() {
    let (realm, _) = hamlet_with(0).await;

    let said = realm
        .ask(&say(&RequestId::new(), AgentNumber(9), "Hello."))
        .await
        .unwrap();

    let SayAnswer::Refused { why } = said else {
        panic!("a line to no agent is refused: {said:?}");
    };
    assert_eq!(why.to_string(), "no agent has the address `9`");
    realm.host.shutdown().await.unwrap();
}

/// The first line's delivery is held at the agent while the second
/// is said. The test looks for the second line at the agent for
/// 500 ms, over a hundred times what a delivery that is not held
/// takes here; the order it asserts holds however long that look
/// lasts.
#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_two_lines_to_one_agent_reach_it_in_the_order_said()
{
    let (realm, _) = hamlet_with(1).await;
    let (first, second) = (RequestId::new(), RequestId::new());
    realm.seen.hold_next(Callee::Submit, THREE);

    let (dig, rest) = (
        say(&first, THREE, "dig the well"),
        say(&second, THREE, "then rest"),
    );
    let said_first = realm.ask(&dig);
    let then_second = async {
        until("the first line waits at the agent", || {
            realm.seen.is_waiting(Callee::Submit, THREE)
        })
        .await;
        let said_second = realm.ask(&rest);
        let looked = async {
            let _ = tokio::time::timeout(
                Duration::from_millis(500),
                until("the second line reaches the agent", || {
                    !submits_to(&realm, THREE).is_empty()
                }),
            )
            .await;
            realm.seen.release(Callee::Submit);
        };
        tokio::join!(said_second, looked).0
    };
    let (first_said, second_said) =
        tokio::join!(said_first, then_second);

    assert!(
        matches!(first_said.unwrap(), SayAnswer::Taken { .. }),
        "the first line was not taken"
    );
    assert!(
        matches!(second_said.unwrap(), SayAnswer::Taken { .. }),
        "the second line was not taken"
    );
    let ids = submits_to(&realm, THREE)
        .into_iter()
        .map(|submit| submit.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [first, second].map(|id| InputId(id.as_str().to_owned()))
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_held_at_one_agent_does_not_hold_a_line_to_another() {
    let (realm, _) = hamlet_with(2).await;
    realm.seen.hold_next(Callee::Submit, THREE);
    let held = say(&RequestId::new(), THREE, "dig the well");
    let other = say(&RequestId::new(), FOUR, "fetch water");

    let said_held = realm.ask(&held);
    let said_other = async {
        until("the line to hire 3 waits at it", || {
            realm.seen.is_waiting(Callee::Submit, THREE)
        })
        .await;
        let said = realm.ask(&other).await;
        let reached = (
            submits_to(&realm, THREE).len(),
            submits_to(&realm, FOUR).len(),
        );
        realm.seen.release(Callee::Submit);
        (said, reached)
    };
    let (said_held, (said_other, reached)) =
        tokio::join!(said_held, said_other);

    assert!(matches!(said_other.unwrap(), SayAnswer::Taken { .. }));
    assert_eq!(reached, (0, 1));
    assert!(matches!(said_held.unwrap(), SayAnswer::Taken { .. }));
    realm.host.shutdown().await.unwrap();
}

/// A chief's go on 3's objection tells 3 and gives the mayor the
/// letter 3 sent on that task while it objected. The notice is held
/// at 3; the test looks for the letter at the mayor for 5 s, over a
/// hundred times what a delivery that is not held takes here.
#[tokio::test(flavor = "multi_thread")]
async fn an_acts_delivery_held_at_one_agent_does_not_hold_its_delivery_to_another()
 {
    let (realm, _) = hamlet_with(1).await;
    let task = chief_task(&realm, THREE).await;
    realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    realm
        .tool(
            THREE,
            &paid("t1", &task, tell(to(MAYOR), "it is done")),
        )
        .await
        .unwrap();
    let waits = open_waits(&realm, THREE).await;
    let before = submits_to(&realm, THREE).len();
    realm.seen.hold_next(Callee::Submit, THREE);

    realm
        .ask(&answer(&waits[0].0.to_string(), Choice::Go, None))
        .await
        .unwrap();
    until("the notice waits at 3", || {
        realm.seen.is_waiting(Callee::Submit, THREE)
    })
    .await;
    let reached = tokio::time::timeout(
        Duration::from_secs(5),
        until("the letter reaches the mayor", || {
            given(&realm, MAYOR, 0)
                .contains(&"letter: it is done".to_owned())
        }),
    )
    .await;
    realm.seen.release(Callee::Submit);

    assert!(reached.is_ok(), "the letter waited for 3's notice");
    until("the notice reaches 3", || {
        submits_to(&realm, THREE).len() > before
    })
    .await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_that_fails_to_reach_an_agent_lets_the_next_one_through()
 {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.fail_once(Callee::Submit, THREE);
    let next = RequestId::new();

    let failed = realm
        .ask(&say(&RequestId::new(), THREE, "dig the well"))
        .await
        .unwrap();
    let taken =
        realm.ask(&say(&next, THREE, "then rest")).await.unwrap();

    assert!(
        matches!(failed, SayAnswer::NotTaken { .. }),
        "{failed:?}"
    );
    assert!(matches!(taken, SayAnswer::Taken { .. }), "{taken:?}");
    let ids = submits_to(&realm, THREE)
        .into_iter()
        .map(|submit| submit.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, [InputId(next.as_str().to_owned())]);
    realm.host.shutdown().await.unwrap();
}
