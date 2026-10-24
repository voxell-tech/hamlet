//! A made-up society, and the sky with effects switched.

use bevy::prelude::*;
use hamlet_realm::generator::land::Land;
use hamlet_realm::generator::noise::{key, unit};
use hamlet_realm::generator::society::{RoadLevel, Society};
use hamlet_realm::sky::SkySettings;

/// The site of the town an agent lives in.
pub fn site_of(
    world: &Society,
    agent: &hamlet_realm::generator::society::Agent,
) -> usize {
    world.town(agent.town).map(|t| t.site).unwrap_or(0)
}

/// The sky's settings with the named effects switched: `shafts`,
/// `occlusion`, `atmosphere`, `nolamps` (no light from lanterns) and
/// `fixed` (the sun does not move), separated by commas.
pub fn effects(names: &str) -> SkySettings {
    let mut set = SkySettings::default();
    for name in names.split(',') {
        match name.trim() {
            "shafts" => set.shafts = true,
            "occlusion" => set.occlusion = true,
            "atmosphere" => set.atmosphere = true,
            "nolamps" => set.door_lumens = 0.0,
            "fixed" => set.moving = false,
            _ => {}
        }
    }
    set
}

/// A society of towns of the given sizes, with some roads between
/// agents and a spread of road levels. The same arguments always
/// give the same society.
pub fn society(
    land: &Land,
    sizes: &[usize],
    roads: usize,
) -> Society {
    let mut s = Society::default();
    // Only the sites the land starts with, since this land is
    // everyone's.
    let room = land.sites.iter().filter(|s| s.buildable).count();
    for &size in sizes.iter().take(room) {
        let town = s.found_town(&mut land.clone());
        for _ in 0..size {
            s.add_agent(land, town);
        }
    }
    let towns = s.towns.clone();
    for (t, town) in towns.iter().enumerate() {
        let ids =
            s.agents_of(town.id).map(|a| a.id).collect::<Vec<_>>();
        for k in 0..roads.min(ids.len()) {
            let h = key(&[t as u64, k as u64, 1]);
            let a = ids[(h % ids.len() as u64) as usize];
            let b = ids[(key(&[h, 2]) % ids.len() as u64) as usize];
            s.add_road(a, b);
        }
        // Now and then two agents in neighbouring towns are joined.
        if t > 0 && !ids.is_empty() && roads > 0 {
            let other = s
                .agents_of(towns[t - 1].id)
                .map(|a| a.id)
                .collect::<Vec<_>>();
            let h = key(&[t as u64, 3]);
            s.add_road(
                ids[(h % ids.len() as u64) as usize],
                other[(key(&[h, 4]) % other.len() as u64) as usize],
            );
        }
    }
    for i in 0..s.roads.len() {
        let pair = s.roads[i].pair;
        let roll = unit(key(&[pair.0.0 as u64, pair.1.0 as u64, 6]));
        let level = if roll < 0.08 {
            RoadLevel::Overgrown
        } else if roll < 0.6 {
            RoadLevel::Dirt
        } else if roll < 0.88 {
            RoadLevel::Paved
        } else {
            RoadLevel::Rail
        };
        s.set_level(pair, level);
    }
    s
}
