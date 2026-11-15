use std::time::Duration;

use hamlet_actors::realm;
use hamlet_messages::agent::{Author, Input, Submit};
use hamlet_messages::realm::AgentRefusal::{Act, Standing as Stood};
use hamlet_messages::realm::client::SayAnswer;
use hamlet_messages::realm::{
    Ask, Asked, Done, End, FoundAnswer, HowEnded, Left, LetterId,
    LetterRefusal, Named, Reply, ReplyRefusal, Standing, Tell,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, ConversationId};
use hamlet_protocol::client::RequestId;

use super::{
    Callee, MAYOR, Realm, Seen, error_code, found, founded,
    hamlet_with, hires, purpose_of, say, standing, submits_to, tool,
    until,
};

const THREE: AgentNumber = AgentNumber(3);

fn to(agent: AgentNumber) -> Named {
    Named::Agent { agent }
}

fn tell(to: Named, text: &str) -> Tell {
    Tell {
        to,
        text: text.to_owned(),
        task: None,
    }
}

fn from(agent: AgentNumber) -> Author {
    Author { agent, name: None }
}

async fn seen_submits(
    realm: &Realm,
    agent: AgentNumber,
    count: usize,
) -> Vec<Submit> {
    until(&format!("agent {} saw {count} inputs", agent.0), || {
        submits_to(realm, agent).len() >= count
    })
    .await;
    submits_to(realm, agent)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_tell_keeps_one_letter() {
    // Tool<Tell> twice with one call id: both Done, one letter row
    // naming its way, and two Submits to the receiver with one
    // InputId and the sender's task. The same id with another text
    // is a reused id.
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, MAYOR);
    let sent = tool(
        "t1",
        Tell {
            task: Some(task.clone()),
            ..tell(to(THREE), "hi")
        },
    );
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
        .rows(|db| {
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

    assert_eq!([first, again], [Done::Done, Done::Done]);
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
            text: "hi".to_owned()
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tell_with_no_road_is_refused() {
    // two hires of one lead tell each other: `NoRoad`.
    let (realm, _) = hamlet_with(2).await;
    let four = AgentNumber(4);

    let three_to_four =
        realm.tool(THREE, &tool("t1", tell(to(four), "hi"))).await;
    let four_to_three =
        realm.tool(four, &tool("t1", tell(to(THREE), "hi"))).await;

    let no_road = |to| Done::Refused {
        why: Act(LetterRefusal::NoRoad { to }),
    };
    assert_eq!(three_to_four.unwrap(), no_road(four));
    assert_eq!(four_to_three.unwrap(), no_road(THREE));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tell_with_no_road_to_a_hire_still_hiring_is_refused_at_once()
 {
    // the mayor hires 4 with its fill held: 3, the mayor's other
    // hire, has no road to it, and hears `NoRoad` with no wait.
    let (realm, _) = hamlet_with(1).await;
    let four = AgentNumber(4);
    realm.seen.hold(Callee::Fill);
    hires(&realm, MAYOR, "h1").await;

    let told =
        realm.tool(THREE, &tool("t1", tell(to(four), "hi"))).await;
    realm.seen.release(Callee::Fill);

    assert_eq!(
        told.unwrap(),
        Done::Refused {
            why: Act(LetterRefusal::NoRoad { to: four })
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_letter_or_a_question_as_the_first_message_gives_the_purpose()
 {
    // mayors 2 and 3 are founded with no purpose. The Hand tells 2
    // and 2 asks 3: the first line of each becomes its receiver's
    // purpose, and the chief's line to 2 after it changes nothing.
    let (realm, _) = hamlet_with(0).await;
    let barn = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Barn"),
        })
        .await
        .unwrap();
    realm.live(THREE).await;
    let ask = Ask {
        to: to(THREE),
        text: "Mind the barn?\nsoon".to_owned(),
        conversation: ConversationId("c1".to_owned()),
        task: None,
    };

    let told = realm
        .tool(
            Hand::NUMBER,
            &tool(
                "t1",
                tell(to(MAYOR), "\n Dig the well \nthen rest"),
            ),
        )
        .await
        .unwrap();
    let asked = realm.tool(MAYOR, &tool("a1", ask)).await.unwrap();
    let said = realm
        .ask(&say(&RequestId::new(), MAYOR, "Paint the mill"))
        .await
        .unwrap();

    assert!(matches!(
        barn,
        FoundAnswer::Founded { mayor: THREE, .. }
    ));
    assert_eq!((told, asked), (Done::Done, Done::Done));
    assert!(matches!(said, SayAnswer::Taken { .. }), "{said:?}");
    assert_eq!(
        (
            purpose_of(&realm, MAYOR).await,
            purpose_of(&realm, THREE).await
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
    let four = AgentNumber(4);
    hires(&realm, THREE, "h3").await;
    realm.live(four).await;
    let told = async |by, call, act| {
        realm.tool(by, &tool(call, act)).await.unwrap()
    };

    let to_self = told(THREE, "t1", tell(to(THREE), "hi")).await;
    let empty = told(THREE, "t2", tell(to(four), " \n")).await;
    let to_hand = told(four, "t1", tell(Named::Hand, "hi")).await;
    let to_mayor = told(four, "t2", tell(Named::Mayor, "hi")).await;
    let end = End {
        hire: to(four),
        why: Some("done".to_owned()),
    };
    let ended = realm.tool(THREE, &tool("e1", end)).await.unwrap();
    let to_ended = told(THREE, "t3", tell(to(four), "hi")).await;
    let from_ended = told(four, "t3", tell(to(THREE), "hi")).await;

    let refused = |why| Done::Refused { why: Act(why) };
    assert_eq!(to_self, refused(LetterRefusal::Yourself));
    assert_eq!(empty, refused(LetterRefusal::Empty));
    assert_eq!(
        [to_hand, to_mayor],
        [
            refused(LetterRefusal::NoRoad { to: Hand::NUMBER }),
            refused(LetterRefusal::NoRoad { to: MAYOR })
        ]
    );
    assert_eq!(ended, Done::Done);
    let left = Left {
        how: HowEnded::ByALead { lead: THREE },
        why: Some("done".to_owned()),
    };
    assert_eq!(
        to_ended,
        refused(LetterRefusal::Left {
            to: four,
            left: left.clone()
        })
    );
    assert_eq!(
        from_ended,
        Done::Refused {
            why: Stood(Standing::YouLeft {
                how: left.how,
                why: left.why
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
        Done::Refused {
            why: Act(LetterRefusal::StillHiring { to: THREE })
        }
    );
    assert_eq!(sent, Done::Done);
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
        task: Some(task.clone()),
    };
    let asked = realm.tool(MAYOR, &tool("a1", ask)).await.unwrap();
    let question = seen_submits(&realm, THREE, 1).await.remove(0);
    let Input::Question {
        letter,
        from: asker,
        text,
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

    assert_eq!(asked, Done::Done);
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
