use std::time::Duration;

use hamlet_actors::realm;
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::realm::client::{SayAnswer, SayRefusal};
use hamlet_messages::sender::Hand;
use hamlet_protocol::client::RequestId;

use super::{
    Callee, MAYOR, Realm, Seen, error_code, founded, hamlet_with,
    say, submits_to, until,
};

async fn mayor_held_at_its_branch(realm: &Realm) {
    realm.live(Hand::NUMBER).await;
    realm.seen.hold(Callee::Branch);
    founded(realm).await;
    until("the mayor's drive waits on the held branch", || {
        realm.seen.held.receiver_count() > 0
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
        .rows(|db| {
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
    let wait = |hiring_wait| Some(realm::Options { hiring_wait });
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
            why: SayRefusal::StillHiring { agent: MAYOR }
        }
    );
    assert!(submits_to(&realm, MAYOR).is_empty());
    realm.host.shutdown().await.unwrap();
}
