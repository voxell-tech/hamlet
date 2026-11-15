use hamlet_messages::agent::Place;
use hamlet_messages::realm::{Asked, Done, End, FoundAnswer, Named};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::{
    Callee, MAYOR, Realm, chief_hires, found, hamlet_with, hire,
    hired, tool, until,
};

const THREE: AgentNumber = AgentNumber(3);

/// The places `agent` was created with and then sent, as (version,
/// hires).
fn places_of(
    realm: &Realm,
    agent: AgentNumber,
) -> Vec<(u64, Vec<u32>)> {
    let shown = |place: &Place| {
        let hires = place.hires.iter().map(|hire| hire.0).collect();
        (place.version.0, hires)
    };
    let made = realm.seen.made.lock().unwrap();
    let places = realm.seen.places.lock().unwrap();
    made.iter()
        .filter(|(made, _)| *made == agent)
        .map(|(_, input)| shown(&input.place))
        .chain(
            places
                .iter()
                .filter(|(sent, _)| *sent == agent)
                .map(|(_, place)| shown(place)),
        )
        .collect()
}

async fn places_reach(
    realm: &Realm,
    agent: AgentNumber,
    count: usize,
) {
    until(&format!("agent {} holds {count} places", agent.0), || {
        places_of(realm, agent).len() >= count
    })
    .await;
}

/// Whether `agent` was sent a place with a road to `to`.
fn sent_a_road(
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
        .any(|(sent, place)| {
            *sent == agent && place.roads.contains(&to)
        })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_place_follows_the_tree() {
    // hire under a lead, then end the hire: the lead's stand-in got
    // places with rising versions, the one after `Live` holding the
    // hire, the last without it. `Fill` is held so that the place the
    // hire moved at `Hire` arrives before the one at `Live`.
    let (realm, _) = hamlet_with(0).await;
    realm.seen.hold(Callee::Fill);

    let hired_3 = realm.tool(MAYOR, &tool("h1", hire(None))).await;
    places_reach(&realm, MAYOR, 2).await;
    realm.seen.release(Callee::Fill);
    until("the mayor's place holds hire 3", || {
        places_of(&realm, MAYOR).last().unwrap().1 == [3]
    })
    .await;
    let ended = realm
        .tool(
            MAYOR,
            &tool(
                "e1",
                End {
                    hire: Named::Agent { agent: THREE },
                    why: None,
                },
            ),
        )
        .await;
    places_reach(&realm, MAYOR, 4).await;

    assert_eq!(hired_3.unwrap(), hired(3));
    assert_eq!(ended.unwrap(), Done::Done);
    assert_eq!(
        places_of(&realm, MAYOR),
        [(1, vec![]), (2, vec![]), (3, vec![3]), (4, vec![])]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_place_the_agent_fails_to_take_is_not_sent_again() {
    // the mayor fails the place hire 3's `Hire` moved, with 3's fill
    // held: the realm still answers. The mayor gets nothing more
    // until 3 is live, and then the place of that change, holding 3.
    let (realm, hamlet) = hamlet_with(0).await;
    until("the Hand has its road to the mayor", || {
        sent_a_road(&realm, Hand::NUMBER, MAYOR)
    })
    .await;
    realm.seen.hold(Callee::Fill);
    realm.seen.fail_once.lock().unwrap().insert(Callee::Place);

    let hired_3 = chief_hires(&realm, &hamlet, hire(None)).await;
    until("the mayor fails a place", || {
        realm.seen.fail_once.lock().unwrap().is_empty()
    })
    .await;
    let missed = places_of(&realm, MAYOR);
    realm.seen.release(Callee::Fill);
    realm.live(THREE).await;
    places_reach(&realm, MAYOR, 2).await;

    assert_eq!(hired_3, hired(3));
    assert_eq!(missed, [(1, vec![])]);
    assert_eq!(places_of(&realm, MAYOR), [(1, vec![]), (3, vec![3])]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_place_at_a_step_leaves_the_others_sent() {
    // Pond's mayor 3 goes live with the first place its step pushes,
    // the Hand's, failing: mayor 2 is still sent its road to 3.
    let (realm, _) = hamlet_with(0).await;
    until("the Hand has its road to the mayor", || {
        sent_a_road(&realm, Hand::NUMBER, MAYOR)
    })
    .await;
    let before = [Hand::NUMBER, MAYOR]
        .map(|agent| places_of(&realm, agent).len());
    realm.seen.hold(Callee::Fill);
    let pond = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Pond"),
        })
        .await
        .unwrap();
    places_reach(&realm, Hand::NUMBER, before[0] + 1).await;
    places_reach(&realm, MAYOR, before[1] + 1).await;

    realm.seen.fail_once.lock().unwrap().insert(Callee::Place);
    realm.seen.release(Callee::Fill);
    realm.live(THREE).await;
    until("mayor 2 has its road to 3", || {
        sent_a_road(&realm, MAYOR, THREE)
    })
    .await;

    assert!(matches!(
        pond,
        FoundAnswer::Founded { mayor: THREE, .. }
    ));
    assert!(realm.seen.fail_once.lock().unwrap().is_empty());
    assert!(!sent_a_road(&realm, Hand::NUMBER, THREE));
    realm.host.shutdown().await.unwrap();
}
