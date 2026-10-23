use hamlet_engine_bevy::api::agents::{
    Doing, RosterHamlet, RosterRoad,
};

use super::*;
use crate::society::Agent;
use crate::tuning::WorldTuning;

const HOME: &str = "home";

/// The realm's side of the roster, with nothing met yet.
struct Realm {
    bridge: Bridge,
    land: Land,
    society: Society,
}

/// An agent of the first hamlet by its number, which came at `since`
/// and left at `until`, merged into `into` when it has a host.
fn agent(
    number: u64,
    since: u64,
    until: Option<u64>,
    into: Option<u64>,
) -> RosterAgent {
    RosterAgent {
        name: format!("agent {number}"),
        number,
        hamlet: HOME.into(),
        mayor: number == 0,
        purpose: String::new(),
        state: String::new(),
        busy: false,
        gone: until.is_some(),
        merged_into: into.map(|host| format!("agent {host}")),
        waiting: 0,
        model: String::new(),
        account: String::new(),
        spent: String::new(),
        since,
        until,
        doing: Doing::default(),
        step: 0,
        failures: 0,
        fault: false,
    }
}

/// Agents that came one after another and are all still there.
fn living(numbers: &[u64]) -> Vec<RosterAgent> {
    numbers
        .iter()
        .map(|&n| agent(n, n * 10, None, None))
        .collect()
}

fn road(a: u64, b: u64, level: RoadUse) -> RosterRoad {
    RosterRoad {
        ends: [(HOME.into(), a), (HOME.into(), b)],
        level,
        messages: 0,
        last_from: None,
    }
}

/// The hamlet `id`, founded `founded`th in the realm's record.
fn hamlet(id: &str, founded: u32) -> RosterHamlet {
    RosterHamlet {
        id: id.into(),
        title: id.into(),
        mayor: String::new(),
        living: 0,
        working: 0,
        waiting: 0,
        gone: 0,
        state: String::new(),
        account: String::new(),
        folder: None,
        manual: true,
        mode: String::new(),
        before: Vec::new(),
        spot: None,
        founded,
        budget: Default::default(),
    }
}

impl Realm {
    fn new() -> Self {
        let (tuning, pal) = WorldTuning::read().unwrap();
        Self {
            bridge: Bridge::default(),
            land: Land::new(1, tuning.generator, pal),
            society: Society::default(),
        }
    }

    /// Follows a roster of the first hamlet alone.
    fn tell(
        &mut self,
        agents: Vec<RosterAgent>,
        roads: Vec<RosterRoad>,
    ) {
        let roster = Roster {
            agents,
            hamlets: vec![hamlet(HOME, 0)],
            roads,
            closed: Vec::new(),
        };
        self.bridge.follow(
            &roster,
            &mut self.land,
            &mut self.society,
        );
    }

    fn agent(&self, number: u64) -> &Agent {
        let drawn = self.bridge.housed[&(HOME.to_owned(), number)];
        self.society.agent(drawn).unwrap()
    }
}

#[test]
fn the_realm_shows_who_left_and_who_came_back() {
    let mut realm = Realm::new();
    realm.tell(living(&[0, 1, 2, 3]), Vec::new());
    let plots = [1, 2, 3].map(|n| realm.agent(n).plot);

    // Removed: a ruin.
    let removed = agent(2, 20, Some(50), None);
    let mut now = living(&[0, 1, 3]);
    now.push(removed.clone());
    realm.tell(now, Vec::new());
    assert!(realm.agent(2).archived);

    // Merged: a ruin, and a wing on the house that took it in.
    realm.tell(
        vec![
            agent(0, 0, None, None),
            agent(1, 10, None, None),
            removed,
            agent(3, 30, Some(60), Some(1)),
        ],
        Vec::new(),
    );
    assert!(realm.agent(3).archived);
    assert_eq!(realm.agent(3).into, Some(realm.agent(1).id));
    assert_eq!(realm.agent(1).wings, 1);

    // Split and restored: both stand again where they stood.
    realm.tell(living(&[0, 1, 2, 3]), Vec::new());
    assert_eq!(realm.agent(1).wings, 0);
    for (n, plot) in [1, 2, 3].into_iter().zip(plots) {
        let now = realm.agent(n);
        assert!(!now.archived);
        assert_eq!(now.plot, plot);
    }
}

#[test]
fn a_realm_opened_later_puts_everyone_on_the_same_plot() {
    let mut first = Realm::new();
    first.tell(living(&[0, 1, 2, 3]), Vec::new());
    let mut later = Realm::new();
    later.tell(
        vec![
            agent(0, 0, None, None),
            agent(1, 10, None, None),
            agent(2, 20, Some(50), None),
            agent(3, 30, Some(60), Some(1)),
        ],
        Vec::new(),
    );
    for n in [1, 2, 3] {
        assert_eq!(later.agent(n).plot, first.agent(n).plot);
    }
    assert!(later.agent(2).archived && later.agent(3).archived);
    assert_eq!(later.agent(1).wings, 1);
}

#[test]
fn a_newcomer_takes_the_plot_of_one_who_left_and_keeps_it() {
    // Two is removed before three comes, so three builds where two
    // stood, in a realm that sees it happen and in one that is
    // opened afterward.
    let story = [
        agent(0, 0, None, None),
        agent(1, 10, None, None),
        agent(2, 20, Some(30), None),
        agent(3, 40, None, None),
    ];
    let mut seen = Realm::new();
    seen.tell(living(&[0, 1, 2]), Vec::new());
    let stood = seen.agent(2).plot;
    seen.tell(story[..3].to_vec(), Vec::new());
    seen.tell(story.to_vec(), Vec::new());
    assert_eq!(seen.agent(3).plot, stood);
    assert!(seen.agent(2).cleared);

    let mut later = Realm::new();
    later.tell(story.to_vec(), Vec::new());
    assert_eq!(later.agent(3).plot, stood);
    assert!(later.agent(2).cleared);
    assert_eq!(later.agent(1).plot, seen.agent(1).plot);
}

#[test]
fn roads_follow_the_roster() {
    let mut realm = Realm::new();
    realm.tell(living(&[0, 1, 2]), Vec::new());
    let pair = Pair::new(realm.agent(1).id, realm.agent(2).id);
    let level =
        |realm: &Realm| realm.society.road(pair).map(|r| r.level);
    assert_eq!(level(&realm), None);

    realm.tell(living(&[0, 1, 2]), vec![road(1, 2, RoadUse::Dirt)]);
    assert_eq!(level(&realm), Some(RoadLevel::Dirt));
    realm.tell(living(&[0, 1, 2]), vec![road(1, 2, RoadUse::Paved)]);
    assert_eq!(level(&realm), Some(RoadLevel::Paved));
    realm.tell(living(&[0, 1, 2]), Vec::new());
    assert_eq!(level(&realm), None);
}

#[test]
fn towns_are_founded_in_the_order_of_the_record() {
    let site = |hamlets: Vec<RosterHamlet>, name: &str| {
        let mut realm = Realm::new();
        let roster = Roster {
            hamlets,
            ..Roster::default()
        };
        realm.bridge.follow(
            &roster,
            &mut realm.land,
            &mut realm.society,
        );
        let town = realm.bridge.town(name).unwrap();
        realm.society.town(town).unwrap().site
    };
    // The roster lists hamlets by title, whatever order they came in.
    let listed =
        vec![hamlet("aspen", 2), hamlet(HOME, 0), hamlet("mill", 1)];
    let founded =
        vec![hamlet(HOME, 0), hamlet("mill", 1), hamlet("aspen", 2)];
    for name in [HOME, "mill", "aspen"] {
        assert_eq!(
            site(listed.clone(), name),
            site(founded.clone(), name)
        );
    }
}
