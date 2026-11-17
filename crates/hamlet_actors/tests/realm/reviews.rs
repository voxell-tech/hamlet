use std::time::{Duration, Instant};

use hamlet_actors::realm;
use hamlet_messages::address;
use hamlet_messages::agent::{AgentKind, Brief, Input, Submit};
use hamlet_messages::realm::choices::{ModelOf, SetModel};
use hamlet_messages::realm::client::SayAnswer;
use hamlet_messages::realm::documents::{
    EditSettings, Endpoint, Setting,
};
use hamlet_messages::realm::oversight::{Object, ObjectRefusal};
use hamlet_messages::realm::workers::{Ended, ErrandDone};
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, End, HamletId, Limit, Refusable as _,
    Review, SetLimits, Tell,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, ErrandNumber, RealmTask,
};
use hamlet_protocol::client::{Choice, RequestId};

use super::chiefs::chief;
use super::{
    Callee, FOUR, MAYOR, Realm, Seen, THREE, answer, founded, hires,
    say, submits_to, tell, to, tool, until,
};

/// A hamlet whose mayor leads 3 and 4, where a review lasts at most
/// `deadline`, and lines are reviewed as `review` says.
async fn hamlet(
    deadline: Duration,
    review: Review,
) -> (Realm, HamletId) {
    let options = realm::Options {
        review_deadline: deadline,
        ..realm::Options::default()
    };
    let realm =
        Realm::start_with(Some(options), Seen::default()).await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    realm.live(MAYOR).await;
    for (n, call) in [(3, "h0"), (4, "h1")] {
        hires(&realm, MAYOR, call).await;
        realm.live(AgentNumber(n)).await;
    }
    reviews(&realm, &hamlet, review).await;
    (realm, hamlet)
}

async fn reviews(realm: &Realm, hamlet: &HamletId, review: Review) {
    let set = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: SetLimits {
                hamlet: Some(hamlet.clone()),
                budget: Vec::new(),
                limits: vec![Limit::Review { review }],
            },
        })
        .await
        .unwrap();
    assert_eq!(set, Done::Done);
}

fn errand(n: u32) -> AgentNumber {
    ErrandNumber::new(n).unwrap().agent()
}

/// The lines `agent` was given, with their notes.
fn lines(
    realm: &Realm,
    agent: AgentNumber,
) -> Vec<(String, Option<String>)> {
    submits_to(realm, agent)
        .into_iter()
        .filter_map(|Submit { input, .. }| match input {
            Input::Line { text, note, .. } => Some((text, note)),
            _ => None,
        })
        .collect()
}

async fn lines_until(
    realm: &Realm,
    agent: AgentNumber,
    count: usize,
) -> Vec<(String, Option<String>)> {
    until(
        &format!("agent {} was given {count} lines", agent.0),
        || lines(realm, agent).len() >= count,
    )
    .await;
    lines(realm, agent)
}

/// Says `text` to 3, as a chief.
async fn tells_three(realm: &Realm, text: &str) -> SayAnswer {
    realm
        .ask(&say(&RequestId::new(), THREE, text))
        .await
        .unwrap()
}

/// The kind, brief and task errand `n` was made with, once it is.
async fn briefed(
    realm: &Realm,
    n: u32,
) -> (AgentKind, Brief, String) {
    let agent = errand(n);
    let made = || {
        realm
            .seen
            .made
            .lock()
            .unwrap()
            .iter()
            .find(|(made, _)| *made == agent)
            .map(|(_, input)| input.clone())
    };
    until(&format!("errand {n} is made"), || made().is_some()).await;
    let input = made().unwrap();
    (input.kind, input.brief.unwrap(), input.task.0)
}

async fn ends(realm: &Realm, n: u32, ended: Ended) {
    realm
        .call_as(&address::Agent(errand(n)), &ErrandDone { ended })
        .await
        .unwrap();
}

fn answered(text: &str) -> Ended {
    Ended::Answered {
        text: text.to_owned(),
    }
}

async fn outcomes(realm: &Realm) -> Vec<Option<String>> {
    realm
        .stopped_rows(|db| {
            db.prepare("SELECT outcome FROM review ORDER BY task")?
                .query_map([], |row| row.get(0))?
                .collect()
        })
        .await
}

/// The open waits of 3: id, kind and subject.
async fn waits_of_three(realm: &Realm) -> Vec<(i64, String, String)> {
    realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT id, kind, subject FROM wait WHERE agent = \
                 ?1 AND closed_at IS NULL ORDER BY id",
            )?
            .query_map([THREE.0], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect()
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_passed_line_reaches_the_hire_once_with_the_note() {
    let deadline = Duration::from_secs(2);
    let (realm, _) = hamlet(deadline, Review::Always).await;

    let said = tells_three(&realm, "Plain line.").await;
    let (kind, brief, task) = briefed(&realm, 0).await;
    let before = lines(&realm, THREE);
    ends(&realm, 0, answered("PASS: mind the tests")).await;
    let given = lines_until(&realm, THREE, 1).await;
    ends(&realm, 0, answered("PASS")).await;
    tokio::time::sleep(deadline * 2).await;

    let SayAnswer::Taken { task: said } = said else {
        panic!("the line is taken: {said:?}");
    };
    assert_eq!(task, said.0);
    assert_eq!(kind, AgentKind::Errand);
    assert_eq!(
        brief.instructions.as_deref(),
        Some(
            "You are `hire#2`, and you lead `hire#3`, hired to \
             Check the links. A chief has just instructed `hire#3` \
             directly. The work has begun already, and the chief \
             always decides. If the instruction is a mistake for \
             the work you lead, answer `OBJECT: <reason>`. \
             Otherwise answer `PASS`. Most instructions are \
             fine.\n\n# The hires you lead\n- hire#3: Check the \
             links\n- hire#4: Check the links"
        )
    );
    assert_eq!(brief.text, "A chief told `hire#3`: Plain line.");
    assert!(before.is_empty(), "{before:?}");
    assert_eq!(
        given,
        [(
            "Plain line.".to_owned(),
            Some(
                "hire#2 reviewed this line: mind the tests"
                    .to_owned()
            )
        )]
    );
    assert_eq!(lines(&realm, THREE).len(), 1);
    assert_eq!(outcomes(&realm).await, [Some("passed".to_owned())]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_objection_holds_the_line_past_the_deadline() {
    let deadline = Duration::from_secs(1);
    let (realm, _) = hamlet(deadline, Review::Always).await;

    tells_three(&realm, "Odd line.").await;
    briefed(&realm, 0).await;
    ends(
        &realm,
        0,
        answered("OBJECT: the odd line breaks the plan."),
    )
    .await;
    tokio::time::sleep(deadline * 3).await;
    let waits = waits_of_three(&realm).await;
    let dismissed = realm
        .ask(&answer(&waits[0].0.to_string(), Choice::Dismiss, None))
        .await
        .unwrap();

    assert_eq!(waits.len(), 1, "{waits:?}");
    assert_eq!(waits[0].1, "objection");
    assert_eq!(
        waits[0].2,
        "A chief told `hire#3`: Odd line.\nhire#2 objects: the odd \
         line breaks the plan."
    );
    assert!(
        matches!(dismissed, Done::Refused { .. }),
        "an objection takes go or withdraw: {dismissed:?}"
    );
    assert!(lines(&realm, THREE).is_empty());
    assert_eq!(outcomes(&realm).await, [Some("objected".to_owned())]);
    realm.host.shutdown().await.unwrap();
}

/// The notices `agent` was given.
fn notices(realm: &Realm, agent: AgentNumber) -> Vec<String> {
    submits_to(realm, agent)
        .into_iter()
        .filter_map(|Submit { input, .. }| match input {
            Input::Notice { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// The lead objects to "Odd line." to 3, and the chief's wait on 3
/// opens: its id.
async fn objected(realm: &Realm) -> String {
    tells_three(realm, "Odd line.").await;
    briefed(realm, 0).await;
    ends(realm, 0, answered("OBJECT: the odd line breaks the plan."))
        .await;
    waits_of_three(realm).await[0].0.to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_objected_line_waits_for_go() {
    let (realm, _) =
        hamlet(Duration::from_secs(1), Review::Always).await;

    let wait = objected(&realm).await;
    let ruled =
        realm.ask(&answer(&wait, Choice::Go, None)).await.unwrap();
    let given = lines_until(&realm, THREE, 1).await;
    outcomes(&realm).await;

    assert_eq!(ruled, Done::Done);
    assert_eq!(given, [("Odd line.".to_owned(), None)]);
    assert_eq!(notices(&realm, THREE), Vec::<String>::new());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_line_never_reaches_the_hire() {
    let deadline = Duration::from_secs(1);
    let (realm, _) = hamlet(deadline, Review::Always).await;

    let wait = objected(&realm).await;
    let ruled = realm
        .ask(&answer(&wait, Choice::Withdraw, None))
        .await
        .unwrap();
    outcomes(&realm).await;
    let realm = realm.restart().await;
    tokio::time::sleep(deadline * 2).await;
    outcomes(&realm).await;

    assert_eq!(ruled, Done::Done);
    assert_eq!(submits_to(&realm, THREE), Vec::new());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_does_not_wait_for_an_open_review() {
    let (realm, _) =
        hamlet(Duration::from_secs(60), Review::Always).await;
    tells_three(&realm, "Plain line.").await;
    briefed(&realm, 0).await;

    let stopping = Instant::now();
    let realm = realm.restart().await;
    let took = stopping.elapsed();
    ends(&realm, 0, answered("PASS")).await;
    let given = lines_until(&realm, THREE, 1).await;

    assert!(took < Duration::from_secs(2), "{took:?}");
    assert_eq!(given, [("Plain line.".to_owned(), None)]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_review_lets_the_line_through() {
    let deadline = Duration::from_secs(1);
    let (realm, _) = hamlet(deadline, Review::Always).await;
    let said = Instant::now();

    tells_three(&realm, "Plain line.").await;
    briefed(&realm, 0).await;
    let given = lines_until(&realm, THREE, 1).await;

    assert!(said.elapsed() >= deadline, "{:?}", said.elapsed());
    assert_eq!(given, [("Plain line.".to_owned(), None)]);
    assert_eq!(outcomes(&realm).await, [Some("failed".to_owned())]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_review_lets_the_line_through() {
    let (realm, _) =
        hamlet(Duration::from_secs(60), Review::Always).await;

    tells_three(&realm, "Plain line.").await;
    briefed(&realm, 0).await;
    ends(
        &realm,
        0,
        Ended::Failed {
            why: "the model failed".to_owned(),
        },
    )
    .await;
    let given = lines_until(&realm, THREE, 1).await;

    assert_eq!(given, [("Plain line.".to_owned(), None)]);
    assert_eq!(outcomes(&realm).await, [Some("failed".to_owned())]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_review_survives_a_restart_with_the_time_left() {
    let deadline = Duration::from_secs(4);
    let (realm, _) = hamlet(deadline, Review::Always).await;
    let said = Instant::now();

    tells_three(&realm, "Plain line.").await;
    briefed(&realm, 0).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let realm = realm.restart().await;
    let given = lines_until(&realm, THREE, 1).await;
    let took = said.elapsed();
    tokio::time::sleep(Duration::from_secs(1)).await;

    assert!(took >= deadline, "{took:?}");
    assert!(took < deadline + Duration::from_secs(2), "{took:?}");
    assert_eq!(given, [("Plain line.".to_owned(), None)]);
    assert_eq!(lines(&realm, THREE).len(), 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_of_the_answer_and_the_deadline_wins() {
    let deadline = Duration::from_secs(1);
    let (realm, _) = hamlet(deadline, Review::Always).await;

    tells_three(&realm, "Late line.").await;
    briefed(&realm, 0).await;
    lines_until(&realm, THREE, 1).await;
    ends(&realm, 0, answered("OBJECT: too late")).await;
    tells_three(&realm, "Early line.").await;
    briefed(&realm, 1).await;
    ends(&realm, 1, answered("PASS")).await;
    lines_until(&realm, THREE, 2).await;
    tokio::time::sleep(deadline * 3).await;

    assert_eq!(
        lines(&realm, THREE),
        [
            ("Late line.".to_owned(), None),
            ("Early line.".to_owned(), None),
        ]
    );
    assert!(waits_of_three(&realm).await.is_empty());
    assert_eq!(
        outcomes(&realm).await,
        [Some("failed".to_owned()), Some("passed".to_owned())]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_led_hire_is_reviewed_as_the_limits_say() {
    let (realm, hamlet) =
        hamlet(Duration::from_secs(60), Review::Always).await;
    let settings = async |set: Vec<Setting>| {
        let edited = realm
            .ask(&Asked {
                id: RequestId::new(),
                act: EditSettings { set },
            })
            .await
            .unwrap();
        assert_eq!(edited, Done::Done);
    };
    let endpoints = Setting::Endpoints {
        endpoints: vec![
            Endpoint {
                name: "near".to_owned(),
                base_url: "http://127.0.0.1:11434/v1".to_owned(),
            },
            Endpoint {
                name: "far".to_owned(),
                base_url: "https://models.example/v1".to_owned(),
            },
        ],
    };

    realm
        .ask(&say(&RequestId::new(), MAYOR, "Mayor's line."))
        .await
        .unwrap();
    let to_the_mayor = lines_until(&realm, MAYOR, 1).await;
    reviews(&realm, &hamlet, Review::Remote).await;
    tells_three(&realm, "No model.").await;
    lines_until(&realm, THREE, 1).await;
    settings(vec![
        endpoints,
        Setting::Model {
            model: "near::small".to_owned(),
        },
    ])
    .await;
    tells_three(&realm, "Near model.").await;
    lines_until(&realm, THREE, 2).await;
    settings(vec![Setting::Model {
        model: "far::big".to_owned(),
    }])
    .await;
    tells_three(&realm, "Far model.").await;
    let (_, brief, _) = briefed(&realm, 0).await;
    ends(&realm, 0, answered("PASS")).await;
    lines_until(&realm, THREE, 3).await;
    reviews(&realm, &hamlet, Review::Never).await;
    tells_three(&realm, "Never.").await;
    let given = lines_until(&realm, THREE, 4).await;

    assert_eq!(to_the_mayor, [("Mayor's line.".to_owned(), None)]);
    assert_eq!(brief.model.as_deref(), Some("far::big"));
    assert_eq!(
        given.into_iter().map(|(text, _)| text).collect::<Vec<_>>(),
        ["No model.", "Near model.", "Far model.", "Never."]
    );
    assert_eq!(outcomes(&realm).await, [Some("passed".to_owned())]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_review_runs_on_the_leads_own_model() {
    let (realm, _) =
        hamlet(Duration::from_secs(60), Review::Remote).await;
    let edited = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: EditSettings {
                set: vec![
                    Setting::Endpoints {
                        endpoints: vec![
                            Endpoint {
                                name: "near".to_owned(),
                                base_url: "http://127.0.0.1:11434/v1"
                                    .to_owned(),
                            },
                            Endpoint {
                                name: "far".to_owned(),
                                base_url: "https://models.example/v1"
                                    .to_owned(),
                            },
                        ],
                    },
                    Setting::Model {
                        model: "near::small".to_owned(),
                    },
                ],
            },
        })
        .await
        .unwrap();
    let chosen = chief(
        &realm,
        SetModel {
            of: ModelOf::Hire { hire: to(MAYOR) },
            model: Some("far::big".to_owned()),
            effort: None,
            fast: None,
            why: None,
        },
    )
    .await;

    tells_three(&realm, "Check the far model.").await;
    let (_, brief, _) = briefed(&realm, 0).await;
    ends(&realm, 0, answered("PASS")).await;
    lines_until(&realm, THREE, 1).await;

    assert_eq!((edited, chosen), (Done::Done, Done::Done));
    assert_eq!(brief.model.as_deref(), Some("far::big"));
    assert_eq!(outcomes(&realm).await, [Some("passed".to_owned())]);
    realm.host.shutdown().await.unwrap();
}

/// The errands' ends: errand number and how it ended.
async fn errands_ended(realm: &Realm) -> Vec<(u32, Option<String>)> {
    realm
        .stopped_rows(|db| {
            db.prepare("SELECT n, ended FROM errand ORDER BY n")?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect()
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_review_of_an_ended_hire_gives_nothing() {
    let (realm, _) =
        hamlet(Duration::from_secs(60), Review::Always).await;
    tells_three(&realm, "Plain line.").await;
    briefed(&realm, 0).await;
    realm
        .ask(&say(&RequestId::new(), FOUR, "Odd line."))
        .await
        .unwrap();
    briefed(&realm, 1).await;
    for hire in [THREE, FOUR] {
        let ended = chief(
            &realm,
            End {
                hire: to(hire),
                why: None,
            },
        )
        .await;
        assert_eq!(ended, Done::Done);
    }

    ends(&realm, 0, answered("PASS: fine")).await;
    ends(&realm, 1, answered("OBJECT: odd")).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let open = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT COUNT(*) FROM wait WHERE closed_at IS NULL",
                [],
                |row| row.get::<_, u32>(0),
            )
        })
        .await;

    assert_eq!(lines(&realm, THREE), []);
    assert_eq!(lines(&realm, FOUR), []);
    assert_eq!(open, 0);
    assert_eq!(
        outcomes(&realm).await,
        [Some("passed".to_owned()), Some("objected".to_owned())]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_cannot_object_to_a_line_it_was_not_given() {
    let (realm, _) =
        hamlet(Duration::from_secs(60), Review::Always).await;
    tells_three(&realm, "Odd line.").await;
    let (_, _, task) = briefed(&realm, 0).await;

    let objected = realm
        .tool(
            THREE,
            &tool(
                "o1",
                Object {
                    task: RealmTask(task),
                    conversation: ConversationId("c".to_owned()),
                    reason: "no".to_owned(),
                },
            ),
        )
        .await
        .unwrap();
    ends(&realm, 0, answered("OBJECT: odd")).await;
    let waits = waits_of_three(&realm).await;

    assert_eq!(
        objected,
        Done::refused(AgentRefusal::Act(
            ObjectRefusal::NotAChiefsLine
        ))
    );
    assert_eq!(
        errands_ended(&realm).await,
        [(0, Some("answered".to_owned()))]
    );
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert_eq!(
        waits[0].2,
        "A chief told `hire#3`: Odd line.\nhire#2 objects: odd"
    );
    assert!(lines(&realm, THREE).is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_line_on_go_still_gives_the_held_letters() {
    let (realm, _) =
        hamlet(Duration::from_secs(60), Review::Always).await;
    let wait = objected(&realm).await;
    let (_, _, task) = briefed(&realm, 0).await;
    let told = realm
        .tool(
            MAYOR,
            &tool(
                "t1",
                Tell {
                    task: Some(RealmTask(task)),
                    ..tell(to(FOUR), "Wait for the chief.")
                },
            ),
        )
        .await
        .unwrap();
    let before = submits_to(&realm, FOUR);
    realm.seen.fail_once(Callee::Submit, THREE);

    let ruled =
        realm.ask(&answer(&wait, Choice::Go, None)).await.unwrap();
    until("giving 3 the line failed", || {
        realm.seen.has_failed(Callee::Submit, THREE)
    })
    .await;
    outcomes(&realm).await;
    let letters = submits_to(&realm, FOUR)
        .into_iter()
        .filter_map(|Submit { input, .. }| match input {
            Input::Letter { text, .. } => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!((told, ruled), (Done::Done, Done::Done));
    assert_eq!(before, []);
    assert_eq!(letters, ["Wait for the chief."]);
    realm.host.shutdown().await.unwrap();
}
