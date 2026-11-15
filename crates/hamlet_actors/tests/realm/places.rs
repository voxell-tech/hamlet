use std::path::PathBuf;

use hamlet_actor_core::ShutdownKind;
use hamlet_messages::address::{self, Party as _};
use hamlet_messages::agent::{Place, PlaceVersion};
use hamlet_messages::realm::{
    Asked, Done, End, HamletId, Hire, HireAnswer, HireRefusal, Named,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::{
    Callee, DEADLINE, MAYOR, Realm, error_code, founded, tool, until,
};

const THREE: AgentNumber = AgentNumber(3);

fn hire(hamlet: Option<HamletId>) -> Hire {
    Hire {
        hamlet,
        under: Some(Named::Agent { agent: MAYOR }),
        profile: None,
        purpose: Some("Check the links".to_owned()),
    }
}

fn hired<R>(agent: u32) -> HireAnswer<R> {
    HireAnswer::Hired {
        agent: AgentNumber(agent),
    }
}

async fn chief_hires(
    realm: &Realm,
    hamlet: &HamletId,
) -> HireAnswer<HireRefusal> {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act: hire(Some(hamlet.clone())),
        })
        .await
        .unwrap()
}

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

/// The files of `agent`'s actor, moved aside with the actor stopped,
/// so that a call to it fails as a stand-in's failure would.
struct Aside {
    files: Vec<(PathBuf, PathBuf)>,
}

impl Aside {
    async fn take(realm: &Realm, agent: AgentNumber) -> Self {
        let at = address::Agent(agent).address().unwrap();
        realm.host.stop(&at, ShutdownKind::Sleep).await.unwrap();
        let name = format!("{}.sqlite", at.key());
        let files = std::fs::read_dir(
            realm.scratch.join("actors").join(at.kind()),
        )
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| file.starts_with(&name))
        })
        .map(|path| {
            let mut aside = path.clone().into_os_string();
            aside.push(".aside");
            let aside = PathBuf::from(aside);
            std::fs::rename(&path, &aside).unwrap();
            (path, aside)
        })
        .collect::<Vec<_>>();
        assert!(!files.is_empty(), "agent {} has a file", agent.0);
        Self { files }
    }

    fn put_back(self) {
        for (path, aside) in self.files {
            std::fs::rename(aside, path).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_place_follows_the_tree() {
    // hire under a lead, then end the hire: the lead's stand-in got
    // places with rising versions, the one after `Live` holding the
    // hire, the last without it. `Fill` is held so that the place the
    // hire moved at `Hire` arrives before the one at `Live`.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    realm.live(MAYOR).await;
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
    // the mayor's actor cannot be reached while hire 3 goes live: the
    // realm still answers, and the mayor gets nothing. Reached again,
    // the next change (hire 4, held at `Fill`) sends it one place,
    // newer than the two it missed and holding 3.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    realm.live(MAYOR).await;
    let aside = Aside::take(&realm, MAYOR).await;
    let unreached = realm
        .host
        .call(
            &address::Agent(MAYOR).address().unwrap(),
            "place",
            hamlet_actor::action::encode_positional(&Place {
                version: PlaceVersion(0),
                lead: None,
                hires: Vec::new(),
                roads: Vec::new(),
                tools_off: Vec::new(),
            })
            .unwrap(),
            DEADLINE,
        )
        .await;

    let hired_3 = chief_hires(&realm, &hamlet).await;
    realm.live(THREE).await;
    let missed = places_of(&realm, MAYOR);
    aside.put_back();
    realm.seen.hold(Callee::Fill);
    let hired_4 = chief_hires(&realm, &hamlet).await;
    places_reach(&realm, MAYOR, 2).await;
    let sent = places_of(&realm, MAYOR);
    realm.seen.release(Callee::Fill);

    assert_eq!(error_code(unreached), "actor.not_found");
    assert_eq!(hired_3, hired(3));
    assert_eq!(missed, [(1, vec![])]);
    assert_eq!(hired_4, hired(4));
    assert_eq!(sent, [(1, vec![]), (4, vec![3])]);
    realm.host.shutdown().await.unwrap();
}
