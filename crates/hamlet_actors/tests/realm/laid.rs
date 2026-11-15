use hamlet_messages::agent::{Author, Input, Place, Submit};
use hamlet_messages::realm::world::{RoadSeen, Seen};
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, End, FoundAnswer, Gate, HamletId,
    Hire, HireAnswer, LayRefusal, LayRoad, LeadRefusal,
    LetterRefusal, Named, Rank, RequestRoad, RequestRoadRefusal,
    SetTools, SocietyTool, TakeUpRoad, Tell, ToolsRefusal,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::{MAYOR, Realm, found, founded, tool, until, world};

const THREE: AgentNumber = AgentNumber(3);
const FOUR: AgentNumber = AgentNumber(4);

fn to(agent: AgentNumber) -> Named {
    Named::Agent { agent }
}

fn lay(a: AgentNumber, b: AgentNumber) -> LayRoad {
    LayRoad { a: to(a), b: to(b) }
}

fn tell(to: AgentNumber) -> Tell {
    Tell {
        to: self::to(to),
        text: "hi".to_owned(),
        task: None,
    }
}

fn tools(off: &[SocietyTool], on: &[SocietyTool]) -> SetTools {
    SetTools {
        hire: to(THREE),
        off: off.to_vec(),
        on: on.to_vec(),
    }
}

/// A realm with a hamlet whose mayor has `count` live hires, from
/// agent 3 up.
async fn hamlet_with(count: u32) -> (Realm, HamletId) {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    realm.live(MAYOR).await;
    for n in 0..count {
        let hire = Hire {
            hamlet: None,
            under: None,
            profile: None,
            purpose: Some("Write the docs".to_owned()),
        };
        let hired =
            realm.tool(MAYOR, &tool(&format!("h{n}"), hire)).await;
        assert!(
            matches!(hired, Ok(HireAnswer::Hired { .. })),
            "{hired:?}"
        );
        realm.live(AgentNumber(3 + n)).await;
    }
    (realm, hamlet)
}

fn submits_to(realm: &Realm, agent: AgentNumber) -> Vec<Submit> {
    realm
        .seen
        .submits
        .lock()
        .unwrap()
        .iter()
        .filter(|(to, _)| *to == agent)
        .map(|(_, submit)| submit.clone())
        .collect()
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

/// Waits until the last place sent to `agent` holds.
async fn placed(
    realm: &Realm,
    agent: AgentNumber,
    holds: impl Fn(&Place) -> bool,
) {
    until(&format!("agent {}'s place", agent.0), || {
        realm
            .seen
            .places
            .lock()
            .unwrap()
            .iter()
            .rfind(|(to, _)| *to == agent)
            .is_some_and(|(_, place)| holds(place))
    })
    .await;
}

async fn roads_seen(realm: &Realm, roads: &[(u32, u32)]) -> Seen {
    let roads = roads
        .iter()
        .map(|&(a, b)| RoadSeen {
            a: AgentNumber(a),
            b: AgentNumber(b),
        })
        .collect::<Vec<_>>();
    world::seen_until(
        realm,
        &format!("the roads {roads:?}"),
        |seen| seen.roads == roads,
    )
    .await
}

/// The charges of laid roads, read with the realm stopped.
async fn road_charges(realm: &Realm) -> i64 {
    realm
        .rows(|db| {
            db.query_row(
                "SELECT COUNT(*) FROM charge WHERE agent = 0",
                [],
                |row| row.get(0),
            )
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_laid_road_carries_letters_and_is_charged_once() {
    // the mayor lays a road between its hires 3 and 4, who have no
    // road by the tree, then lays it again with the same call and
    // with another: all Done. 3 tells 4 twice with two call ids:
    // both delivered. The road's charge is one row, the world shows
    // the road, and both ends' places hold each other.
    let (realm, _) = hamlet_with(2).await;
    let laying = tool("r1", lay(THREE, FOUR));

    let laid = realm.tool(MAYOR, &laying).await.unwrap();
    let repeated = realm.tool(MAYOR, &laying).await.unwrap();
    let again = realm
        .tool(MAYOR, &tool("r2", lay(FOUR, THREE)))
        .await
        .unwrap();
    let first = realm.tool(THREE, &tool("t1", tell(FOUR))).await;
    let second = realm.tool(THREE, &tool("t2", tell(FOUR))).await;
    let letters = seen_submits(&realm, FOUR, 2).await;
    roads_seen(&realm, &[(3, 4)]).await;
    placed(&realm, THREE, |place| place.roads == [FOUR]).await;
    placed(&realm, FOUR, |place| place.roads == [THREE]).await;

    assert_eq!(
        [laid, repeated, again],
        [Done::Done, Done::Done, Done::Done]
    );
    assert_eq!(
        [first.unwrap(), second.unwrap()],
        [Done::Done, Done::Done]
    );
    assert_ne!(letters[0].id, letters[1].id);
    assert_eq!(
        letters[0].input,
        Input::Letter {
            from: Author {
                agent: THREE,
                name: None
            },
            text: "hi".to_owned()
        }
    );
    assert_eq!(road_charges(&realm).await, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_turned_off_is_refused() {
    // the mayor turns `tell` off for its hire 3: 3's tell is refused
    // `ToolOff`, and its place lists the tool. Turned on again, the
    // tell goes through. The Hand's and a mayor's tools are not
    // turned off, by the Hand, a hire or the chief.
    let (realm, _) = hamlet_with(1).await;
    let set = async |by, call, act| {
        realm.tool(by, &tool(call, act)).await.unwrap()
    };

    let off =
        set(MAYOR, "s1", tools(&[SocietyTool::Tell], &[])).await;
    let refused = realm.tool(THREE, &tool("t1", tell(MAYOR))).await;
    placed(&realm, THREE, |place| {
        place.tools_off == [SocietyTool::Tell]
    })
    .await;
    let on = set(MAYOR, "s2", tools(&[], &[SocietyTool::Tell])).await;
    placed(&realm, THREE, |place| place.tools_off.is_empty()).await;
    let told = realm.tool(THREE, &tool("t2", tell(MAYOR))).await;
    let of_hand = SetTools {
        hire: Named::Hand,
        ..tools(&[SocietyTool::Hire], &[])
    };
    let of_mayor = SetTools {
        hire: Named::Mayor,
        ..tools(&[SocietyTool::Hire], &[])
    };
    let hand_by_hand = set(Hand::NUMBER, "s1", of_hand.clone()).await;
    let mayor_by_hire = set(THREE, "s1", of_mayor).await;
    let chief = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: of_hand,
        })
        .await
        .unwrap();

    assert_eq!([off, on], [Done::Done, Done::Done]);
    assert_eq!(
        refused.unwrap(),
        Done::Refused {
            why: AgentRefusal::Gate(Gate::ToolOff {
                tool: SocietyTool::Tell
            })
        }
    );
    assert_eq!(told.unwrap(), Done::Done);
    let not_a_hire = Done::Refused {
        why: LeadRefusal::Act(ToolsRefusal::NotAHire),
    };
    assert_eq!(
        [hand_by_hand, mayor_by_hire],
        [not_a_hire.clone(), not_a_hire]
    );
    assert_eq!(
        chief,
        Done::Refused {
            why: ToolsRefusal::NotAHire
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_is_asked_for_by_a_letter_to_the_lead() {
    // 3 asks for a road to 4: its lead, the mayor, gets a letter from
    // 3 naming 4 and why. The mayor's request goes to the Hand. The
    // Hand has no lead, and no road is asked to no agent.
    let (realm, _) = hamlet_with(2).await;
    let request = |to: Named| RequestRoad {
        to,
        why: "to share the notes".to_owned(),
        task: None,
    };
    let ask = async |by, call, to| {
        realm.tool(by, &tool(call, request(to))).await.unwrap()
    };

    let by_hire = ask(THREE, "q1", to(FOUR)).await;
    let by_mayor = ask(MAYOR, "q1", to(FOUR)).await;
    let by_hand = ask(Hand::NUMBER, "q1", to(FOUR)).await;
    let to_none = ask(THREE, "q2", to(AgentNumber(99))).await;
    let to_mayor = seen_submits(&realm, MAYOR, 1).await;
    let to_hand = seen_submits(&realm, Hand::NUMBER, 1).await;

    assert_eq!([by_hire, by_mayor], [Done::Done, Done::Done]);
    let refused = |why| Done::Refused {
        why: AgentRefusal::Act(why),
    };
    assert_eq!(
        [by_hand, to_none],
        [
            refused(RequestRoadRefusal::NoLead),
            refused(RequestRoadRefusal::NoSuchAgent)
        ]
    );
    for (letters, from) in [(to_mayor, THREE), (to_hand, MAYOR)] {
        let Input::Letter { from: author, text } = &letters[0].input
        else {
            panic!("a letter: {letters:?}");
        };
        assert_eq!(author.agent, from);
        assert!(text.contains("`hire#4`"), "{text}");
        assert!(text.contains("to share the notes"), "{text}");
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_taken_up_carries_no_letter_and_is_charged_again() {
    // lay 3-4 and take it up: a tell across it is `NoRoad`, and
    // taking it up again changes nothing. Laid again, it is charged
    // again: two charges.
    let (realm, _) = hamlet_with(2).await;
    let take_up = TakeUpRoad {
        a: to(FOUR),
        b: to(THREE),
    };

    realm
        .tool(MAYOR, &tool("r1", lay(THREE, FOUR)))
        .await
        .unwrap();
    let taken = realm
        .tool(MAYOR, &tool("u1", take_up.clone()))
        .await
        .unwrap();
    roads_seen(&realm, &[]).await;
    let told = realm.tool(THREE, &tool("t1", tell(FOUR))).await;
    let again =
        realm.tool(MAYOR, &tool("u2", take_up)).await.unwrap();
    let relaid = realm
        .tool(MAYOR, &tool("r2", lay(THREE, FOUR)))
        .await
        .unwrap();
    roads_seen(&realm, &[(3, 4)]).await;

    assert_eq!([taken, again], [Done::Done, Done::Done]);
    assert_eq!(relaid, Done::Done);
    assert_eq!(
        told.unwrap(),
        Done::Refused {
            why: AgentRefusal::Act(LetterRefusal::NoRoad {
                to: FOUR
            })
        }
    );
    assert_eq!(road_charges(&realm).await, 2);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_joins_two_hires_of_one_hamlet_under_its_layer() {
    // the chief lays no road to the Hand, across hamlets, from a hire
    // to itself, to an ended hire or to no agent; a hire lays none it
    // does not lead both ends of. A road between a hire and its own
    // lead is there by the tree, so none is laid and nothing is
    // charged.
    let (realm, _) = hamlet_with(2).await;
    let other = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Kiln"),
        })
        .await
        .unwrap();
    let chief = async |act| {
        realm
            .ask(&Asked {
                id: RequestId::new(),
                act,
            })
            .await
            .unwrap()
    };

    let to_hand = chief(LayRoad {
        a: Named::Hand,
        b: to(THREE),
    })
    .await;
    let across = chief(lay(THREE, AgentNumber(5))).await;
    let itself = chief(lay(THREE, THREE)).await;
    let to_none = chief(lay(THREE, AgentNumber(99))).await;
    let to_own_lead = chief(lay(MAYOR, THREE)).await;
    let by_hire =
        realm.tool(THREE, &tool("r1", lay(THREE, FOUR))).await;
    let ended = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: End {
                hire: to(FOUR),
                why: None,
            },
        })
        .await
        .unwrap();
    let to_ended = chief(lay(THREE, FOUR)).await;
    let seen = world::seen_until(&realm, "the world", |_| true).await;

    assert!(
        matches!(other, FoundAnswer::Founded { .. }),
        "{other:?}"
    );
    let refused = |why| Done::Refused { why };
    assert_eq!(
        [to_hand, across, itself, to_ended, to_none, to_own_lead],
        [
            refused(LayRefusal::NotTwoHiresOfOneHamlet),
            refused(LayRefusal::NotTwoHiresOfOneHamlet),
            refused(LayRefusal::NotTwoHiresOfOneHamlet),
            refused(LayRefusal::NotTwoHiresOfOneHamlet),
            refused(LayRefusal::NoSuchAgent),
            Done::Done
        ]
    );
    assert_eq!(
        by_hire.unwrap(),
        Done::Refused {
            why: LeadRefusal::Rank(Rank::NotALead { hire: THREE })
        }
    );
    assert_eq!(ended, Done::Done);
    assert_eq!(seen.roads, []);
    assert_eq!(road_charges(&realm).await, 0);
    realm.host.shutdown().await.unwrap();
}
