use std::time::Duration;

use hamlet_messages::agent::Input;
use hamlet_messages::realm::budget::{Payer, ResetBudget};
use hamlet_messages::realm::client::AnswerRefusal;
use hamlet_messages::realm::models::{Cause, TurnFailed};
use hamlet_messages::realm::oversight::{Lapse, LapseReason};
use hamlet_messages::realm::{
    Asked, Done, End, HireAnswer, Named, NeedsChief, SecretList,
    Secrets,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::{
    Footing, Need, SecretsVersion,
};
use hamlet_messages::workspace::{
    self, AgentNumber, ChiefAnswer, ConversationId, RealmTask,
};
use hamlet_protocol::client::{Choice, RequestId};

use super::{
    Callee, Realm, answer, chief_hires, error_code, founded, hire,
    needs, seen_submits, standing, until,
};

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
    let hired = chief_hires(&realm, &hamlet, hire(None)).await;
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
        .rows(|db| {
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
        .rows(|db| {
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
        .rows(|db| {
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
    // a network wait of the Hand, while the workspace holds its
    // set_reach. The chief answers always: Done, and nothing reaches
    // the workspace while the Hand's new reach waits; once it is
    // taken, the reach gives the network and the workspace gets
    // Allow.
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
    // a failed turn's wait and a question's wait offer no always:
    // each is refused NotOffered and stays open, and nothing is
    // granted or trusted. The fault is then dismissed.
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let task = standing(&realm, HAND);
    realm
        .tool(HAND, &failed(&task, Cause::Model))
        .await
        .unwrap();
    realm
        .write(&format!(
            "INSERT INTO wait (agent, kind, conversation, at) \
             VALUES ({}, 'question', 'c', 0)",
            HAND.0
        ))
        .await;

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
    // three commands on the Hand's task, each refused by the chief.
    // A fourth on that task is refused at once with today's words,
    // its wait kept closed; one on no task still waits.
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let task = standing(&realm, HAND);
    for n in 1..=3 {
        let call = format!("c{n}");
        realm
            .call_as_workspace(&asked(
                &call,
                Need::Command,
                Some(&task),
                "c",
            ))
            .await
            .unwrap();
        realm
            .ask(&answer(&n.to_string(), Choice::Deny, Some("no")))
            .await
            .unwrap();
    }

    realm
        .call_as_workspace(&asked(
            "c4",
            Need::Command,
            Some(&task),
            "c",
        ))
        .await
        .unwrap();
    realm
        .call_as_workspace(&asked("c5", Need::Command, None, "c"))
        .await
        .unwrap();
    until("the workspace has four answers", || {
        chief_answers(&realm).len() == 4
    })
    .await;

    assert_eq!(
        chief_answers(&realm)[3],
        ChiefAnswer {
            agent: HAND,
            call: needs(HAND, "c4").call,
            answer: workspace::Answer::Refuse {
                reason: Some(PAST_THE_COUNT.to_owned()),
            },
        }
    );
    let refused = Some("refuse".to_owned());
    assert_eq!(
        waits_of(&realm, HAND).await,
        [
            (1, false, refused.clone()),
            (2, false, refused.clone()),
            (3, false, refused),
            (4, false, Some("unasked".to_owned())),
            (5, true, None),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lapse_closes_the_conversations_waits() {
    // two waits in conversation c, one in d, and a failed turn in c.
    // A lapse of c as stopped closes the three of c and tells the
    // workspace of its two tool waits; the wait in d stays open until
    // a lapse of all, as rewound.
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

    let lapsed_ = Some("lapsed".to_owned());
    assert_eq!(
        after_c,
        [
            (1, false, lapsed_.clone()),
            (2, false, lapsed_.clone()),
            (3, true, None),
            (4, false, lapsed_),
        ]
    );
    assert_eq!(
        chief_answers(&realm),
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
    // the chief answers always to a command in conversation c: the
    // Hand's reach trusts c with commands. A lapse of c as resumed
    // pushes a reach without that trust and leaves a wait in c open.
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
    // a failed turn for the budget and one for the model, both on the
    // Hand's task, which the realm pays. The chief resets the realm's
    // budget: the budget's wait closes, the model's stays open.
    let realm = Realm::start().await;
    realm.live(HAND).await;
    let task = standing(&realm, HAND);
    realm
        .tool(HAND, &failed(&task, Cause::Budget))
        .await
        .unwrap();
    realm
        .tool(HAND, &failed(&task, Cause::Model))
        .await
        .unwrap();

    let reset = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: ResetBudget {
                payer: Payer::Realm,
            },
        })
        .await
        .unwrap();

    assert_eq!(reset, Done::Done);
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
    // a question, a second question and an objection of the Hand's,
    // in conversation c. A reply, a dismiss, and a go with words
    // each give the Hand a notice in c, in today's words; a reply
    // with no words is refused.
    let realm = Realm::start().await;
    realm.live(HAND).await;
    realm
        .write(&format!(
            "INSERT INTO wait (agent, kind, conversation, at) \
             VALUES ({0}, 'question', 'c', 0), ({0}, 'question', \
             'c', 0), ({0}, 'objection', 'c', 0)",
            HAND.0
        ))
        .await;

    let empty = realm
        .ask(&answer("1", Choice::Reply, Some(" ")))
        .await
        .unwrap();
    for (wait, choice, text) in [
        ("1", Choice::Reply, Some("plant oats")),
        ("2", Choice::Dismiss, None),
        ("3", Choice::Go, Some("carry on")),
    ] {
        let ruled =
            realm.ask(&answer(wait, choice, text)).await.unwrap();
        assert_eq!(ruled, Done::Done);
    }

    assert_eq!(empty, not_offered());
    let c = Some("c".to_owned());
    assert_eq!(
        notices(&realm, HAND, 3).await,
        [
            ("The chief answers: plant oats".to_owned(), c.clone()),
            (
                "The chief did not answer; go on with what you know."
                    .to_owned(),
                c.clone()
            ),
            (
                "The chief rules: go on with the instruction. The \
                 chief says: carry on"
                    .to_owned(),
                c
            ),
        ]
    );
    realm.host.shutdown().await.unwrap();
}
