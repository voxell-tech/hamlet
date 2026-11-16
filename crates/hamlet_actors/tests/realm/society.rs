use hamlet_messages::agent::Input;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::choices::SetNickname;
use hamlet_messages::realm::hamlets::{ReadRecord, Record};
use hamlet_messages::realm::world::{RoadSeen, Seen};
use hamlet_messages::realm::{
    AgentRefusal, AgreeRefusal, AgreeRoad, Agreed, Done, End,
    FoundAnswer, HamletId, LeadRefusal, LetterRefusal, Limit, Named,
    Rank, RequestRoad, RequestRoadRefusal, SetLimits, TakeUpRoad,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;

use super::chiefs::chief;
use super::{
    FOUR, MAYOR, Realm, THREE, found, hamlet_with, hires,
    seen_submits, submits_to, tell, to, tool, until, world,
};

const OTHER_MAYOR: AgentNumber = AgentNumber(4);
const FIVE: AgentNumber = AgentNumber(5);

/// Two hamlets: the mayor 2 with its hire 3, and the mayor 4 with
/// its hire 5.
async fn two_hamlets() -> (Realm, HamletId, HamletId) {
    let (realm, mill) = hamlet_with(1).await;
    let FoundAnswer::Founded {
        hamlet: kiln,
        mayor: OTHER_MAYOR,
    } = chief(&realm, found("Kiln")).await
    else {
        panic!("the second hamlet's mayor is 4");
    };
    realm.live(OTHER_MAYOR).await;
    hires(&realm, OTHER_MAYOR, "k0").await;
    realm.live(FIVE).await;
    (realm, mill, kiln)
}

fn agree(mine: AgentNumber, theirs: AgentNumber) -> AgreeRoad {
    AgreeRoad {
        mine: to(mine),
        theirs: to(theirs),
        why: Some("share files".to_owned()),
    }
}

async fn agreed_by(
    realm: &Realm,
    mayor: AgentNumber,
    call: &str,
    act: AgreeRoad,
) -> Agreed<AgentRefusal<AgreeRefusal>> {
    realm.tool(mayor, &tool(call, act)).await.unwrap()
}

fn refused(why: AgreeRefusal) -> Agreed<AgentRefusal<AgreeRefusal>> {
    Agreed::Refused {
        why: AgentRefusal::Act(why),
    }
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

async fn told(
    realm: &Realm,
    call: &str,
) -> Done<AgentRefusal<LetterRefusal>> {
    realm
        .tool(THREE, &tool(call, tell(to(FIVE), "hi")))
        .await
        .unwrap()
}

fn no_road() -> Done<AgentRefusal<LetterRefusal>> {
    Done::Refused {
        why: AgentRefusal::Act(LetterRefusal::NoRoad { to: FIVE }),
    }
}

async fn roads_between(realm: &Realm, allowed: bool) {
    let set = chief(
        realm,
        SetLimits {
            hamlet: None,
            budget: Vec::new(),
            limits: vec![Limit::RoadsBetween { allowed }],
        },
    )
    .await;
    assert_eq!(set, Done::Done);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_road_is_laid_when_both_mayors_agree() {
    // the mayor 2 agrees a road from its hire 3 to the other
    // hamlet's 5: waiting, and again waiting when it asks again. The
    // mayor 4 agrees from its side: laid. The world shows the road,
    // each hamlet's record keeps it, and a new agreement is refused
    // `RoadExists`.
    let (realm, mill, kiln) = two_hamlets().await;

    let first =
        agreed_by(&realm, MAYOR, "a1", agree(THREE, FIVE)).await;
    let repeated =
        agreed_by(&realm, MAYOR, "a2", agree(THREE, FIVE)).await;
    let theirs = AgreeRoad {
        why: Some("trade".to_owned()),
        ..agree(FIVE, THREE)
    };
    let second = agreed_by(&realm, OTHER_MAYOR, "a1", theirs).await;
    roads_seen(&realm, &[(3, 5)]).await;
    let again =
        agreed_by(&realm, MAYOR, "a3", agree(THREE, FIVE)).await;
    let records = [
        chief(&realm, ReadRecord { hamlet: Some(mill) }).await,
        chief(&realm, ReadRecord { hamlet: Some(kiln) }).await,
    ];

    assert_eq!(
        [first, repeated, second],
        [Agreed::Waiting, Agreed::Waiting, Agreed::Laid]
    );
    assert_eq!(
        again,
        refused(AgreeRefusal::RoadExists { a: THREE, b: FIVE })
    );
    let agreed = [
        "- hire#2 agreed a road from hire#3: share files",
        "- hire#4 agreed a road from hire#5: trade",
    ];
    for (record, line) in records.iter().zip(agreed) {
        let Record::Read { lines } = record else {
            panic!("a record: {record:?}");
        };
        assert!(
            lines.iter().any(|kept| kept.text == line),
            "{line} missing from {lines:?}"
        );
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agreed_road_is_not_charged() {
    // both mayors agree 3-5: neither hamlet spent anything.
    let (realm, mill, kiln) = two_hamlets().await;

    agreed_by(&realm, MAYOR, "a1", agree(THREE, FIVE)).await;
    agreed_by(&realm, OTHER_MAYOR, "a1", agree(FIVE, THREE)).await;
    let seen = roads_seen(&realm, &[(3, 5)]).await;

    for hamlet in [mill, kiln] {
        let pays = Payer::Hamlet { hamlet };
        let spent = seen
            .spend
            .iter()
            .find(|spend| spend.payer == pays)
            .map_or(0, |spend| spend.spent_micros);
        assert_eq!(spent, 0, "{pays:?}");
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn roads_between_off_refuses_an_agreement() {
    // the chief allows no roads between hamlets: an agreement is
    // refused `NoRoadsBetween` and none is kept, so once allowed
    // again the other mayor's agreement alone lays no road.
    let (realm, _, _) = two_hamlets().await;
    roads_between(&realm, false).await;

    let off =
        agreed_by(&realm, MAYOR, "a1", agree(THREE, FIVE)).await;
    roads_between(&realm, true).await;
    let other =
        agreed_by(&realm, OTHER_MAYOR, "a1", agree(FIVE, THREE))
            .await;

    assert_eq!(off, refused(AgreeRefusal::NoRoadsBetween));
    assert_eq!(other, Agreed::Waiting);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_letter_passes_only_after_both_agreed() {
    // 3 tells 5 in the other hamlet: no road, nor after one mayor
    // agreed; once both agreed, the letter passes. While the chief
    // allows no roads between hamlets the agreed road carries none,
    // and carries one again once allowed.
    let (realm, _, _) = two_hamlets().await;

    let before = told(&realm, "t1").await;
    agreed_by(&realm, MAYOR, "a1", agree(THREE, FIVE)).await;
    let half = told(&realm, "t2").await;
    agreed_by(&realm, OTHER_MAYOR, "a1", agree(FIVE, THREE)).await;
    let both = told(&realm, "t3").await;
    seen_submits(&realm, FIVE, 1).await;
    roads_between(&realm, false).await;
    let off = told(&realm, "t4").await;
    roads_between(&realm, true).await;
    let on = told(&realm, "t5").await;
    seen_submits(&realm, FIVE, 2).await;

    assert_eq!(
        [before, half, off],
        [no_road(), no_road(), no_road()]
    );
    assert_eq!([both, on], [Done::Done, Done::Done]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agreement_is_refused_as_today() {
    // a hire that is no mayor, two ends of one hamlet, the Hand as an
    // end, the mayor 2 between hamlets 4 and 7, an end that left and
    // an end that is no agent: each refused in today's words.
    let (realm, _, _) = two_hamlets().await;
    hires(&realm, MAYOR, "h1").await;
    realm.live(AgentNumber(6)).await;
    let third = chief(&realm, found("Weir")).await;
    let ended = chief(
        &realm,
        End {
            hire: to(AgentNumber(6)),
            why: None,
        },
    )
    .await;

    let by_hire =
        agreed_by(&realm, THREE, "a1", agree(THREE, FIVE)).await;
    let one_hamlet =
        agreed_by(&realm, MAYOR, "a1", agree(THREE, MAYOR)).await;
    let to_hand = agreed_by(
        &realm,
        MAYOR,
        "a2",
        AgreeRoad {
            theirs: Named::Hand,
            ..agree(THREE, FIVE)
        },
    )
    .await;
    let neither =
        agreed_by(&realm, MAYOR, "a3", agree(FIVE, AgentNumber(7)))
            .await;
    let to_ended =
        agreed_by(&realm, MAYOR, "a4", agree(AgentNumber(6), FIVE))
            .await;
    let to_none =
        agreed_by(&realm, MAYOR, "a5", agree(THREE, AgentNumber(99)))
            .await;

    assert!(
        matches!(
            third,
            FoundAnswer::Founded {
                mayor: AgentNumber(7),
                ..
            }
        ),
        "{third:?}"
    );
    assert_eq!(ended, Done::Done);
    assert_eq!(
        [by_hire, one_hamlet, to_hand, neither, to_ended, to_none],
        [
            refused(AgreeRefusal::NotAMayor),
            refused(AgreeRefusal::SameHamlet),
            refused(AgreeRefusal::NotHires),
            refused(AgreeRefusal::NotAMayor),
            refused(AgreeRefusal::NoSuchAgent {
                address: "hire#6".to_owned()
            }),
            refused(AgreeRefusal::NoSuchAgent {
                address: "hire#99".to_owned()
            }),
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agreed_road_is_taken_up_by_the_hand_and_agreed_anew() {
    // a mayor leads no hire of the other hamlet, so it cannot take up
    // the agreed road 3-5; the Hand can. The earlier agreements are
    // spent: one mayor agreeing again is waiting, both again lay it.
    let (realm, _, _) = two_hamlets().await;
    agreed_by(&realm, MAYOR, "a1", agree(THREE, FIVE)).await;
    agreed_by(&realm, OTHER_MAYOR, "a1", agree(FIVE, THREE)).await;
    roads_seen(&realm, &[(3, 5)]).await;
    let take_up = TakeUpRoad {
        a: to(THREE),
        b: to(FIVE),
        why: None,
    };

    let by_mayor = realm
        .tool(MAYOR, &tool("u1", take_up.clone()))
        .await
        .unwrap();
    let by_hand = realm
        .tool(Hand::NUMBER, &tool("u1", take_up))
        .await
        .unwrap();
    roads_seen(&realm, &[]).await;
    let one =
        agreed_by(&realm, MAYOR, "a2", agree(THREE, FIVE)).await;
    let both =
        agreed_by(&realm, OTHER_MAYOR, "a2", agree(FIVE, THREE))
            .await;
    roads_seen(&realm, &[(3, 5)]).await;

    assert_eq!(
        by_mayor,
        Done::Refused {
            why: LeadRefusal::Rank(Rank::NotALead { hire: FIVE })
        }
    );
    assert_eq!(by_hand, Done::Done);
    assert_eq!([one, both], [Agreed::Waiting, Agreed::Laid]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_road_with_no_reason_writes_todays_words() {
    // 3 asks for a road to 4 with no reason, and 4 for one to 3 with
    // a reason padded by spaces: the mayor reads today's words, the
    // reason trimmed.
    let (realm, _) = hamlet_with(2).await;
    let nickname = SetNickname {
        hire: to(THREE),
        name: Some("Spade".to_owned()),
        why: None,
    };
    assert_eq!(chief(&realm, nickname).await, Done::Done);
    let request = |to: AgentNumber, why: Option<&str>| RequestRoad {
        to: self::to(to),
        why: why.map(str::to_owned),
        task: None,
    };

    let bare = realm
        .tool(THREE, &tool("q1", request(FOUR, None)))
        .await
        .unwrap();
    let padded = realm
        .tool(FOUR, &tool("q1", request(THREE, Some("  notes  "))))
        .await
        .unwrap();
    let letters = seen_submits(&realm, MAYOR, 2).await;

    assert_eq!([bare, padded], [Done::Done, Done::Done]);
    let mut texts = letters
        .iter()
        .map(|letter| match &letter.input {
            Input::Letter { text, .. } => text.as_str(),
            other => panic!("a letter: {other:?}"),
        })
        .collect::<Vec<_>>();
    texts.sort_unstable();
    assert_eq!(
        texts,
        [
            "I ask for a road to hire#3 (Spade): notes",
            "I ask for a road to hire#4: ",
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_road_to_an_ended_agent_is_refused() {
    // the chief ends 4; 3's request for a road to it is refused as
    // to no agent, and the mayor is given nothing.
    let (realm, _) = hamlet_with(2).await;
    let ended = chief(
        &realm,
        End {
            hire: to(FOUR),
            why: None,
        },
    )
    .await;

    let asked = realm
        .tool(
            THREE,
            &tool(
                "q1",
                RequestRoad {
                    to: to(FOUR),
                    why: Some("notes".to_owned()),
                    task: None,
                },
            ),
        )
        .await
        .unwrap();

    assert_eq!(ended, Done::Done);
    assert_eq!(
        asked,
        Done::Refused {
            why: AgentRefusal::Act(RequestRoadRefusal::NoSuchAgent {
                address: "hire#4".to_owned()
            })
        }
    );
    assert!(submits_to(&realm, MAYOR).is_empty());
    realm.host.shutdown().await.unwrap();
}

/// Whether the newest place `agent` was sent has a road to `to`.
fn place_has_road(
    realm: &Realm,
    agent: AgentNumber,
    to: AgentNumber,
) -> bool {
    realm
        .seen
        .places
        .lock()
        .unwrap()
        .iter()
        .filter(|(sent, _)| *sent == agent)
        .max_by_key(|(_, place)| place.version)
        .is_some_and(|(_, place)| place.roads.contains(&to))
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agreed_road_is_hidden_while_roads_between_are_off() {
    // once both mayors agreed 3-5, the world shows the road and each
    // end's place holds it. While the chief allows no roads between
    // hamlets, neither does; allowed again, both do.
    let (realm, _, _) = two_hamlets().await;
    agreed_by(&realm, MAYOR, "a1", agree(THREE, FIVE)).await;
    agreed_by(&realm, OTHER_MAYOR, "a1", agree(FIVE, THREE)).await;
    roads_seen(&realm, &[(3, 5)]).await;
    until("3's place holds the road to 5", || {
        place_has_road(&realm, THREE, FIVE)
    })
    .await;

    roads_between(&realm, false).await;
    roads_seen(&realm, &[]).await;
    until("3's place drops the road to 5", || {
        !place_has_road(&realm, THREE, FIVE)
    })
    .await;
    roads_between(&realm, true).await;
    roads_seen(&realm, &[(3, 5)]).await;
    until("5's place holds the road to 3 again", || {
        place_has_road(&realm, FIVE, THREE)
    })
    .await;
    realm.host.shutdown().await.unwrap();
}
