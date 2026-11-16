use hamlet_messages::agent::{Input, Submit};
use hamlet_messages::realm::budget::{
    MaySpend, OpenTask, SpendRefusal, TaskOpened, TaskTurn,
};
use hamlet_messages::realm::client::AnswerRefusal;
use hamlet_messages::realm::oversight::{
    AskChief, AskChiefRefusal, Hold, Lapse, LapseReason, Object,
    ObjectRefusal, Offer, Question,
};
use hamlet_messages::realm::{
    AgentRefusal, Ask, Done, LetterId, Reply, Tell,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
use hamlet_protocol::client::{Choice, Pick, RequestId};

use super::{
    FOUR, MAYOR, Realm, THREE, answer, hamlet_with, say, standing,
    submits_to, tell, to, tool, until, words,
};

fn conversation() -> ConversationId {
    ConversationId("c".to_owned())
}

fn is_line(submit: &Submit) -> bool {
    matches!(submit.input, Input::Line { .. })
}

/// The task of a new chief's line to `agent`, as the line carried
/// it.
async fn chief_task(realm: &Realm, agent: AgentNumber) -> RealmTask {
    let lines = || {
        submits_to(realm, agent)
            .iter()
            .filter(|s| is_line(s))
            .count()
    };
    let before = lines();
    realm
        .ask(&say(&RequestId::new(), agent, "Make the file."))
        .await
        .unwrap();
    until("the line reaches its agent", || lines() > before).await;
    submits_to(realm, agent)
        .into_iter()
        .rfind(is_line)
        .and_then(|submit| submit.task)
        .expect("a line carries its task")
}

fn asked(task: &RealmTask, questions: Vec<Question>) -> AskChief {
    AskChief {
        task: task.clone(),
        conversation: conversation(),
        message: None,
        questions,
    }
}

fn plain(question: &str) -> Question {
    Question {
        question: question.to_owned(),
        description: None,
        options: Vec::new(),
        text_input: true,
    }
}

fn offering(question: &str, titles: &[&str]) -> Question {
    Question {
        options: titles
            .iter()
            .map(|title| Offer {
                title: (*title).to_owned(),
                description: None,
            })
            .collect(),
        text_input: false,
        ..plain(question)
    }
}

fn objection(task: &RealmTask, reason: &str) -> Object {
    Object {
        task: task.clone(),
        conversation: conversation(),
        reason: reason.to_owned(),
    }
}

fn refused<R>(why: R) -> Done<AgentRefusal<R>> {
    Done::Refused {
        why: AgentRefusal::Act(why),
    }
}

async fn may_spend(
    realm: &Realm,
    agent: AgentNumber,
    task: &RealmTask,
) -> Done<SpendRefusal> {
    let may = MaySpend { task: task.clone() };
    realm.tool(agent, &may).await.unwrap()
}

fn held(task: &RealmTask) -> Done<SpendRefusal> {
    Done::Refused {
        why: SpendRefusal::Held(Hold::Objection {
            task: task.clone(),
        }),
    }
}

/// The open waits of `agent`: id, kind and subject.
async fn open_waits(
    realm: &Realm,
    agent: AgentNumber,
) -> Vec<(i64, String, String)> {
    realm
        .rows(|db| {
            db.prepare(
                "SELECT id, kind, subject FROM wait WHERE agent = \
                 ?1 AND closed_at IS NULL ORDER BY id",
            )?
            .query_map([agent.0], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect()
        })
        .await
}

/// What `agent` was given after its first `skip` inputs, in words.
fn given(
    realm: &Realm,
    agent: AgentNumber,
    skip: usize,
) -> Vec<String> {
    submits_to(realm, agent)
        .into_iter()
        .skip(skip)
        .map(|Submit { input, .. }| match input {
            Input::Notice { text, .. } => format!("notice: {text}"),
            Input::Letter { text, .. } => format!("letter: {text}"),
            Input::Question { text, .. } => {
                format!("question: {text}")
            }
            Input::Answer { text, .. } => format!("answer: {text}"),
            other => format!("{other:?}"),
        })
        .collect()
}

async fn given_until(
    realm: &Realm,
    agent: AgentNumber,
    skip: usize,
    count: usize,
) -> Vec<String> {
    until(
        &format!("agent {} was given {count} more", agent.0),
        || given(realm, agent, skip).len() >= count,
    )
    .await;
    given(realm, agent, skip)
}

/// The question the mayor asks 3 on `task`, as 3 was given it.
async fn the_mayor_asks_three(
    realm: &Realm,
    task: &RealmTask,
) -> LetterId {
    let ask = Ask {
        to: to(THREE),
        text: "Which file?".to_owned(),
        conversation: ConversationId("mayor".to_owned()),
        task: Some(task.clone()),
        paths: Vec::new(),
    };
    let asked = realm.tool(MAYOR, &tool("q1", ask)).await.unwrap();
    assert_eq!(asked, Done::Done);
    let question = || {
        submits_to(realm, THREE).into_iter().find_map(|submit| {
            match submit.input {
                Input::Question { letter, .. } => Some(letter),
                _ => None,
            }
        })
    };
    until("3 is asked", || question().is_some()).await;
    question().unwrap()
}

fn on(task: &RealmTask, text: &str) -> Tell {
    Tell {
        task: Some(task.clone()),
        ..tell(to(THREE), text)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ask_chief_answers_at_once_and_the_reply_is_a_notice() {
    // on a chief's line the Hand asks two questions, one with
    // options: Done at once, and an open question wait whose subject
    // is today's numbered words. Picks that do not fit, and words
    // for two questions or for options only, are refused with today's
    // words and leave it open; the chief's picks are given to the
    // Hand as a notice under the task, each question quoted over its
    // answer. A plain question is answered in words, and a dismissed
    // one says so.
    let (realm, _) = hamlet_with(0).await;
    let task = chief_task(&realm, Hand::NUMBER).await;
    let before = submits_to(&realm, Hand::NUMBER).len();
    let two = asked(
        &task,
        vec![
            offering(" Paper or ebook? ", &["Paper", "Ebook"]),
            plain("Budget?"),
        ],
    );
    let picked = |picks| {
        let mut chief = answer("1", Choice::Reply, None);
        chief.0.picks = Some(picks);
        chief
    };
    let words = |text: &str| Pick::Words {
        text: text.to_owned(),
    };

    let first =
        realm.tool(Hand::NUMBER, &tool("a1", two)).await.unwrap();
    let waits = open_waits(&realm, Hand::NUMBER).await;
    for (picks, why) in [
        (vec![Pick::Offer { index: 0 }], AnswerRefusal::Unequal),
        (
            vec![words("both"), words("30")],
            AnswerRefusal::TakesOption { question: 1 },
        ),
        (
            vec![Pick::Offer { index: 2 }, words("30")],
            AnswerRefusal::NoOption { question: 1 },
        ),
        (
            vec![Pick::Offer { index: 0 }, words(" ")],
            AnswerRefusal::NoWords { question: 2 },
        ),
    ] {
        let ruled = realm.ask(&picked(picks)).await.unwrap();
        assert_eq!(ruled, Done::Refused { why });
    }
    let in_words = realm
        .ask(&answer("1", Choice::Reply, Some("paper, 30")))
        .await
        .unwrap();
    let still = open_waits(&realm, Hand::NUMBER).await.len();
    let ruled = realm
        .ask(&picked(vec![
            Pick::Offer { index: 1 },
            words(" under 30 "),
        ]))
        .await
        .unwrap();
    let one = asked(&task, vec![plain("Plant now?")]);
    realm
        .tool(Hand::NUMBER, &tool("a2", one.clone()))
        .await
        .unwrap();
    realm
        .ask(&answer("2", Choice::Reply, Some("yes, plant")))
        .await
        .unwrap();
    realm.tool(Hand::NUMBER, &tool("a3", one)).await.unwrap();
    realm
        .ask(&answer("3", Choice::Dismiss, None))
        .await
        .unwrap();
    let told = given_until(&realm, Hand::NUMBER, before, 3).await;
    let other = chief_task(&realm, Hand::NUMBER).await;
    let options_only =
        asked(&other, vec![offering("Which?", &["Left", "Right"])]);
    realm
        .tool(Hand::NUMBER, &tool("a4", options_only))
        .await
        .unwrap();
    let not_an_option = realm
        .ask(&answer("4", Choice::Reply, Some("the left")))
        .await
        .unwrap();

    assert_eq!(first, Done::Done);
    assert_eq!(
        waits,
        [(
            1,
            "question".to_owned(),
            "1. Paper or ebook?\n   a) Paper\n   b) Ebook\n2. \
             Budget?"
                .to_owned()
        )]
    );
    assert_eq!(
        (in_words, not_an_option),
        (
            Done::Refused {
                why: AnswerRefusal::Unequal
            },
            Done::Refused {
                why: AnswerRefusal::TakesOption { question: 1 }
            }
        )
    );
    assert_eq!((still, ruled), (1, Done::Done));
    assert_eq!(
        told,
        [
            "notice: The chief answers:\n> Paper or \
             ebook?\nEbook\n\n> Budget?\nunder 30",
            "notice: The chief answers: yes, plant",
            "notice: The chief did not answer; go on with what you \
             know.",
        ]
    );
    let notice = &submits_to(&realm, Hand::NUMBER)[before];
    assert_eq!(notice.task.as_ref(), Some(&task));
    assert_eq!(open_waits(&realm, Hand::NUMBER).await.len(), 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ask_chief_needs_a_chief_and_stops_at_the_hamlets_count() {
    // on its standing task the Hand has no chief; on a retry's task
    // it has one. On a chief's line it asks three times, and a fourth
    // is refused with no wait opened.
    let (realm, _) = hamlet_with(0).await;
    let own = standing(&realm, Hand::NUMBER);
    let task = chief_task(&realm, Hand::NUMBER).await;
    let opened = realm
        .tool(
            Hand::NUMBER,
            &OpenTask {
                turn: TaskTurn::Retry,
            },
        )
        .await
        .unwrap();
    let TaskOpened::Opened { task: retry } = opened else {
        panic!("the Hand opens a retry's task: {opened:?}");
    };
    let ask = |on: &RealmTask| asked(on, vec![plain("Why?")]);

    let unseen = realm
        .tool(Hand::NUMBER, &tool("s", ask(&own)))
        .await
        .unwrap();
    let on_retry = realm
        .tool(Hand::NUMBER, &tool("r", ask(&retry)))
        .await
        .unwrap();
    let mut answers = Vec::new();
    for call in ["a1", "a2", "a3", "a4"] {
        answers.push(
            realm
                .tool(Hand::NUMBER, &tool(call, ask(&task)))
                .await
                .unwrap(),
        );
    }

    assert_eq!(unseen, refused(AskChiefRefusal::NoChief));
    assert_eq!(on_retry, Done::Done);
    assert_eq!(
        answers,
        [
            Done::Done,
            Done::Done,
            Done::Done,
            refused(AskChiefRefusal::TooMany { most: 3 })
        ]
    );
    assert_eq!(open_waits(&realm, Hand::NUMBER).await.len(), 4);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_misshapen_question_is_refused_as_today() {
    let (realm, _) = hamlet_with(0).await;
    let task = chief_task(&realm, Hand::NUMBER).await;
    let mut crowded =
        offering("Which?", &["a", "b", "c", "d", "e", "f"]);
    crowded.options.push(Offer {
        title: "g".to_owned(),
        description: None,
    });
    let cases = [
        (Vec::new(), AskChiefRefusal::NoQuestions),
        (vec![plain("q"); 11], AskChiefRefusal::TooManyQuestions),
        (
            vec![plain("q"), plain("  ")],
            AskChiefRefusal::Blank { question: 2 },
        ),
        (vec![crowded], AskChiefRefusal::Crowded { question: 1 }),
        (
            vec![offering("Which?", &["a", " "])],
            AskChiefRefusal::Untitled { question: 1 },
        ),
        (
            vec![offering("Which?", &["a", " a"])],
            AskChiefRefusal::Twice {
                question: 1,
                title: "a".to_owned(),
            },
        ),
    ];

    for (n, (questions, why)) in cases.into_iter().enumerate() {
        let call = format!("m{n}");
        let answered = realm
            .tool(Hand::NUMBER, &tool(&call, asked(&task, questions)))
            .await
            .unwrap();
        assert_eq!(answered, refused(why));
    }
    assert!(open_waits(&realm, Hand::NUMBER).await.is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_objection_holds_spending_and_letters_until_go() {
    // 3 objects to a chief's line: Done at once, and an objection
    // wait with its reason. Its spending on that task is held, on its
    // own task it is not; a letter to 3 and an answer to the mayor on
    // that task are kept, a letter on another task is delivered. Go
    // gives 3 the chief's words, then the kept letter, and the mayor
    // the kept answer; 3's spending goes on.
    let (realm, _) = hamlet_with(1).await;
    let task = chief_task(&realm, THREE).await;
    let question = the_mayor_asks_three(&realm, &task).await;
    let mayor_before = submits_to(&realm, MAYOR).len();
    let before = submits_to(&realm, THREE).len();

    let objected = realm
        .tool(
            THREE,
            &tool("o1", objection(&task, "better left alone")),
        )
        .await
        .unwrap();
    let spending = may_spend(&realm, THREE, &task).await;
    let own =
        may_spend(&realm, THREE, &standing(&realm, THREE)).await;
    let mayors = standing(&realm, MAYOR);
    realm
        .tool(MAYOR, &tool("t1", on(&task, "the file is in /tmp")))
        .await
        .unwrap();
    realm
        .tool(MAYOR, &tool("t2", on(&mayors, "lunch at noon")))
        .await
        .unwrap();
    let reply = Reply {
        question,
        text: "the second".to_owned(),
    };
    realm.tool(THREE, &reply).await.unwrap();
    let unheld = given_until(&realm, THREE, before, 1).await;
    // Reading the rows stops the realm, which first makes every
    // delivery its acts so far have started.
    let waits = open_waits(&realm, THREE).await;
    let mayor_unheld = given(&realm, MAYOR, mayor_before);
    let ruled = realm
        .ask(&answer(
            &waits[0].0.to_string(),
            Choice::Go,
            Some("go on"),
        ))
        .await
        .unwrap();
    let told = given_until(&realm, THREE, before, 3).await;
    let answered = given_until(&realm, MAYOR, mayor_before, 1).await;

    assert_eq!(objected, Done::Done);
    assert_eq!((spending, own), (held(&task), Done::Done));
    assert_eq!(unheld, ["letter: lunch at noon"]);
    assert!(mayor_unheld.is_empty(), "{mayor_unheld:?}");
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].1, "objection");
    assert!(
        waits[0].2.contains("better left alone"),
        "{}",
        waits[0].2
    );
    assert_eq!(ruled, Done::Done);
    assert_eq!(
        told,
        [
            "letter: lunch at noon",
            "notice: The chief rules: go on with the instruction. \
             The chief says: go on",
            "letter: the file is in /tmp",
        ]
    );
    assert_eq!(answered, ["answer: the second"]);
    assert_eq!(may_spend(&realm, THREE, &task).await, Done::Done);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn withdraw_drops_only_that_tasks_letters() {
    // 3 objects to two chief's lines, and a letter on each is kept.
    // Withdrawing the first tells 3 so and drops its letter; the
    // second's stays kept until its go.
    let (realm, _) = hamlet_with(1).await;
    let first = chief_task(&realm, THREE).await;
    let second = chief_task(&realm, THREE).await;
    for (call, task) in [("o1", &first), ("o2", &second)] {
        let objected = realm
            .tool(THREE, &tool(call, objection(task, "no")))
            .await
            .unwrap();
        assert_eq!(objected, Done::Done);
    }
    realm
        .tool(MAYOR, &tool("t1", on(&first, "for the first")))
        .await
        .unwrap();
    realm
        .tool(MAYOR, &tool("t2", on(&second, "for the second")))
        .await
        .unwrap();
    let before = submits_to(&realm, THREE).len();
    let waits = open_waits(&realm, THREE).await;

    let withdrawn = realm
        .ask(&answer(&waits[0].0.to_string(), Choice::Withdraw, None))
        .await
        .unwrap();
    let after_withdraw = given_until(&realm, THREE, before, 1).await;
    let still_held = may_spend(&realm, THREE, &second).await;
    realm
        .ask(&answer(&waits[1].0.to_string(), Choice::Go, None))
        .await
        .unwrap();
    let told = given_until(&realm, THREE, before, 3).await;

    assert_eq!(withdrawn, Done::Done);
    assert_eq!(
        after_withdraw,
        ["notice: The chief withdrew the instruction. Stop working \
          on it."]
    );
    assert_eq!(still_held, held(&second));
    assert_eq!(
        told[1..],
        [
            "notice: The chief rules: go on with the instruction.",
            "letter: for the second",
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_objection_survives_a_restart() {
    // 3 objects and a letter to it is kept; the realm's host stops
    // and starts again. The wait is still open, spending still held,
    // and one go delivers the notice and the letter; a second go
    // finds nothing waiting.
    let (realm, _) = hamlet_with(1).await;
    let task = chief_task(&realm, THREE).await;
    realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    realm
        .tool(MAYOR, &tool("t1", on(&task, "kept")))
        .await
        .unwrap();
    let before = submits_to(&realm, THREE).len();

    let realm = realm.restart().await;
    let waits = open_waits(&realm, THREE).await;
    let spending = may_spend(&realm, THREE, &task).await;
    let wait = waits[0].0.to_string();
    let first = realm.ask(&answer(&wait, Choice::Go, None)).await;
    let again = realm.ask(&answer(&wait, Choice::Go, None)).await;
    let told = given_until(&realm, THREE, before, 2).await;

    assert_eq!(waits.len(), 1);
    assert_eq!(spending, held(&task));
    assert_eq!(first.unwrap(), Done::Done);
    assert_eq!(
        again.unwrap(),
        Done::Refused {
            why: AnswerRefusal::NothingWaits { wait }
        }
    );
    assert_eq!(
        told,
        [
            "notice: The chief rules: go on with the instruction.",
            "letter: kept",
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_objection_on_a_task_is_refused() {
    // a second objection to one line is refused, after a ruling too;
    // an objection with no reason, or to anything but the caller's
    // own chief's line, is refused and opens no wait.
    let (realm, _) = hamlet_with(2).await;
    let task = chief_task(&realm, THREE).await;
    let fours = chief_task(&realm, FOUR).await;

    let first = realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    let second = realm
        .tool(THREE, &tool("o2", objection(&task, "still no")))
        .await
        .unwrap();
    let wait = open_waits(&realm, THREE).await[0].0.to_string();
    realm.ask(&answer(&wait, Choice::Go, None)).await.unwrap();
    let after_go = realm
        .tool(THREE, &tool("o3", objection(&task, "no!")))
        .await
        .unwrap();
    let unreasoned = realm
        .tool(FOUR, &tool("o4", objection(&fours, "  ")))
        .await
        .unwrap();
    let not_its_own = realm
        .tool(FOUR, &tool("o5", objection(&task, "no")))
        .await
        .unwrap();
    let own = standing(&realm, FOUR);
    let standing_task = realm
        .tool(FOUR, &tool("o6", objection(&own, "no")))
        .await
        .unwrap();

    assert_eq!(first, Done::Done);
    assert_eq!(
        [second, after_go],
        [
            refused(ObjectRefusal::AlreadyHeld),
            refused(ObjectRefusal::AlreadyHeld)
        ]
    );
    assert_eq!(unreasoned, refused(ObjectRefusal::NoReason));
    assert_eq!(
        [not_its_own, standing_task],
        [
            refused(ObjectRefusal::NotAChiefsLine),
            refused(ObjectRefusal::NotAChiefsLine)
        ]
    );
    assert!(open_waits(&realm, FOUR).await.is_empty());
    realm.host.shutdown().await.unwrap();
}

/// The rows of the `held` table.
async fn held_rows(realm: &Realm) -> i64 {
    realm
        .rows(|db| {
            db.query_row("SELECT COUNT(*) FROM held", [], |row| {
                row.get(0)
            })
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawn_task_spends_nothing_and_its_letters_are_dropped()
{
    // 3 objects to a chief's line and the chief withdraws it. No
    // agent may spend on that task any more, in today's words; a
    // letter on it is answered and never given, one on another task
    // is.
    let (realm, _) = hamlet_with(1).await;
    let task = chief_task(&realm, THREE).await;
    realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    let wait = open_waits(&realm, THREE).await[0].0.to_string();
    let before = submits_to(&realm, THREE).len();

    realm
        .ask(&answer(&wait, Choice::Withdraw, None))
        .await
        .unwrap();
    let three = may_spend(&realm, THREE, &task).await;
    let mayor = may_spend(&realm, MAYOR, &task).await;
    let dropped = realm
        .tool(MAYOR, &tool("t1", on(&task, "too late")))
        .await
        .unwrap();
    let mayors = standing(&realm, MAYOR);
    realm
        .tool(MAYOR, &tool("t2", on(&mayors, "lunch at noon")))
        .await
        .unwrap();
    // Reading the rows stops the realm, which first makes every
    // delivery its acts so far have started.
    held_rows(&realm).await;

    let withdrawn = "The chief withdrew the instruction, so this \
                     call did not run.";
    assert_eq!(
        (words(three), words(mayor)),
        (withdrawn.to_owned(), withdrawn.to_owned())
    );
    assert_eq!(dropped, Done::Done);
    assert_eq!(
        given(&realm, THREE, before),
        [
            "notice: The chief withdrew the instruction. Stop \
             working on it.",
            "letter: lunch at noon",
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lapsed_objection_drops_its_held_letters() {
    // 3 objects and a letter to it is held; its turn is stopped, or
    // rewound, and it lapses its conversation's waits. As today, the
    // held letter is dropped and the task is no longer held: 3 may
    // spend on it, a later letter is given, and a go finds nothing
    // waiting.
    for why in [LapseReason::Stopped, LapseReason::Rewound] {
        let (realm, _) = hamlet_with(1).await;
        let task = chief_task(&realm, THREE).await;
        realm
            .tool(THREE, &tool("o1", objection(&task, "no")))
            .await
            .unwrap();
        realm
            .tool(MAYOR, &tool("t1", on(&task, "held")))
            .await
            .unwrap();
        let wait = open_waits(&realm, THREE).await[0].0.to_string();
        let before = submits_to(&realm, THREE).len();

        let lapse = Lapse {
            conversation: Some(conversation()),
            why,
        };
        realm.tool(THREE, &lapse).await.unwrap();
        let spending = may_spend(&realm, THREE, &task).await;
        realm
            .tool(MAYOR, &tool("t2", on(&task, "after")))
            .await
            .unwrap();
        let go = realm.ask(&answer(&wait, Choice::Go, None)).await;
        let held = held_rows(&realm).await;

        assert_eq!(spending, Done::Done, "{why:?}");
        assert_eq!(
            go.unwrap(),
            Done::Refused {
                why: AnswerRefusal::NothingWaits { wait }
            }
        );
        assert_eq!(held, 0, "{why:?}");
        assert_eq!(given(&realm, THREE, before), ["letter: after"]);
        realm.host.shutdown().await.unwrap();
    }
}
