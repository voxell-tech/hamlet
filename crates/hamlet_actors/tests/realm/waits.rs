use std::time::Duration;

use hamlet_messages::agent::Input;
use hamlet_messages::realm::budget::{Payer, ResetBudget};
use hamlet_messages::realm::client::AnswerRefusal;
use hamlet_messages::realm::models::{Cause, TurnFailed};
use hamlet_messages::realm::oversight::{
    AskChief, Lapse, LapseReason, Object, Question,
};
use hamlet_messages::realm::world::WaitKind;
use hamlet_messages::realm::{
    Asked, Done, End, Hire, HireAnswer, Limit, Named, NeedsChief,
    ReservedAct, SecretList, Secrets, SetLimits, SocietyTool,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::{
    Footing, Need, SecretsVersion,
};
use hamlet_messages::workspace::{
    self, AgentNumber, ChiefAnswer, ConversationId, RealmTask,
};
use hamlet_protocol::client::{Choice, RequestId};
use serde::Serialize;

use super::reserved::{in_turn, refused_with};
use super::{
    Callee, MAYOR, Realm, answer, chief_hires, error_code, founded,
    hamlet_with, hire, needs, say, seen_submits, standing, tool,
    until,
};

/// A reserved act as its wait's detail keeps it.
#[derive(Serialize)]
struct Keeping<A> {
    tool: SocietyTool,
    act: A,
}

/// Writes an open wait of `kind` for the Hand in conversation `c`, as
/// the realm raises one.
async fn raw_wait(
    realm: &Realm,
    kind: &str,
    subject: &str,
    detail: &impl Serialize,
) {
    let mut bytes = Vec::new();
    ciborium::into_writer(detail, &mut bytes).unwrap();
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    realm
        .write(&format!(
            "INSERT INTO wait (agent, kind, conversation, subject, \
             detail, at) VALUES ({}, '{kind}', 'c', '{}', X'{hex}', \
             0)",
            HAND.0,
            subject.replace('\'', "''")
        ))
        .await;
}

fn nothing_waits(wait: &str) -> Done<AnswerRefusal> {
    Done::Refused {
        why: AnswerRefusal::NothingWaits {
            wait: wait.to_owned(),
        },
    }
}

async fn waits_of(
    realm: &Realm,
    agent: AgentNumber,
) -> Vec<(i64, bool, Option<String>)> {
    realm
        .stopped_rows(|db| {
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
    let hired = chief_hires(&realm, &hamlet, hire(None)).await;
    let HireAnswer::Hired { agent } = hired else {
        panic!("the chief hires: {hired:?}");
    };
    let hire = agent.agent;
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

const HAND: AgentNumber = Hand::NUMBER;

const PAST_THE_COUNT: &str =
    "the chief has refused 3 requests this turn and will not be \
     asked again in it. Stop and report what you could not do.";

/// The Hand's call `call`, needing `need`, in `conversation`.
fn asked(
    call: &str,
    need: Need,
    task: Option<&RealmTask>,
    conversation: &str,
) -> NeedsChief {
    NeedsChief {
        conversation: ConversationId(conversation.to_owned()),
        realm_task: task.cloned(),
        needs: vec![need],
        subject: format!("run {call}"),
        ..needs(HAND, call)
    }
}

fn failed(task: &RealmTask, cause: Cause) -> TurnFailed {
    TurnFailed {
        task: task.clone(),
        conversation: ConversationId("c".to_owned()),
        words: "the model fell over".to_owned(),
        cause,
    }
}

fn lapse(conversation: Option<&str>, why: LapseReason) -> Lapse {
    Lapse {
        conversation: conversation.map(|conversation| {
            ConversationId(conversation.to_owned())
        }),
        why,
    }
}

fn lapsed(call: &str, lapse: workspace::Lapse) -> ChiefAnswer {
    ChiefAnswer {
        agent: HAND,
        call: needs(HAND, call).call,
        answer: workspace::Answer::Lapsed { lapse },
    }
}

fn refused_unasked(call: &str, words: &str) -> ChiefAnswer {
    ChiefAnswer {
        agent: HAND,
        call: needs(HAND, call).call,
        answer: workspace::Answer::Refuse {
            reason: Some(words.to_owned()),
        },
    }
}

/// The task of a chief's line to `agent`.
async fn chief_task(realm: &Realm, agent: AgentNumber) -> RealmTask {
    let line = say(&RequestId::new(), agent, "go");
    realm.ask(&line).await.unwrap();
    let submits = seen_submits(realm, agent, 1).await;
    submits[0].task.clone().expect("a line carries its task")
}

/// The footing of the last reach the workspace took for `agent`.
fn footing(realm: &Realm, agent: AgentNumber) -> Footing {
    realm
        .seen
        .reaches
        .lock()
        .unwrap()
        .iter()
        .rfind(|set| set.agent == agent)
        .and_then(|set| set.reach.footing().cloned())
        .expect("the workspace holds a living reach")
}

async fn kinds_of(realm: &Realm) -> Vec<(String, bool)> {
    realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT kind, closed_at IS NULL FROM wait ORDER BY \
                 id",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
        })
        .await
}

async fn ruling_of(realm: &Realm, wait: i64) -> Option<String> {
    realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT ruling FROM wait WHERE id = ?1",
                [wait],
                |row| row.get(0),
            )
        })
        .await
}

async fn kept_reaches(realm: &Realm) -> (i64, i64) {
    realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT (SELECT count(*) FROM grant), (SELECT \
                 count(*) FROM trust)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .await
}

/// The first `count` notices `agent` was given, with their
/// conversations.
async fn notices(
    realm: &Realm,
    agent: AgentNumber,
    count: usize,
) -> Vec<(String, Option<String>)> {
    seen_submits(realm, agent, count)
        .await
        .into_iter()
        .filter_map(|submit| match submit.input {
            Input::Notice { text, conversation } => {
                Some((text, conversation.map(|c| c.0)))
            }
            _ => None,
        })
        .collect()
}

fn not_offered() -> Done<AnswerRefusal> {
    Done::Refused {
        why: AnswerRefusal::NotOffered,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_always_on_the_network_writes_a_grant_before_the_answer() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let network = asked("c1", Need::Network, None, "c");
    realm.call_as_workspace(&network).await.unwrap();
    assert!(!footing(&realm, HAND).network);
    realm.seen.hold(Callee::SetReach);

    let always =
        realm.ask(&answer("1", Choice::Always, None)).await.unwrap();
    until("the Hand's new reach waits", || {
        realm.seen.is_waiting(Callee::SetReach, HAND)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let early = chief_answers(&realm);
    realm.seen.release(Callee::SetReach);
    until("the workspace has the chief's answer", || {
        !chief_answers(&realm).is_empty()
    })
    .await;

    assert_eq!(always, Done::Done);
    assert!(early.is_empty(), "answered before the reach: {early:?}");
    assert!(footing(&realm, HAND).network);
    assert_eq!(
        chief_answers(&realm),
        [ChiefAnswer {
            agent: HAND,
            call: network.call,
            answer: workspace::Answer::Allow,
        }]
    );
    assert_eq!(kept_reaches(&realm).await, (1, 0));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_always_is_refused_where_today_refuses_it() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let task = standing(&realm, HAND);
    realm
        .tool(HAND, &failed(&task, Cause::Model))
        .await
        .unwrap();
    let question = WaitKind::Question {
        message: None,
        questions: vec![Question {
            question: "Oats or barley?".to_owned(),
            description: None,
            options: Vec::new(),
            text_input: true,
        }],
    };
    raw_wait(&realm, "question", "Oats or barley?", &question).await;

    let fault =
        realm.ask(&answer("1", Choice::Always, None)).await.unwrap();
    let question =
        realm.ask(&answer("2", Choice::Always, None)).await.unwrap();
    let open = waits_of(&realm, HAND).await;
    let dismissed = realm
        .ask(&answer("1", Choice::Dismiss, None))
        .await
        .unwrap();

    assert_eq!((fault, question), (not_offered(), not_offered()));
    assert_eq!(open, [(1, true, None), (2, true, None)]);
    assert_eq!(dismissed, Done::Done);
    assert_eq!(
        waits_of(&realm, HAND).await,
        [(1, false, Some("dismiss".to_owned())), (2, true, None)]
    );
    assert_eq!(kept_reaches(&realm).await, (0, 0));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn past_the_count_a_tool_wait_is_refused_unasked() {
    let (realm, hamlet) = hamlet_with(0).await;
    let limited = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: SetLimits {
                hamlet: Some(hamlet),
                budget: Vec::new(),
                limits: vec![Limit::Refusals { most: 2 }],
            },
        })
        .await
        .unwrap();
    let task = chief_task(&realm, MAYOR).await;
    let mayors = |call: &str, task: Option<&RealmTask>| NeedsChief {
        agent: MAYOR,
        ..asked(call, Need::Command, task, "c")
    };
    for n in 1..=2 {
        realm
            .call_as_workspace(&mayors(&format!("c{n}"), Some(&task)))
            .await
            .unwrap();
        realm
            .ask(&answer(&n.to_string(), Choice::Deny, Some("no")))
            .await
            .unwrap();
    }

    realm
        .call_as_workspace(&mayors("c3", Some(&task)))
        .await
        .unwrap();
    realm.call_as_workspace(&mayors("c4", None)).await.unwrap();
    until("the workspace has three answers", || {
        chief_answers(&realm).len() == 3
    })
    .await;
    let third = chief_answers(&realm)
        .into_iter()
        .find(|answered| answered.call == mayors("c3", None).call);

    assert_eq!(limited, Done::Done);
    assert_eq!(
        third,
        Some(ChiefAnswer {
            agent: MAYOR,
            ..refused_unasked(
                "c3",
                "the chief has refused 2 requests this turn and \
                 will not be asked again in it. Stop and report \
                 what you could not do."
            )
        })
    );
    let refused = Some("refuse".to_owned());
    assert_eq!(
        waits_of(&realm, MAYOR).await,
        [
            (1, false, refused.clone()),
            (2, false, refused),
            (3, false, Some("unasked".to_owned())),
            (4, true, None),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lapse_closes_the_conversations_waits() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let task = standing(&realm, HAND);
    for (call, conversation) in
        [("c1", "c"), ("c2", "c"), ("c3", "d")]
    {
        realm
            .call_as_workspace(&asked(
                call,
                Need::Command,
                None,
                conversation,
            ))
            .await
            .unwrap();
    }
    realm
        .tool(HAND, &failed(&task, Cause::Model))
        .await
        .unwrap();

    realm
        .tool(HAND, &lapse(Some("c"), LapseReason::Stopped))
        .await
        .unwrap();
    until("the workspace has two answers", || {
        chief_answers(&realm).len() == 2
    })
    .await;
    let after_c = waits_of(&realm, HAND).await;
    realm
        .tool(HAND, &lapse(None, LapseReason::Rewound))
        .await
        .unwrap();
    until("the workspace has three answers", || {
        chief_answers(&realm).len() == 3
    })
    .await;

    let closed = Some("lapsed".to_owned());
    assert_eq!(
        after_c,
        [
            (1, false, closed.clone()),
            (2, false, closed.clone()),
            (3, true, None),
            (4, false, closed),
        ]
    );
    let mut answers = chief_answers(&realm);
    answers.sort_by(|a, b| a.call.0.cmp(&b.call.0));
    assert_eq!(
        answers,
        [
            lapsed("c1", workspace::Lapse::Stopped),
            lapsed("c2", workspace::Lapse::Stopped),
            lapsed("c3", workspace::Lapse::Rewound),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_lapse_drops_the_conversations_trust() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    realm
        .call_as_workspace(&asked("c1", Need::Command, None, "c"))
        .await
        .unwrap();
    realm.ask(&answer("1", Choice::Always, None)).await.unwrap();
    until("the workspace has the chief's answer", || {
        !chief_answers(&realm).is_empty()
    })
    .await;
    let trusted = footing(&realm, HAND).trust;
    realm
        .call_as_workspace(&asked("c2", Need::Command, None, "c"))
        .await
        .unwrap();

    realm
        .tool(HAND, &lapse(Some("c"), LapseReason::Resumed))
        .await
        .unwrap();
    until("the Hand's reach trusts nothing", || {
        footing(&realm, HAND).trust.is_empty()
    })
    .await;

    assert_eq!(trusted.len(), 1);
    assert!(trusted[0].commands && !trusted[0].edits, "{trusted:?}");
    assert_eq!(
        trusted[0].conversation,
        ConversationId("c".to_owned())
    );
    assert_eq!(
        waits_of(&realm, HAND).await,
        [(1, false, Some("allow".to_owned())), (2, true, None)]
    );
    assert_eq!(chief_answers(&realm).len(), 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_always_answers_after_the_reach() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    for call in ["c1", "c2"] {
        realm
            .call_as_workspace(&asked(call, Need::Network, None, "c"))
            .await
            .unwrap();
    }
    realm.seen.hold(Callee::SetReach);

    realm.ask(&answer("1", Choice::Always, None)).await.unwrap();
    until("the Hand's new reach waits", || {
        realm.seen.is_waiting(Callee::SetReach, HAND)
    })
    .await;
    realm.ask(&answer("2", Choice::Always, None)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let early = chief_answers(&realm);
    realm.seen.release(Callee::SetReach);
    until("the workspace has both answers", || {
        chief_answers(&realm).len() == 2
    })
    .await;

    assert!(early.is_empty(), "answered before the reach: {early:?}");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fault_on_no_known_task_opens_no_wait() {
    let realm = Realm::start().await;
    realm.live(HAND).await;

    for task in ["x", "999"] {
        let task = RealmTask(task.to_owned());
        realm
            .tool(HAND, &failed(&task, Cause::Budget))
            .await
            .unwrap();
    }

    assert!(kinds_of(&realm).await.is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn two_faults_open_two_waits() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let task = standing(&realm, HAND);
    let fault = failed(&task, Cause::Model);

    realm.tool(HAND, &fault).await.unwrap();
    realm.tool(HAND, &fault).await.unwrap();

    let open = ("fault".to_owned(), true);
    assert_eq!(kinds_of(&realm).await, [open.clone(), open]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reset_budget_closes_the_budget_faults() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let hamlet = founded(&realm).await;
    let task = standing(&realm, HAND);
    realm
        .tool(HAND, &failed(&task, Cause::Budget))
        .await
        .unwrap();
    realm
        .tool(HAND, &failed(&task, Cause::Model))
        .await
        .unwrap();

    let reset = async |payer| {
        realm
            .ask(&Asked {
                id: RequestId::new(),
                act: ResetBudget { payer },
            })
            .await
            .unwrap()
    };

    assert_eq!(reset(Payer::Hamlet { hamlet }).await, Done::Done);
    assert_eq!(
        waits_of(&realm, HAND).await,
        [(1, true, None), (2, true, None)]
    );
    assert_eq!(reset(Payer::Realm).await, Done::Done);
    assert_eq!(
        waits_of(&realm, HAND).await,
        [(1, false, Some("dismiss".to_owned())), (2, true, None)]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_chiefs_text_with_an_allow_is_kept() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    realm
        .call_as_workspace(&asked("c1", Need::Command, None, "c"))
        .await
        .unwrap();

    let allowed = realm
        .ask(&answer("1", Choice::Allow, Some(" mind the cache ")))
        .await
        .unwrap();
    until("the workspace has the chief's answer", || {
        !chief_answers(&realm).is_empty()
    })
    .await;

    assert_eq!(allowed, Done::Done);
    assert_eq!(
        chief_answers(&realm)[0].answer,
        workspace::Answer::Allow
    );
    assert_eq!(
        ruling_of(&realm, 1).await.as_deref(),
        Some("mind the cache")
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ruling_on_a_question_or_an_objection_is_a_notice() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let first = chief_task(&realm, HAND).await;
    realm
        .ask(&say(&RequestId::new(), HAND, "and this"))
        .await
        .unwrap();
    let second = seen_submits(&realm, HAND, 2).await[1]
        .task
        .clone()
        .expect("a line carries its task");
    let conversation = ConversationId("c".to_owned());
    for call in ["a1", "a2"] {
        let question = AskChief {
            task: first.clone(),
            conversation: conversation.clone(),
            message: None,
            questions: vec![Question {
                question: "Oats or barley?".to_owned(),
                description: None,
                options: Vec::new(),
                text_input: true,
            }],
        };
        let asked = realm.tool(HAND, &tool(call, question)).await;
        assert_eq!(asked.unwrap(), Done::Done);
    }
    for (call, task) in [("o1", &first), ("o2", &second)] {
        let objection = Object {
            task: task.clone(),
            conversation: conversation.clone(),
            reason: "no".to_owned(),
        };
        let objected = realm.tool(HAND, &tool(call, objection)).await;
        assert_eq!(objected.unwrap(), Done::Done);
    }
    let end = Keeping {
        tool: SocietyTool::End,
        act: End {
            hire: Named::Agent {
                agent: AgentNumber(3),
            },
            why: None,
        },
    };
    raw_wait(
        &realm,
        "reserved",
        "end_hire {\"hire\":\"hire#3\"}",
        &end,
    )
    .await;

    let empty = realm
        .ask(&answer("1", Choice::Reply, Some(" ")))
        .await
        .unwrap();
    for (wait, choice, text) in [
        ("1", Choice::Reply, Some("plant oats")),
        ("2", Choice::Dismiss, None),
        ("3", Choice::Go, Some("carry on")),
        ("4", Choice::Withdraw, None),
        ("5", Choice::Deny, None),
    ] {
        let ruled =
            realm.ask(&answer(wait, choice, text)).await.unwrap();
        assert_eq!(ruled, Done::Done);
    }

    assert_eq!(
        empty,
        Done::Refused {
            why: AnswerRefusal::BlankReply
        }
    );
    let mut told = notices(&realm, HAND, 7)
        .await
        .into_iter()
        .map(|(text, conversation)| {
            assert_eq!(conversation.as_deref(), Some("c"));
            text
        })
        .collect::<Vec<_>>();
    told.sort();
    assert_eq!(
        told,
        [
            "The chief answers: plant oats",
            "The chief did not answer; go on with what you know.",
            "The chief rules: go on with the instruction. The chief \
             says: carry on",
            "The chief withdrew the instruction. Stop working on it.",
            "the chief refused it: no reason given",
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_the_chief_refused_is_refused_again_unasked() {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let task = chief_task(&realm, HAND).await;
    let command = |call: &str, subject: &str| NeedsChief {
        subject: subject.to_owned(),
        ..asked(call, Need::Command, Some(&task), "c")
    };
    realm
        .call_as_workspace(&command("c1", "make clean"))
        .await
        .unwrap();
    realm.ask(&answer("1", Choice::Deny, None)).await.unwrap();
    realm
        .call_as_workspace(&asked(
            "c2",
            Need::Network,
            Some(&task),
            "c",
        ))
        .await
        .unwrap();
    realm
        .ask(&answer("2", Choice::Deny, Some("stay offline")))
        .await
        .unwrap();

    for asked in [
        command("c3", "make clean"),
        asked("c4", Need::Network, Some(&task), "c"),
        command("c5", "make all"),
    ] {
        realm.call_as_workspace(&asked).await.unwrap();
    }
    until("the workspace has four answers", || {
        chief_answers(&realm).len() == 4
    })
    .await;

    let answers = chief_answers(&realm);
    let answer_to = |call: &str| {
        answers
            .iter()
            .find(|answer| answer.call == needs(HAND, call).call)
            .cloned()
    };
    assert_eq!(
        [answer_to("c3"), answer_to("c4")],
        [
            Some(refused_unasked(
                "c3",
                "the chief already refused this in this turn (no \
                 reason given). Do not ask again: go on without it, \
                 or report that you could not."
            )),
            Some(refused_unasked(
                "c4",
                "the chief already refused this in this turn (stay \
                 offline). Do not ask again: go on without it, or \
                 report that you could not."
            )),
        ]
    );
    let open = waits_of(&realm, HAND).await;
    assert_eq!(open[4], (5, true, None));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_refusal_that_reaches_the_count_closes_the_tasks_other_waits()
 {
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let task = chief_task(&realm, HAND).await;
    for n in 1..=4 {
        realm
            .call_as_workspace(&asked(
                &format!("c{n}"),
                Need::Command,
                Some(&task),
                "c",
            ))
            .await
            .unwrap();
    }
    realm
        .call_as_workspace(&asked("c5", Need::Command, None, "c"))
        .await
        .unwrap();

    for wait in ["1", "2", "3"] {
        realm.ask(&answer(wait, Choice::Deny, None)).await.unwrap();
    }
    until("the workspace has four answers", || {
        chief_answers(&realm).len() == 4
    })
    .await;

    assert!(
        chief_answers(&realm)
            .contains(&refused_unasked("c4", PAST_THE_COUNT)),
        "{:?}",
        chief_answers(&realm)
    );
    let waits = waits_of(&realm, HAND).await;
    assert_eq!(
        waits[3..],
        [(4, false, Some("unasked".to_owned())), (5, true, None)]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn denied_reserved_calls_count_toward_the_tasks_refusals() {
    let (realm, hamlet) = hamlet_with(0).await;
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act: SetLimits {
                hamlet: Some(hamlet),
                budget: Vec::new(),
                limits: vec![
                    Limit::Refusals { most: 2 },
                    Limit::Reserved {
                        act: ReservedAct::Hire,
                        reserved: true,
                    },
                ],
            },
        })
        .await
        .unwrap();
    let task = chief_task(&realm, MAYOR).await;
    let hiring = |call: &str, purpose: &str| {
        in_turn(
            call,
            Hire {
                purpose: Some(purpose.to_owned()),
                ..hire(None)
            },
            Some(&task),
        )
    };
    let command = |call: &str| NeedsChief {
        agent: MAYOR,
        ..asked(call, Need::Command, Some(&task), "c")
    };
    realm.tool(MAYOR, &hiring("h1", "one")).await.unwrap();
    realm
        .ask(&answer("1", Choice::Deny, Some("no")))
        .await
        .unwrap();
    realm.call_as_workspace(&command("c1")).await.unwrap();
    realm.tool(MAYOR, &hiring("h2", "two")).await.unwrap();
    realm
        .ask(&answer("3", Choice::Deny, Some("no")))
        .await
        .unwrap();

    let third =
        realm.tool(MAYOR, &hiring("h3", "three")).await.unwrap();
    realm.call_as_workspace(&command("c2")).await.unwrap();
    until("the workspace has two answers", || {
        chief_answers(&realm).len() == 2
    })
    .await;

    let past = "the chief has refused 2 requests this turn and will \
                not be asked again in it. Stop and report what you \
                could not do.";
    assert_eq!(refused_with(third), past);
    assert_eq!(
        chief_answers(&realm),
        [
            ChiefAnswer {
                agent: MAYOR,
                ..refused_unasked("c1", past)
            },
            ChiefAnswer {
                agent: MAYOR,
                ..refused_unasked("c2", past)
            },
        ]
    );
    realm.host.shutdown().await.unwrap();
}
