use hamlet_messages::agent::{Author, Input, Place};
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::hamlets::{ReadRecord, Record};
use hamlet_messages::realm::world::{RoadSeen, Seen};
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, DoneTo, End, FoundAnswer, HamletId,
    Hire, HowEnded, LayRefusal, LayRoad, LeadRefusal, Left,
    LetterRefusal, Named, OnlyAHire, Rank, RequestRoad,
    RequestRoadRefusal, RoadDone, Sent, SetTier, SetTools,
    TakeUpRefusal, TakeUpRoad, Tier, TierRefusal, ToolName,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::chiefs::chief;
use super::{
    Callee, FOUR, MAYOR, Realm, THREE, chief_hires, done_to, found,
    hamlet_with, hire, named_as, purpose_of, road_done, seen_submits,
    sent, shown, submits_to, tell, to, tool, until, world,
};

fn request_road(to: AgentNumber) -> RequestRoad {
    RequestRoad {
        to: self::to(to),
        why: Some("to report the well".to_owned()),
    }
}

fn lay(a: AgentNumber, b: AgentNumber) -> LayRoad {
    LayRoad {
        a: to(a),
        b: to(b),
        why: None,
    }
}

fn tools(off: &[&str], on: &[&str]) -> SetTools {
    SetTools {
        hire: to(THREE),
        off: names(off),
        on: names(on),
        why: None,
    }
}

fn names(names: &[&str]) -> Vec<ToolName> {
    names
        .iter()
        .map(|&name| ToolName { name: name.into() })
        .collect()
}

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

const ROAD_PRICE: u64 = 10_000;

async fn spent(realm: &Realm, hamlet: &HamletId, micros: u64) {
    let pays = Payer::Hamlet {
        hamlet: hamlet.clone(),
    };
    world::seen_until(
        realm,
        &format!("the hamlet spent {micros}"),
        |seen| {
            seen.spend
                .iter()
                .find(|spend| spend.payer == pays)
                .map_or(0, |spend| spend.spent_micros)
                == micros
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_laid_road_carries_letters_and_is_charged_once() {
    // the mayor lays a road between its hires 3 and 4, who have no
    // road by the tree, then lays it again with the same call: Done;
    // and with another: `RoadExists`. 3 tells 4 twice with two call
    // ids: both delivered. The hamlet spent one road's price, the
    // world shows the road, and both ends' places hold each other.
    let (realm, hamlet) = hamlet_with(2).await;
    let laying = tool("r1", lay(THREE, FOUR));

    let laid = realm.tool(MAYOR, &laying).await.unwrap();
    let repeated = realm.tool(MAYOR, &laying).await.unwrap();
    let again = realm
        .tool(MAYOR, &tool("r2", lay(FOUR, THREE)))
        .await
        .unwrap();
    let first =
        realm.tool(THREE, &tool("t1", tell(to(FOUR), "hi"))).await;
    let second =
        realm.tool(THREE, &tool("t2", tell(to(FOUR), "hi"))).await;
    let letters = seen_submits(&realm, FOUR, 2).await;
    roads_seen(&realm, &[(3, 4)]).await;
    placed(&realm, THREE, |place| place.roads == [FOUR]).await;
    placed(&realm, FOUR, |place| place.roads == [THREE]).await;

    assert_eq!(
        [laid, repeated],
        [road_done(THREE, FOUR), road_done(THREE, FOUR)]
    );
    assert_eq!(
        again,
        RoadDone::Refused {
            why: LeadRefusal::Act(LayRefusal::RoadExists {
                a: shown(FOUR),
                b: shown(THREE)
            })
        }
    );
    assert_eq!(
        [first.unwrap(), second.unwrap()],
        [sent(FOUR), sent(FOUR)]
    );
    assert_ne!(letters[0].id, letters[1].id);
    assert_eq!(
        letters[0].input,
        Input::Letter {
            from: Author {
                agent: THREE,
                name: Some(shown(THREE).name),
            },
            text: "hi".to_owned(),
            files: Vec::new(),
        }
    );
    spent(&realm, &hamlet, ROAD_PRICE).await;
    let used = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT used_at IS NOT NULL FROM road WHERE a = 3 \
                 AND b = 4",
                [],
                |row| row.get::<_, bool>(0),
            )
        })
        .await;
    assert!(used, "a letter along the road marks it used");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_society_tool_turned_off_is_still_offered() {
    let (realm, _) = hamlet_with(1).await;
    let set = async |by, call, act| {
        realm.tool(by, &tool(call, act)).await.unwrap()
    };

    let off = set(MAYOR, "s1", tools(&["tell"], &[])).await;
    placed(&realm, THREE, |place| {
        place.tools_off == names(&["tell"])
    })
    .await;
    let told =
        realm.tool(THREE, &tool("t1", tell(to(MAYOR), "hi"))).await;
    let on = set(MAYOR, "s2", tools(&[], &["tell"])).await;
    placed(&realm, THREE, |place| place.tools_off.is_empty()).await;

    assert_eq!([off, on], [done_to(THREE), done_to(THREE)]);
    assert_eq!(told.unwrap(), sent(MAYOR));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_off_is_kept_by_its_name() {
    let (realm, _) = hamlet_with(1).await;
    let set = async |call, act| {
        realm.tool(MAYOR, &tool(call, act)).await.unwrap()
    };

    let off = set("s1", tools(&["bash", "tell"], &[])).await;
    placed(&realm, THREE, |place| {
        place.tools_off == names(&["bash", "tell"])
    })
    .await;
    let on = set("s2", tools(&[], &["bash"])).await;
    placed(&realm, THREE, |place| {
        place.tools_off == names(&["tell"])
    })
    .await;

    assert_eq!([off, on], [done_to(THREE), done_to(THREE)]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mayors_tools_are_set_by_the_chief_and_the_hand() {
    let (realm, _) = hamlet_with(1).await;
    let of = |hire, off: &[&str], on: &[&str]| SetTools {
        hire,
        ..tools(off, on)
    };
    let mayor_off = of(to(MAYOR), &["tell"], &[]);
    let hand_off = of(Named::Hand, &["hire"], &[]);
    let by_chief = async |act| {
        realm
            .ask(&Asked {
                id: RequestId::new(),
                act,
            })
            .await
            .unwrap()
    };
    let by = async |agent, call, act| {
        realm.tool(agent, &tool(call, act)).await.unwrap()
    };
    let mayor_tells = async |call| {
        realm
            .tool(MAYOR, &tool(call, tell(to(THREE), "hi")))
            .await
            .unwrap()
    };

    let off = by_chief(mayor_off.clone()).await;
    assert_eq!(off, Done::Done);
    placed(&realm, MAYOR, |place| {
        place.tools_off == names(&["tell"])
    })
    .await;
    let told_while_off = mayor_tells("t1").await;
    let by_hire = by(THREE, "s1", mayor_off).await;
    let on =
        by(Hand::NUMBER, "s1", of(to(MAYOR), &[], &["tell"])).await;
    placed(&realm, MAYOR, |place| place.tools_off.is_empty()).await;
    let told = mayor_tells("t2").await;
    let hand_by_hand = by(Hand::NUMBER, "s2", hand_off.clone()).await;
    let hand_by_chief = by_chief(hand_off).await;

    assert_eq!(on, done_to(MAYOR));
    assert_eq!([told_while_off, told], [sent(THREE), sent(THREE)]);
    assert_eq!(
        by_hire,
        DoneTo::Refused {
            why: LeadRefusal::Rank(Rank::NotALead {
                hire: shown(MAYOR)
            })
        }
    );
    assert_eq!(
        hand_by_hand,
        DoneTo::Refused {
            why: LeadRefusal::Act(OnlyAHire::NotAHire {
                name: "hand".to_owned()
            })
        }
    );
    assert_eq!(
        hand_by_chief,
        Done::Refused {
            why: OnlyAHire::NotAHire {
                name: "hand".to_owned()
            }
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
        why: Some("to share the notes".to_owned()),
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
            refused(RequestRoadRefusal::NoSuchAgent {
                address: "hire#99".to_owned()
            })
        ]
    );
    for (letters, from) in [(to_mayor, THREE), (to_hand, MAYOR)] {
        let Input::Letter {
            from: author, text, ..
        } = &letters[0].input
        else {
            panic!("a letter: {letters:?}");
        };
        assert_eq!(author.agent, from);
        assert!(text.contains(" hire#4 (Hire 2): "), "{text}");
        assert!(text.contains("to share the notes"), "{text}");
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_request_gives_a_lead_with_no_purpose_one() {
    // the chief hires 3 under the mayor with no purpose, and 4 under
    // 3. 4's request for a road to the mayor is the first message 3
    // is given: its first line becomes 3's purpose, as a letter's
    // does.
    let (realm, hamlet) = hamlet_with(0).await;
    chief_hires(
        &realm,
        &hamlet,
        Hire {
            purpose: None,
            ..hire(Some(to(MAYOR)))
        },
    )
    .await;
    realm.live(THREE).await;
    chief_hires(&realm, &hamlet, hire(Some(to(THREE)))).await;
    realm.live(FOUR).await;

    let asked = realm
        .tool(FOUR, &tool("q1", request_road(MAYOR)))
        .await
        .unwrap();
    seen_submits(&realm, THREE, 1).await;

    assert_eq!(asked, Done::Done);
    assert_eq!(
        purpose_of(&realm, THREE).await.as_deref(),
        Some(
            "I ask for a road to hire#2 (Mayor): to report the well"
        )
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_request_to_a_lead_still_hiring_waits_for_its_steps() {
    // the chief hires 3 under the mayor and 4 under 3. 3's create
    // is held, so 3 stays hiring. 4's road request waits out the
    // hiring wait and is refused `StillHiring`, and 3 is given
    // nothing.
    let (realm, hamlet) = hamlet_with(0).await;
    realm.seen.hold_next(Callee::Create, THREE);
    chief_hires(&realm, &hamlet, hire(Some(to(MAYOR)))).await;
    chief_hires(&realm, &hamlet, hire(Some(to(THREE)))).await;
    realm.live(FOUR).await;

    let asked = realm
        .tool(FOUR, &tool("q1", request_road(MAYOR)))
        .await
        .unwrap();

    assert_eq!(
        asked,
        Done::Refused {
            why: AgentRefusal::Act(RequestRoadRefusal::StillHiring {
                lead: shown(THREE)
            })
        }
    );
    assert!(submits_to(&realm, THREE).is_empty());
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_leaves_the_world_when_an_end_is_ended() {
    // lay 3-4, then end 3: the world read after the end shows no
    // road, at a later revision.
    let (realm, _) = hamlet_with(2).await;
    realm
        .tool(MAYOR, &tool("r1", lay(THREE, FOUR)))
        .await
        .unwrap();
    let laid = roads_seen(&realm, &[(3, 4)]).await;

    let ended = realm
        .tool(
            MAYOR,
            &tool(
                "e1",
                End {
                    hire: to(THREE),
                    why: None,
                },
            ),
        )
        .await
        .unwrap();
    let after = realm.ask(&ReadWorld).await.unwrap();

    assert_eq!(ended, done_to(THREE));
    assert_eq!(after.roads, []);
    assert!(after.revision > laid.revision, "{after:?}");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_taken_up_carries_no_letter_and_is_charged_again() {
    // lay 3-4 and take it up: a tell across it is `NoRoad`, and
    // taking it up again is refused `NoRoad`. Laid again, it is
    // charged again: the hamlet spent two roads' price.
    let (realm, hamlet) = hamlet_with(2).await;
    let take_up = TakeUpRoad {
        a: to(FOUR),
        b: to(THREE),
        why: None,
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
    let told =
        realm.tool(THREE, &tool("t1", tell(to(FOUR), "hi"))).await;
    let again =
        realm.tool(MAYOR, &tool("u2", take_up)).await.unwrap();
    let relaid = realm
        .tool(MAYOR, &tool("r2", lay(THREE, FOUR)))
        .await
        .unwrap();
    roads_seen(&realm, &[(3, 4)]).await;

    assert_eq!(taken, road_done(FOUR, THREE));
    assert_eq!(
        again,
        RoadDone::Refused {
            why: LeadRefusal::Act(TakeUpRefusal::NoRoad {
                a: shown(FOUR),
                b: shown(THREE)
            })
        }
    );
    assert_eq!(relaid, road_done(THREE, FOUR));
    assert_eq!(
        told.unwrap(),
        Sent::Refused {
            why: AgentRefusal::Act(LetterRefusal::NoRoad {
                to: shown(FOUR)
            })
        }
    );
    spent(&realm, &hamlet, 2 * ROAD_PRICE).await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_joins_two_hires_of_one_hamlet_under_its_layer() {
    // the chief lays no road to the Hand, across hamlets, from a hire
    // to itself, to an ended hire (`Left`) or to no agent; a hire
    // lays none it does not lead both ends of. A road between a hire
    // and its own lead is there by the tree: `RoadExists`, and
    // nothing is charged.
    let (realm, hamlet) = hamlet_with(2).await;
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
        why: None,
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
            refused(LayRefusal::NotHires),
            refused(LayRefusal::OtherHamlet {
                hire: named_as(5, "hire#5 (Mayor)")
            }),
            refused(LayRefusal::SameEnds),
            refused(LayRefusal::Left {
                hire: shown(FOUR),
                left: Left {
                    how: HowEnded::ByTheChief,
                    why: None
                }
            }),
            refused(LayRefusal::NoSuchAgent {
                address: "hire#99".to_owned()
            }),
            refused(LayRefusal::RoadExists {
                a: shown(MAYOR),
                b: shown(THREE)
            }),
        ]
    );
    assert_eq!(
        by_hire.unwrap(),
        RoadDone::Refused {
            why: LeadRefusal::Rank(Rank::NotALead {
                hire: shown(THREE)
            })
        }
    );
    assert_eq!(ended, Done::Done);
    assert_eq!(seen.roads, []);
    spent(&realm, &hamlet, 0).await;
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hamlet_holds_its_most_laid_roads() {
    // with 24 laid roads in the hamlet, laying 3-4 is `Full`; once
    // the chief takes one up, it is laid.
    let (realm, hamlet) = hamlet_with(2).await;
    realm
        .write(&format!(
            "WITH RECURSIVE n(i) AS (SELECT 100 UNION ALL SELECT i \
             + 1 FROM n WHERE i < 124) INSERT INTO agent (id, kind, \
             status, at) SELECT i, 'hire', 'live', 0 FROM n; WITH \
             RECURSIVE n(i) AS (SELECT 100 UNION ALL SELECT i + 1 \
             FROM n WHERE i < 124) INSERT INTO hire (agent, hamlet, \
             profile, lead) SELECT i, '{0}', 1, 2 FROM n; WITH \
             RECURSIVE n(i) AS (SELECT 101 UNION ALL SELECT i + 1 \
             FROM n WHERE i < 124) INSERT INTO road (a, b, hamlet, \
             at) SELECT 100, i, '{0}', 0 FROM n;",
            hamlet.0
        ))
        .await;

    let full = realm.tool(MAYOR, &tool("r1", lay(THREE, FOUR))).await;
    let taken = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: TakeUpRoad {
                a: to(AgentNumber(100)),
                b: to(AgentNumber(101)),
                why: None,
            },
        })
        .await
        .unwrap();
    let laid = realm.tool(MAYOR, &tool("r2", lay(THREE, FOUR))).await;

    assert_eq!(
        full.unwrap(),
        RoadDone::Refused {
            why: LeadRefusal::Act(LayRefusal::Full {
                hamlet,
                most: 24
            })
        }
    );
    assert_eq!(taken, Done::Done);
    assert_eq!(laid.unwrap(), road_done(THREE, FOUR));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tier_below_the_living_hires_is_refused() {
    // the chief raises the hamlet to a hundred, and the world shows
    // it. With 51 living hires, the mayor among them, lowering it to
    // fifty is `TooSmall`. No hamlet is `NoSuchHamlet`.
    let (realm, hamlet) = hamlet_with(0).await;
    let set = async |hamlet: &HamletId, tier| {
        realm
            .ask(&Asked {
                id: RequestId::new(),
                act: SetTier {
                    hamlet: hamlet.clone(),
                    tier,
                },
            })
            .await
            .unwrap()
    };

    let raised = set(&hamlet, Tier::Hundred).await;
    world::seen_until(&realm, "a hundred", |seen| {
        seen.hamlets.iter().any(|shown| shown.tier == Tier::Hundred)
    })
    .await;
    realm
        .write(&format!(
            "WITH RECURSIVE n(i) AS (SELECT 100 UNION ALL SELECT i \
             + 1 FROM n WHERE i < 149) INSERT INTO agent (id, kind, \
             status, at) SELECT i, 'hire', 'live', 0 FROM n; WITH \
             RECURSIVE n(i) AS (SELECT 100 UNION ALL SELECT i + 1 \
             FROM n WHERE i < 149) INSERT INTO hire (agent, hamlet, \
             profile, lead) SELECT i, '{}', 1, 2 FROM n;",
            hamlet.0
        ))
        .await;
    let lowered = set(&hamlet, Tier::Fifty).await;
    let missing =
        set(&HamletId("none".to_owned()), Tier::Fifty).await;

    assert_eq!(raised, Done::Done);
    assert_eq!(
        lowered,
        Done::Refused {
            why: TierRefusal::TooSmall { living: 51 }
        }
    );
    assert_eq!(
        missing,
        Done::Refused {
            why: TierRefusal::NoSuchHamlet {
                id: HamletId("none".to_owned()),
            }
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_keeps_its_reasons_in_the_record() {
    let (realm, hamlet) = hamlet_with(2).await;
    let laying = LayRoad {
        why: Some("share".to_owned()),
        ..lay(THREE, FOUR)
    };
    let taking_up = TakeUpRoad {
        a: to(THREE),
        b: to(FOUR),
        why: Some("done".to_owned()),
    };

    let laid = realm.tool(MAYOR, &tool("r1", laying)).await.unwrap();
    let taken =
        realm.tool(MAYOR, &tool("u1", taking_up)).await.unwrap();
    let record = chief(
        &realm,
        ReadRecord {
            hamlet: Some(hamlet),
        },
    )
    .await;

    assert_eq!(laid, road_done(THREE, FOUR));
    assert_eq!(taken, road_done(THREE, FOUR));
    let Record::Read { lines } = record else {
        panic!("the hamlet's record: {record:?}");
    };
    let texts = lines.iter().map(|line| line.text.as_str());
    assert_eq!(
        texts.rev().take(2).collect::<Vec<_>>(),
        [
            "- hire#2 (Mayor) took up a road from hire#3 (Hire 1): \
             done",
            "- hire#2 (Mayor) laid a road from hire#3 (Hire 1): \
             share",
        ]
    );
    realm.host.shutdown().await.unwrap();
}
