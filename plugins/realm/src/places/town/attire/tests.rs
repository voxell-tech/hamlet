//! A hamlet's houses wear its skin near and far, its mesh in place
//! of their walls, and its emote about each of them.

use glam::{Vec2, Vec3};
use looks::wardrobe::{Fireflies, Petals, Pinwheels, Smoke, Target};

use super::*;
use crate::places::house::{self, TownStyle};
use crate::society::AgentId;
use crate::tuning::WorldTuning;
use crate::wardrobe::glb::tests::{triangle, write};
use crate::world::land::Land;

fn land() -> (Land, WorldTuning) {
    let (tuning, pal) = WorldTuning::read().unwrap();
    (Land::new(7, tuning.generator.clone(), pal), tuning)
}

fn skin(json: &str) -> Skin {
    Skin::read(json.as_bytes(), Target::Hamlet).unwrap()
}

fn colours(mb: Mb) -> Vec<[f32; 4]> {
    mb.data()
        .colors
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect()
}

/// True for a colour of nothing but blue.
fn blue(c: &[f32; 4]) -> bool {
    c[0] == 0.0 && c[1] == 0.0 && c[2] > 0.0
}

/// True for a colour of nothing but red.
fn red(c: &[f32; 4]) -> bool {
    c[0] > 0.0 && c[1] == 0.0 && c[2] == 0.0
}

#[test]
fn a_skin_recolours_its_slots_of_a_house_near_and_far_and_no_other() {
    let (land, _) = land();
    let worn = skinned(
        &land.pal,
        &skin(
            r##"{"format":1,"colours":{"livery":"#0000ff",
            "roof":"#ff0000","drift":"#ff0000"}}"##,
        ),
    );
    let agent = AgentId(2);
    let yard = house::yard_of(&land, agent, false, 0.0, 0);
    let near = |pal: &Palette| {
        let style = TownStyle::dyed(&land, pal, 0);
        let built = house::dressed(
            &land,
            pal,
            agent,
            &style,
            (false, false, 0),
            &yard,
            Fit::default(),
        );
        colours(built.body)
    };
    let (plain, dyed) = (near(&land.pal), near(&worn));
    assert_eq!(plain.len(), dyed.len());
    // The doors and shutters wear the livery, the roof its own
    // colour, and the walls are as they were.
    assert!(dyed.iter().any(blue) && !plain.iter().any(blue));
    let kept = plain
        .iter()
        .zip(&dyed)
        .filter(|(was, now)| was == now)
        .count();
    assert!(kept > dyed.len() / 2);

    let far = |pal: &Palette| {
        let (mut mb, mut panes) = (Mb::new(), Mb::new());
        let style = TownStyle::dyed(&land, pal, 0);
        house::far(
            (&mut mb, &mut panes),
            Default::default(),
            (&land, pal),
            agent,
            &style,
            (false, None),
            None,
        );
        colours(mb)
    };
    let (plain, dyed) = (far(&land.pal), far(&worn));
    assert!(dyed.iter().any(red) && !plain.iter().any(red));
    for (was, now) in plain.iter().zip(&dyed) {
        assert!(was == now || red(now));
    }
}

#[test]
fn a_worn_mesh_stands_for_the_walls_and_its_spots_are_used() {
    let (land, _) = land();
    let walls = triangle(
        [
            Vec3::new(-1.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::Y * 2.0,
        ],
        Vec3::splat(0.5),
    );
    let panes = triangle([Vec3::ZERO, Vec3::X, Vec3::Y], Vec3::ONE);
    let (lamp, chimney) =
        (Vec3::new(0.4, 1.3, 1.25), Vec3::new(-0.5, 3.1, -0.2));
    let bytes = write(
        &[("walls", &walls), ("panes", &panes)],
        &[("lamp", lamp), ("chimney", chimney)],
    );
    let shell = Shell::of(&Glb::read(&bytes).unwrap()).unwrap();
    assert_eq!(shell.panes.as_ref(), Some(&panes));
    assert_eq!(shell.spots.lamp, lamp);
    // Not marked, the door and the eave are found from the walls and
    // the lamp.
    assert_eq!(shell.spots.door, Vec3::new(0.0, 0.0, 1.0));
    assert_eq!(shell.spots.eaves.z, shell.bulk.max.z);

    let agent = AgentId(2);
    let yard = house::yard_of(&land, agent, false, 0.0, 0);
    let style = TownStyle::of(&land, 0);
    let build = |fit| {
        house::dressed(
            &land,
            &land.pal,
            agent,
            &style,
            (false, false, 0),
            &yard,
            fit,
        )
    };
    let own = build(Fit::default());
    let worn = build(Fit {
        shell: Some(&shell),
        banner: None,
    });
    // Stretched to the house's footprint, its height kept.
    let by = worn.spots.lamp / lamp;
    assert_eq!(by.y, 1.0);
    assert!(by.x > 1.0 && by.z > 1.0);
    assert_eq!(worn.spots.chimney, chimney * by);
    // Its own walls, roof and windows are left out, its grounds not.
    let tris = |mb: Mb| mb.data().indices.len() / 3;
    assert_eq!(tris(worn.panes), 1);
    assert!(tris(worn.body) * 2 < tris(own.body));
}

/// A house to hang an emote about.
fn perch() -> Perch {
    Perch {
        frame: Frame::IDENTITY,
        chimney: Vec3::new(0.5, 3.2, -0.3),
        eaves: [Vec3::new(-1.0, 1.9, 1.3), Vec3::new(1.0, 1.9, 1.3)],
    }
}

#[test]
fn an_emote_hangs_about_a_house_as_many_as_asked_and_no_more() {
    let (_, tuning) = land();
    let set = &tuning.attire;
    let count = |emote: &Emote, kind: Kind| {
        parts(emote, kind, &perch(), 1.0, set).len()
    };
    let petals = Emote::Petals(Petals {
        colours: ["#ffc0cb".into(), "#ffffff".into()],
        count: 5,
        size: 0.06,
        fall: 6.0,
    });
    assert_eq!(
        count(&petals, Kind::Petal(0))
            + count(&petals, Kind::Petal(1)),
        5
    );
    // Asking for more than the settings allow gets no more.
    let fireflies = Emote::Fireflies(Fireflies {
        colour: "#eeff88".into(),
        count: 1000,
        glow: 4.0,
    });
    assert_eq!(
        count(&fireflies, Kind::Firefly),
        set.fireflies as usize
    );
    let pinwheels = Emote::Pinwheels(Pinwheels {
        colours: ["#ff0000".into(), "#ffff00".into()],
        count: 2,
        speed: 1.0,
    });
    assert_eq!(count(&pinwheels, Kind::Pinwheel), 2);
    assert_eq!(count(&pinwheels, Kind::Stick), 2);
    let smoke = Emote::Smoke(Smoke {
        colour: "#dddddd".into(),
        rate: 1000.0,
        size: 0.3,
        rise: 1.5,
    });
    assert_eq!(count(&smoke, Kind::Puff), set.puffs as usize);
    // Each kind only of its own emote.
    assert_eq!(count(&smoke, Kind::Firefly), 0);
}

#[test]
fn a_petal_falls_from_its_eave_to_the_ground_and_comes_round_again() {
    let from = Vec3::new(0.0, 2.0, 1.0);
    let at = |t: f32| petal(from, (0.1, 4.0), (0.0, 0.3), t);
    let high = |t: f32| at(t).at.y;
    assert!((high(0.0) - 2.0).abs() < 1e-4);
    assert!(high(1.0) > high(2.0) && high(2.0) > high(3.0));
    assert!(high(3.999) < 0.1 && high(3.999) > 0.0);
    // Gone at both ends of its fall: it never pops in or out.
    assert!(at(0.0).size.x < 1e-4);
    assert!(at(3.9999).size.x < 1e-3);
    assert!((high(4.0) - high(0.0)).abs() < 1e-3);
}

#[test]
fn a_firefly_wanders_about_its_house_and_never_into_it() {
    let mut sides = [false; 4];
    for step in 0..2000 {
        let at =
            firefly((1.5, 0.6), 1.0, (0.3, 0.4), step as f32 * 0.05)
                .at;
        let out = Vec2::new(at.x, at.z).length();
        assert!((1.5 - 1e-3..=2.1 + 1e-3).contains(&out), "{out}");
        sides
            [usize::from(at.x > 0.0) * 2 + usize::from(at.z > 0.0)] =
            true;
    }
    // It goes all the way round.
    assert_eq!(sides, [true; 4]);
}
