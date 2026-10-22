//! The bazaar's ground, which keeps the towns off, and the names a
//! stall goes by beyond the realm.

use super::*;
use crate::places::bazaar::pitch::{GROUNDS, pitch};
use crate::society::Place;

/// The land and an empty society, as the realm begins with them.
fn land() -> (Land, Society) {
    let (tuning, pal) = WorldTuning::read().unwrap();
    (Land::new(7, tuning.generator, pal), Society::default())
}

#[test]
fn towns_founded_after_the_bazaar_keep_off_its_ground() {
    let (mut land, mut society) = land();
    let at = pitch(&mut land, &mut society).unwrap().at.xz();
    let (claimed, radius) =
        society.stands(&land, Place::Bazaar).unwrap();
    assert_eq!(claimed, at);
    assert!(radius >= GROUNDS);
    assert!(land.holds(at, 0.0));
    let room = land.set.town.keep_apart * 0.5;
    for _ in 0..7 {
        society.found_town(&mut land);
    }
    for town in &society.towns {
        let site = land.sites[town.site].at;
        assert!(
            site.distance(at) >= room + radius - 1e-3,
            "a town at {site} on the bazaar's ground at {at}"
        );
    }
}

#[test]
fn a_stall_is_named_alike_by_its_key_and_its_events() {
    let shorts = Stall::ALL.map(Stall::short);
    assert_eq!(
        shorts,
        ["models", "community", "plugins", "skills", "hamlets"]
    );
    for stall in Stall::ALL {
        assert_eq!(Stall::named(stall.short()), Some(stall));
        let thing = Thing::Stall(stall);
        assert_eq!(Thing::of(thing.key()), Some(thing));
    }
    assert_eq!(Thing::of(Thing::Bazaar.key()), Some(Thing::Bazaar));
}
