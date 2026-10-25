use bevy::prelude::*;

use super::{HOLD_CLEAR, Land};
use crate::generator::testing;

fn land() -> Land {
    testing::land(1)
}

#[test]
fn a_corridor_goes_round_held_ground() {
    let mut land = land();
    let plain = land.corridor(3, 5);
    let on = plain[plain.len() / 2];
    land.hold(&[(on, 8.5)]);
    let bent = land.corridor(3, 5);
    assert!(plain.iter().any(|p| p.distance(on) < 8.5));
    let inner = &bent[1..bent.len() - 1];
    for p in inner {
        assert!(p.distance(on) >= 8.5 + HOLD_CLEAR - 1e-3, "{p}");
    }
    // It still ends at the same gates, and has no leap in it.
    assert_eq!(bent[0], plain[0]);
    assert_eq!(bent.last(), plain.last());
    let longest = |way: &[Vec2]| {
        way.windows(2)
            .map(|w| w[0].distance(w[1]))
            .fold(0.0, f32::max)
    };
    assert!(longest(&bent) < longest(&plain) + 4.0);
}

#[test]
fn there_is_always_one_more_site() {
    let mut land = land();
    let apart = land.set.town.keep_apart;
    for _ in 0..60 {
        let before = land.sites.clone();
        let new = land.open_site(|_| false);
        assert_eq!(new, before.len());
        assert_eq!(land.sites[..new], before[..]);
        let at = land.sites[new].at;
        assert_eq!(land.suits(at), Ok(()));
        assert!(
            (land.height(at) - land.set.land.site_y).abs() < 1e-3
        );
        for old in &before {
            assert!(old.at.distance(at) >= apart);
        }
    }
    // The new ground lies ever further out, not all at one side.
    let far = land.sites[37..]
        .iter()
        .map(|s| s.at.length())
        .fold(0.0, f32::max);
    assert!(far > 400.0, "{far}");
}

#[test]
fn the_land_is_the_same_far_out_whatever_was_asked_first() {
    let (a, b) = (land(), land());
    let spots = (0..200)
        .map(|i| {
            let turn = Vec2::from_angle(i as f32 * 2.4);
            turn * (i as f32 * 97.0 + 50.0)
        })
        .collect::<Vec<_>>();
    let near_first =
        spots.iter().map(|&p| a.height(p)).collect::<Vec<_>>();
    let far_first =
        spots.iter().rev().map(|&p| b.height(p)).collect::<Vec<_>>();
    assert!(near_first.iter().eq(far_first.iter().rev()));
    // It is not all one plain: there is water and high ground
    // out there.
    assert!(spots.iter().any(|&p| a.is_water(p)));
    assert!(near_first.iter().any(|&h| h > 12.0));
}

#[test]
fn a_moved_site_takes_its_level_ground_along() {
    let mut land = land();
    let was = land.sites[4].at;
    let to = (0..400)
        .map(|i| {
            Vec2::from_angle(i as f32) * (300.0 + i as f32 * 5.0)
        })
        .find(|&p| {
            land.suits(p).is_ok()
                && (land.height(p) - land.set.land.site_y).abs() > 0.5
        })
        .unwrap();
    let others = land
        .sites
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 4)
        .map(|(_, s)| (s.at, land.height(s.at)))
        .collect::<Vec<_>>();
    land.move_site(4, to);
    assert!((land.height(to) - land.set.land.site_y).abs() < 1e-3);
    assert_eq!(land.site_dist(to), 0.0);
    assert!(land.site_dist(was) > 20.0);
    for (at, height) in others {
        assert_eq!(land.height(at), height);
    }
}

#[test]
fn roads_reach_a_town_that_was_moved_far_away() {
    let mut land = land();
    let radius = land.set.town.radius;
    let to = (0..400)
        .map(|i| {
            Vec2::from_angle(i as f32) * (1500.0 + i as f32 * 5.0)
        })
        .find(|&p| land.suits(p).is_ok())
        .unwrap();
    land.move_site(2, to);
    land.move_site(9, land.sites[0].at + Vec2::new(70.0, 20.0));
    for (a, b) in [(0, 2), (2, 9), (5, 9), (0, 20)] {
        let way = land.corridor(a, b);
        let (from, till) = (land.sites[a].at, land.sites[b].at);
        assert!(way[0].distance(from) < radius + 10.0);
        assert!(way.last().unwrap().distance(till) < radius + 10.0);
        for step in way.windows(2) {
            assert!(step[0].distance(step[1]) < 20.0);
        }
        // It passes through no town that was moved into its way.
        for (i, site) in land.sites.iter().enumerate() {
            if i == a || i == b || !site.has_moved() {
                continue;
            }
            for p in &way {
                assert!(
                    p.distance(site.at) > radius - 6.0,
                    "{a}-{b} through {i}"
                );
            }
        }
    }
}
