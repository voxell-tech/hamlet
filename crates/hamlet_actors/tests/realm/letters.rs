use std::time::Duration;

use hamlet_actors::realm;
use hamlet_messages::agent::{Author, Input};
use hamlet_messages::realm::AgentRefusal::{Act, Standing as Stood};
use hamlet_messages::realm::client::SayAnswer;
use hamlet_messages::realm::letters::{
    Ask, LetterRefusal, Reply, ReplyRefusal, Sent, Tell, Waiting,
};
use hamlet_messages::realm::{
    Done, End, Hire, HireAnswer, HowEnded, LetterId, Named, Standing,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, ConversationId};
use hamlet_protocol::client::RequestId;

use super::stand_ins::{Callee, Seen};
use super::{
    FOUR, MAYOR, Realm, THREE, chief_hires, done_to, error_code,
    founded, hamlet_with, hire, hires, paid, purpose_of, say,
    seen_submits, sent, shown, standing, submits_to, tell, to, tool,
    until, words,
};

fn from(agent: AgentNumber) -> Author {
    Author {
        agent,
        name: Some(shown(agent).name),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_letter_from_the_hand_names_the_hand() {
    let (realm, _) = hamlet_with(0).await;

    realm
        .tool(Hand::NUMBER, &tool("t1", tell(to(MAYOR), "hello")))
        .await
        .unwrap();
    let given = seen_submits(&realm, MAYOR, 1).await;

    assert_eq!(
        given[0].input,
        Input::Letter {
            from: from(Hand::NUMBER),
            text: "hello".to_owned(),
            files: Vec::new(),
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_tell_keeps_one_letter() {
    // Tool<Tell> twice with one call id: both Done, one letter row
    // naming its way, and two Submits to the receiver with one
    // InputId and the sender's task. The same id with another text
    // is a reused id.
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, MAYOR);
    let sent = paid("t1", &task, tell(to(THREE), "hi"));
    let changed = tool(
        "t1",
        Tell {
            text: "bye".to_owned(),
            ..sent.act.clone()
        },
    );

    let first = realm.tool(MAYOR, &sent).await.unwrap();
    let again = realm.tool(MAYOR, &sent).await.unwrap();
    let reused = realm.tool(MAYOR, &changed).await;
    let submits = seen_submits(&realm, THREE, 2).await;
    let letters = realm
        .stopped_rows(|db| {
            db.prepare(
                "SELECT kind, sender, receiver, way, text FROM \
                 letter",
            )?
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await;

    assert_eq!(
        [first, again],
        [self::sent(THREE), self::sent(THREE)]
    );
    assert_eq!(error_code(reused), "message.id_reused");
    assert_eq!(
        letters,
        [(
            "letter".to_owned(),
            2,
            3,
            "tree".to_owned(),
            "hi".to_owned()
        )]
    );
    assert_eq!(submits.len(), 2);
    assert_eq!(submits[0], submits[1]);
    assert_eq!(submits[0].task, Some(task));
    assert_eq!(
        submits[0].input,
        Input::Letter {
            from: from(MAYOR),
            text: "hi".to_owned(),
            files: Vec::new(),
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tell_with_no_road_is_refused() {
    // two hires of one lead tell each other: `NoRoad`.
    let (realm, _) = hamlet_with(2).await;

    let three_to_four =
        realm.tool(THREE, &tool("t1", tell(to(FOUR), "hi"))).await;
    let four_to_three =
        realm.tool(FOUR, &tool("t1", tell(to(THREE), "hi"))).await;

    let no_road = |to| Sent::Refused {
        why: Act(LetterRefusal::NoRoad { to: shown(to) }),
    };
    assert_eq!(three_to_four.unwrap(), no_road(FOUR));
    assert_eq!(four_to_three.unwrap(), no_road(THREE));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tell_with_no_road_to_a_hire_still_hiring_is_refused_at_once()
 {
    // the mayor hires 4 with its fill held: 3, the mayor's other
    // hire, has no road to it, and hears `NoRoad` with no wait.
    let (realm, _) = hamlet_with(1).await;
    realm.seen.hold(Callee::Fill);
    hires(&realm, MAYOR, "h1").await;

    let told =
        realm.tool(THREE, &tool("t1", tell(to(FOUR), "hi"))).await;
    realm.seen.release(Callee::Fill);

    assert_eq!(
        told.unwrap(),
        Sent::Refused {
            why: Act(LetterRefusal::NoRoad { to: shown(FOUR) })
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_letter_or_a_question_as_the_first_message_gives_the_purpose()
 {
    // the chief hires 3 and 4 with no purpose. The mayor tells 3 and
    // asks 4: the first line of each becomes its
    // receiver's purpose, and the chief's line to 3 after it changes
    // nothing.
    let (realm, hamlet) = hamlet_with(0).await;
    let no_purpose = || Hire {
        purpose: None,
        ..hire(None)
    };
    let hired = [
        chief_hires(&realm, &hamlet, no_purpose()).await,
        chief_hires(&realm, &hamlet, no_purpose()).await,
    ];
    realm.live(THREE).await;
    realm.live(FOUR).await;
    let ask = Ask {
        to: to(FOUR),
        text: "Mind the barn?\nsoon".to_owned(),
        conversation: ConversationId("c1".to_owned()),
        paths: Vec::new(),
    };

    let told = realm
        .tool(
            MAYOR,
            &tool(
                "t1",
                tell(to(THREE), "\n Dig the well \nthen rest"),
            ),
        )
        .await
        .unwrap();
    let asked = realm.tool(MAYOR, &tool("a1", ask)).await.unwrap();
    let said = realm
        .ask(&say(&RequestId::new(), THREE, "Paint the mill"))
        .await
        .unwrap();

    assert!(
        hired
            .iter()
            .all(|hired| matches!(hired, HireAnswer::Hired { .. })),
        "{hired:?}"
    );
    assert_eq!(told, sent(THREE));
    assert!(
        matches!(&asked, Waiting::Waiting { on, .. } if on.agent == FOUR),
        "{asked:?}"
    );
    assert!(matches!(said, SayAnswer::Accepted { .. }), "{said:?}");
    assert_eq!(
        (
            purpose_of(&realm, THREE).await,
            purpose_of(&realm, FOUR).await
        ),
        (
            Some("Dig the well".to_owned()),
            Some("Mind the barn?".to_owned())
        )
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tell_is_refused_for_its_ends() {
    // hire 3 hires 4. A tell to oneself, an empty one, and one from 4
    // to the Hand or the mayor are refused; after 3 ends 4, a tell to
    // 4 says how it left, and 4 may not tell.
    let (realm, _) = hamlet_with(1).await;
    hires(&realm, THREE, "h3").await;
    realm.live(FOUR).await;
    let told = async |by, call, act| {
        realm.tool(by, &tool(call, act)).await.unwrap()
    };

    let to_self = told(THREE, "t1", tell(to(THREE), "hi")).await;
    let empty = told(THREE, "t2", tell(to(FOUR), " \n")).await;
    let to_hand = told(FOUR, "t1", tell(Named::Hand, "hi")).await;
    let to_mayor = told(FOUR, "t2", tell(Named::Mayor, "hi")).await;
    let end = End {
        hire: to(FOUR),
        why: Some("done".to_owned()),
    };
    let ended = realm.tool(THREE, &tool("e1", end)).await.unwrap();
    let to_ended = told(THREE, "t3", tell(to(FOUR), "hi")).await;
    let from_ended = told(FOUR, "t3", tell(to(THREE), "hi")).await;

    let refused = |why| Sent::Refused { why: Act(why) };
    assert_eq!(to_self, refused(LetterRefusal::Yourself));
    assert_eq!(empty, refused(LetterRefusal::Empty));
    assert_eq!(
        [to_hand, to_mayor],
        [
            refused(LetterRefusal::NoRoad {
                to: shown(Hand::NUMBER)
            }),
            refused(LetterRefusal::NoRoad { to: shown(MAYOR) })
        ]
    );
    assert_eq!(ended, done_to(FOUR));
    assert_eq!(
        to_ended,
        refused(LetterRefusal::NoSuchAgent {
            address: "hire#4".to_owned()
        })
    );
    assert_eq!(
        from_ended,
        Sent::Refused {
            why: Stood(Standing::YouLeft {
                how: HowEnded::ByALead { lead: shown(THREE) },
                why: Some("done".to_owned())
            })
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tell_to_a_hire_still_hiring_waits_for_its_steps() {
    // the hire's fill is held: a tell waits out the hiring wait and
    // is refused. Once the fill is let go, a tell waits for the
    // steps and is sent.
    let options = realm::Options {
        hiring_wait: Duration::from_secs(1),
        ..realm::Options::default()
    };
    let realm =
        Realm::start_with(Some(options), Seen::default()).await;
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    realm.live(MAYOR).await;
    realm.seen.hold(Callee::Fill);
    hires(&realm, MAYOR, "h1").await;

    let held = realm
        .tool(MAYOR, &tool("t1", tell(to(THREE), "hi")))
        .await
        .unwrap();
    realm.seen.release(Callee::Fill);
    let sent = realm
        .tool(MAYOR, &tool("t2", tell(to(THREE), "hi")))
        .await
        .unwrap();
    seen_submits(&realm, THREE, 1).await;

    assert_eq!(
        held,
        Sent::Refused {
            why: Act(LetterRefusal::StillHiring { to: shown(THREE) })
        }
    );
    assert_eq!(sent, self::sent(THREE));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_reaches_the_asker_once() {
    // the mayor asks 3; 3 replies twice: the mayor's stand-in saw two
    // Answer inputs under one InputId, each with the first reply's
    // text, its conversation and the question's task. A reply from
    // another agent is `NotAsked`, and one to no question
    // `NoSuchQuestion`.
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, MAYOR);
    let conversation = ConversationId("c1".to_owned());
    let ask = Ask {
        to: to(THREE),
        text: "why?".to_owned(),
        conversation: conversation.clone(),
        paths: Vec::new(),
    };
    let asked =
        realm.tool(MAYOR, &paid("a1", &task, ask)).await.unwrap();
    let question = seen_submits(&realm, THREE, 1).await.remove(0);
    let Input::Question {
        letter,
        from: asker,
        text,
        ..
    } = &question.input
    else {
        panic!("agent 3 got a question: {question:?}");
    };
    let reply = |question, text: &str| Reply {
        question,
        text: text.to_owned(),
    };

    let first = realm.tool(THREE, &reply(*letter, "because")).await;
    let second = realm.tool(THREE, &reply(*letter, "no idea")).await;
    let by_asker = realm.tool(MAYOR, &reply(*letter, "me")).await;
    let to_none = realm
        .tool(THREE, &reply(LetterId(letter.0 + 100), "hm"))
        .await;
    let answers = seen_submits(&realm, MAYOR, 2).await;

    assert_eq!(
        asked,
        Waiting::Waiting {
            on: shown(THREE),
            question: *letter,
        }
    );
    assert_eq!((asker, text.as_str()), (&from(MAYOR), "why?"));
    assert_eq!(
        [first.unwrap(), second.unwrap()],
        [Done::Done, Done::Done]
    );
    assert_eq!(
        [by_asker.unwrap(), to_none.unwrap()],
        [
            Done::Refused {
                why: ReplyRefusal::NotAsked
            },
            Done::Refused {
                why: ReplyRefusal::NoSuchQuestion
            }
        ]
    );
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0], answers[1]);
    assert_ne!(answers[0].id, question.id);
    assert_eq!(answers[0].task, Some(task));
    assert_eq!(
        answers[0].input,
        Input::Answer {
            question: *letter,
            from: from(THREE),
            text: "because".to_owned(),
            conversation,
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_letter_to_no_agent_names_its_address() {
    let (realm, _) = hamlet_with(0).await;
    let sent = realm
        .tool(MAYOR, &tool("t1", tell(to(AgentNumber(9)), "Hello.")))
        .await
        .unwrap();

    assert_eq!(words(sent), "no agent has the address `hire#9`");
    realm.host.shutdown().await.unwrap();
}

/// The first letter is held at the hire while the second is told.
/// The test looks for the second letter at the hire for 500 ms, over
/// a hundred times what a delivery that is not held takes here; the
/// order it asserts holds however long that look lasts.
#[tokio::test(flavor = "multi_thread")]
async fn two_letters_to_one_hire_reach_it_in_the_order_told() {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.hold_next(Callee::Submit, THREE);

    let first = realm
        .tool(MAYOR, &tool("t1", tell(to(THREE), "dig the well")))
        .await
        .unwrap();
    until("the first letter waits at the hire", || {
        realm.seen.is_waiting(Callee::Submit, THREE)
    })
    .await;
    let second = realm
        .tool(MAYOR, &tool("t2", tell(to(THREE), "then rest")))
        .await
        .unwrap();
    let _ = tokio::time::timeout(
        Duration::from_millis(500),
        until("the second letter reaches the hire", || {
            !submits_to(&realm, THREE).is_empty()
        }),
    )
    .await;
    realm.seen.release(Callee::Submit);
    let seen = format!("{:?}", seen_submits(&realm, THREE, 2).await);

    assert_eq!((first, second), (sent(THREE), sent(THREE)));
    let (dig, rest) =
        (seen.find("dig the well"), seen.find("then rest"));
    assert!(dig.is_some() && dig < rest, "{seen}");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_letter_names_only_a_task_of_its_sender() {
    let (realm, _) = hamlet_with(2).await;
    let own = standing(&realm, MAYOR);
    let others = standing(&realm, FOUR);

    realm
        .tool(MAYOR, &paid("t1", &others, tell(to(THREE), "one")))
        .await
        .unwrap();
    realm
        .tool(MAYOR, &paid("t2", &own, tell(to(THREE), "two")))
        .await
        .unwrap();
    let submits = seen_submits(&realm, THREE, 2).await;

    let tasks = submits
        .iter()
        .map(|submit| submit.task.clone())
        .collect::<Vec<_>>();
    assert_eq!(tasks, [None, Some(own)]);
    realm.host.shutdown().await.unwrap();
}
