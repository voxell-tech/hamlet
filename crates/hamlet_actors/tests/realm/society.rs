use hamlet_messages::agent::Input;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::choices::SetNickname;
use hamlet_messages::realm::client::ReadState;
use hamlet_messages::realm::hamlets::{
    ReadRecord, Record, RemoveHamlet,
};
use hamlet_messages::realm::world::{RoadSeen, Seen};
use hamlet_messages::realm::{
    AgentName, AgentRefusal, AgreeRefusal, AgreeRoad, Agreed, Done,
    End, FoundAnswer, HamletId, LeadRefusal, LetterRefusal, Limit,
    Named, Rank, RequestRoad, RequestRoadRefusal, RoadDone, Sent,
    SetLimits, TakeUpRoad,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client as protocol;

use super::chiefs::chief;
use super::laid::ROAD_PRICE;
use super::{
    FOUR, MAYOR, Realm, THREE, found, hamlet_with, hires, named_as,
    seen_submits, shown, submits_to, tell, to, tool, until, world,
};

const OTHER_MAYOR: AgentNumber = AgentNumber(4);
const FIVE: AgentNumber = AgentNumber(5);

/// `agent` as the realm names it in [`two_hamlets`].
fn in_two_hamlets(agent: AgentNumber) -> AgentName {
    match agent {
        OTHER_MAYOR => named_as(4, "hire#4 (Mayor)"),
        FIVE => named_as(5, "hire#5 (Hire 1)"),
        other => shown(other),
    }
}

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

fn waiting(
    mine: AgentNumber,
    theirs: AgentNumber,
) -> Agreed<AgentRefusal<AgreeRefusal>> {
    Agreed::Waiting {
        mine: in_two_hamlets(mine),
        theirs: in_two_hamlets(theirs),
    }
}

fn laid(
    mine: AgentNumber,
    theirs: AgentNumber,
) -> Agreed<AgentRefusal<AgreeRefusal>> {
    Agreed::Laid {
        mine: in_two_hamlets(mine),
        theirs: in_two_hamlets(theirs),
    }
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
) -> Sent<AgentRefusal<LetterRefusal>> {
    realm
        .tool(THREE, &tool(call, tell(to(FIVE), "hi")))
        .await
        .unwrap()
}

fn no_road() -> Sent<AgentRefusal<LetterRefusal>> {
    Sent::Refused {
        why: AgentRefusal::Act(LetterRefusal::NoRoad {
            to: in_two_hamlets(FIVE),
        }),
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
        [
            waiting(THREE, FIVE),
            waiting(THREE, FIVE),
            laid(FIVE, THREE)
        ]
    );
    assert_eq!(
        again,
        refused(AgreeRefusal::RoadExists {
            a: in_two_hamlets(THREE),
            b: in_two_hamlets(FIVE)
        })
    );
    for record in &records {
        let Record::Read { lines } = record else {
            panic!("a record: {record:?}");
        };
        assert_eq!(
            lines.last().map(|line| line.text.as_str()),
            Some(
                "- hire#4 (Mayor) agreed a road from hire#5 (Hire \
                 1): trade"
            )
        );
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agreed_road_is_charged_to_each_hamlet() {
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
        assert_eq!(spent, ROAD_PRICE, "{pays:?}");
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn roads_between_off_refuses_an_agreement() {
    let (realm, _, _) = two_hamlets().await;
    roads_between(&realm, false).await;

    let off =
        agreed_by(&realm, MAYOR, "a1", agree(THREE, FIVE)).await;
    roads_between(&realm, true).await;
    let other =
        agreed_by(&realm, OTHER_MAYOR, "a1", agree(FIVE, THREE))
            .await;

    assert_eq!(off, refused(AgreeRefusal::NoRoadsBetween));
    assert_eq!(other, waiting(FIVE, THREE));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_letter_passes_only_after_both_agreed() {
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
    let sent = Sent::Sent {
        to: in_two_hamlets(FIVE),
    };
    assert_eq!([both, on], [sent.clone(), sent]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agreement_is_refused_as_today() {
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
        RoadDone::Refused {
            why: LeadRefusal::Rank(Rank::NotALead {
                hire: in_two_hamlets(FIVE)
            })
        }
    );
    assert_eq!(
        by_hand,
        RoadDone::Done {
            a: in_two_hamlets(THREE),
            b: in_two_hamlets(FIVE),
        }
    );
    assert_eq!(
        [one, both],
        [waiting(THREE, FIVE), laid(FIVE, THREE)]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_road_with_no_reason_writes_todays_words() {
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
            "I ask for a road to hire#4 (Hire 2): ",
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_road_to_an_ended_agent_is_refused() {
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

#[tokio::test(flavor = "multi_thread")]
async fn a_hamlet_shows_no_road_to_a_hire_that_ended() {
    let (realm, _, kiln) = two_hamlets().await;
    agreed_by(&realm, MAYOR, "a1", agree(THREE, FIVE)).await;
    agreed_by(&realm, OTHER_MAYOR, "a1", agree(FIVE, THREE)).await;
    roads_seen(&realm, &[(3, 5)]).await;

    let removed = RemoveHamlet {
        hamlet: kiln,
        why: None,
    };
    assert_eq!(chief(&realm, removed).await, Done::Done);
    roads_seen(&realm, &[]).await;
    let state: protocol::State = realm.ask(&ReadState).await.unwrap();

    let to_five = state
        .hamlets
        .iter()
        .flat_map(|hamlet| &hamlet.roads)
        .filter(|road| road.a.0 == 5 || road.b.0 == 5)
        .count();
    assert_eq!(to_five, 0);
    realm.host.shutdown().await.unwrap();
}
