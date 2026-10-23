use bevy::prelude::*;

use super::*;
use crate::generator::land::Unfit;
use crate::generator::layout::{End, HALL_PLOT, Route};
use crate::generator::testing;

fn land() -> Land {
    testing::land(1)
}

fn small(land: &mut Land) -> Society {
    let mut s = Society::default();
    let t = s.found_town(land);
    for _ in 0..5 {
        s.add_agent(land, t).unwrap();
    }
    s
}

#[test]
fn what_an_agent_is_doing_is_no_part_of_the_shape() {
    let mut land = land();
    let mut s = small(&mut land);
    let shape = s.shape();
    s.set_state(AgentId(1), KeeperState::Working);
    assert_eq!(s.shape(), shape);
    s.set_level(Pair::new(AgentId(0), AgentId(1)), RoadLevel::Paved);
    assert_ne!(s.shape(), shape);
}

#[test]
fn growth_moves_nothing() {
    let mut land = land();
    let mut s = small(&mut land);
    let before = s.agents.clone();
    let t2 = s.found_town(&mut land);
    s.add_agent(&land, TownId(0)).unwrap();
    s.add_agent(&land, t2).unwrap();
    s.add_road(AgentId(1), AgentId(3));
    s.remove_agent(AgentId(2));
    s.add_agent(&land, TownId(0)).unwrap();
    for old in before {
        let now = s.agent(old.id).unwrap();
        assert_eq!((now.town, now.plot), (old.town, old.plot));
    }
    // The newcomer built where the ruin was, and cleared it.
    let ruin = s.agent(AgentId(2)).unwrap();
    assert!(ruin.cleared);
    let on_it =
        s.agents_of(TownId(0)).filter(|a| a.plot == ruin.plot);
    assert_eq!(on_it.count(), 1);
    // Brought back, it stands on a plot of its own.
    let plot = ruin.plot;
    assert_eq!(s.restore(&land, AgentId(2)), Outcome::Changed);
    let back = s.agent(AgentId(2)).unwrap();
    assert!(!back.archived && !back.cleared);
    assert_ne!(back.plot, plot);
    assert_eq!(
        s.agents_of(TownId(0))
            .filter(|a| a.plot == back.plot)
            .count(),
        1
    );
    // Everyone has a road to the mayor of its town.
    for t in &s.towns {
        for a in s.agents_of(t.id).filter(|a| a.id != t.mayor) {
            assert!(s.road(Pair::new(a.id, t.mayor)).is_some());
        }
    }
}

#[test]
fn merging_keeps_the_roads_of_both() {
    let mut land = land();
    let mut s = small(&mut land);
    s.add_road(AgentId(2), AgentId(4));
    s.set_level(Pair::new(AgentId(2), AgentId(4)), RoadLevel::Rail);
    assert_eq!(s.merge(AgentId(1), AgentId(2)), Outcome::Changed);
    assert!(s.agent(AgentId(2)).unwrap().archived);
    assert_eq!(s.agent(AgentId(2)).unwrap().into, Some(AgentId(1)));
    assert_eq!(s.agent(AgentId(1)).unwrap().wings, 1);
    let kept = s.road(Pair::new(AgentId(1), AgentId(4))).unwrap();
    assert_eq!(kept.level, RoadLevel::Rail);
    assert!(s.roads.iter().all(|r| !r.pair.has_end(AgentId(2))));

    assert_eq!(
        s.split(&land, AgentId(1), AgentId(2)),
        Outcome::Changed
    );
    assert!(!s.agent(AgentId(2)).unwrap().archived);
    assert_eq!(s.agent(AgentId(2)).unwrap().into, None);
    assert_eq!(s.agent(AgentId(1)).unwrap().wings, 0);
    assert!(s.road(Pair::new(AgentId(0), AgentId(2))).is_some());
}

#[test]
fn towns_are_founded_where_they_always_were() {
    let mut land = land();
    let first = land.clone();
    let mut s = Society::default();
    let sites = first
        .sites
        .iter()
        .enumerate()
        .filter(|(_, site)| site.buildable)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    for &site in &sites {
        let id = s.found_town(&mut land);
        let town = s.town(id).unwrap();
        assert_eq!(town.site, site);
        assert_eq!(land.sites[site].at, first.sites[site].at);
        assert_eq!(land.sites[site].turn, first.sites[site].turn);
    }
    // The land was not touched: every plot is where it was.
    assert_eq!(land.sites, first.sites);
    for &site in &sites[..5] {
        for k in 0..land.set.town.plots {
            assert_eq!(plot(&land, site, k), plot(&first, site, k));
        }
    }
}

#[test]
fn there_is_always_room_for_one_more_town() {
    let mut land = land();
    let mut s = small(&mut land);
    let apart = land.set.town.keep_apart;
    for n in 1..90 {
        let before = s
            .towns
            .iter()
            .map(|t| (t.id, t.site, land.sites[t.site].clone()))
            .collect::<Vec<_>>();
        let agents = s.agents.clone();
        let id = s.found_town(&mut land);
        assert_eq!(s.towns.len(), n + 1);
        // Nothing that stood moved or turned.
        for (town, site, stood) in before {
            assert_eq!(s.town(town).unwrap().site, site);
            assert_eq!(land.sites[site], stood);
        }
        for old in agents {
            let now = s.agent(old.id).unwrap();
            assert_eq!((now.town, now.plot), (old.town, old.plot));
        }
        let site = s.town(id).unwrap().site;
        let at = land.sites[site].at;
        assert!(!land.is_water(at));
        // A town readied past the first sites keeps its
        // distance and has plots to build on.
        if site >= 37 {
            for other in s.towns.iter().filter(|t| t.id != id) {
                let d = land.sites[other.site].at.distance(at);
                assert!(d >= apart, "{d} apart");
            }
            let usable = (0..land.set.town.plots)
                .filter(|&k| plot(&land, site, k).usable)
                .count();
            assert!(usable > 20, "{usable} plots");
        }
        // It is joined to every other town.
        let mayor = s.town(id).unwrap().mayor;
        for other in s.towns.iter().filter(|t| t.id != id) {
            assert!(s.road(Pair::new(mayor, other.mayor)).is_some());
        }
    }
    assert!(land.sites.len() > 90);
}

/// Open ground far from every town, for a town to be set down
/// on.
fn meadow(land: &Land, s: &Society, town: TownId) -> Vec2 {
    (0..900)
        .map(|i| {
            Vec2::from_angle(i as f32) * (260.0 + i as f32 * 3.0)
        })
        .find(|&p| s.fits(land, Ground::Town(town), p).is_ok())
        .unwrap()
}

#[test]
fn a_moved_town_keeps_everyone_and_everything_and_lands_where_put() {
    let mut land = land();
    let mut s = small(&mut land);
    let far = s.found_town(&mut land);
    s.add_agent(&land, far).unwrap();
    s.add_road(AgentId(1), AgentId(3));
    s.remove_agent(AgentId(4));
    let town = TownId(0);
    let site = s.town(town).unwrap().site;
    let was = land.sites[site].clone();
    let before = s.clone();
    let other = land.sites[s.town(far).unwrap().site].clone();
    let to = meadow(&land, &s, town);

    assert_eq!(s.move_town(&mut land, town, to), Ok(()));
    // Nothing of the society changed: who lives where, on which
    // plot, and which roads there are.
    assert_eq!(s, before);
    assert_eq!(land.sites[site].at, to);
    assert_eq!(land.sites[site].turn, was.turn);
    assert_eq!(land.sites[s.town(far).unwrap().site], other);
    // Every house stands as it stood about the common, on dry
    // and level ground.
    let mut back = land.clone();
    back.move_site(site, was.at);
    for agent in s.agents.iter().filter(|a| a.town == town) {
        let (now, then) = (
            plot(&land, site, agent.plot),
            plot(&back, site, agent.plot),
        );
        assert!((now.at - to).distance(then.at - was.at) < 1e-3);
        assert!((now.yaw - then.yaw).abs() < 1e-4);
        assert!(now.usable);
    }
    // Its roads reach it where it is now.
    let mayor = s.town(town).unwrap().mayor;
    let across = s.town(far).unwrap().mayor;
    let end = |agent: AgentId| {
        let a = s.agent(agent).unwrap();
        End {
            site: s.town(a.town).unwrap().site,
            plot: a.plot,
        }
    };
    let route = Route::between(&land, end(mayor), end(across));
    let door = plot(&land, site, HALL_PLOT).door();
    let ends = [route.line.pts[0], *route.line.pts.last().unwrap()];
    assert!(
        ends.iter().any(|p| p.xz().distance(door) < 0.5),
        "{ends:?} {door}"
    );
    let inside = Route::between(&land, end(mayor), end(AgentId(1)));
    for p in &inside.line.pts {
        assert!(p.xz().distance(to) < land.set.town.radius + 2.0);
    }
    // Set down again where it was, it is as it was.
    assert_eq!(s.move_town(&mut land, town, was.at), Ok(()));
    assert_eq!(land.sites[site], was);
}

#[test]
fn a_town_is_not_set_down_on_water_or_on_another_place() {
    let mut land = land();
    let mut s = small(&mut land);
    let other = s.found_town(&mut land);
    let town = TownId(0);
    let site = s.town(town).unwrap().site;
    let stood = land.sites.clone();
    let refused = |s: &mut Society, land: &mut Land, to, why| {
        assert_eq!(s.move_town(land, town, to), Err(why));
        assert_eq!(land.sites, stood);
    };

    let river = (0..4000)
        .map(|i| Vec2::new(i as f32 * 0.5 - 1000.0, 40.0))
        .find(|&p| land.is_water(p))
        .unwrap();
    refused(&mut s, &mut land, river, Unfit::Water);

    let there = land.sites[s.town(other).unwrap().site].at;
    refused(
        &mut s,
        &mut land,
        there + Vec2::new(20.0, 5.0),
        Unfit::Crowded,
    );
    let peak = (0..4000)
        .map(|i| Vec2::from_angle(i as f32) * (300.0 + i as f32))
        .find(|&p| land.mountain(p) > 0.5)
        .unwrap();
    refused(&mut s, &mut land, peak, Unfit::Steep);

    // Nor on ground something else holds, and nothing is
    // founded there.
    let to = meadow(&land, &s, town);
    s.claims.push(Claim {
        place: Ground::Chief,
        at: to + Vec2::new(10.0, 0.0),
        radius: 12.0,
    });
    refused(&mut s, &mut land, to, Unfit::Crowded);
    assert_eq!(
        s.fits(&land, Ground::Chief, there),
        Err(Unfit::Crowded)
    );
    assert_eq!(land.sites[site], stood[site]);
    let home = stood
        .iter()
        .position(|site| {
            site.buildable
                && !s
                    .towns
                    .iter()
                    .any(|t| land.sites[t.site].at == site.at)
        })
        .unwrap();
    s.claims.push(Claim {
        place: Ground::Chief,
        at: stood[home].at,
        radius: 12.0,
    });
    let next = s.found_town(&mut land);
    assert_ne!(s.town(next).unwrap().site, home);
}

#[test]
fn a_town_is_founded_clear_of_one_that_was_moved() {
    let mut land = land();
    let mut s = small(&mut land);
    let free = (0..land.sites.len())
        .find(|&i| i > 0 && land.sites[i].buildable)
        .unwrap();
    let near = land.sites[free].at + Vec2::new(8.0, 3.0);
    assert_eq!(s.move_town(&mut land, TownId(0), near), Ok(()));
    let next = s.found_town(&mut land);
    let site = s.town(next).unwrap().site;
    assert_ne!(site, free);
    assert!(
        land.sites[site].at.distance(near)
            >= land.set.town.keep_apart
    );
    // One that the record sets down takes the site it would
    // have had, and stands where the record says.
    let to = meadow(&land, &s, next);
    let third = s.found_town_at(&mut land, to);
    let site = s.town(third).unwrap().site;
    assert_eq!(site, free);
    assert_eq!(land.sites[site].at, to);
    // A town that is removed leaves its site free at its own
    // place again.
    s.remove_town(third);
    let again = s.found_town(&mut land);
    let site = s.town(again).unwrap().site;
    assert_eq!(land.sites[site].at, land.sites[site].home);
}
