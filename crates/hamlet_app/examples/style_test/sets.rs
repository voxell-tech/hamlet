//! Small scenes of robots on the grass, for judging their motion.
//! Each is a cast with a script, and a place for the camera.

use bevy::prelude::*;
use hamlet_realm::keeper::{
    Act, Emblem, Hat, MERGE_GAP, Motion, Paint, TALK_GAP, Who,
};

pub struct Actor {
    pub who: Who,
    /// Metres from the middle of the set, across and toward the
    /// camera.
    pub at: Vec2,
    pub yaw: f32,
    /// The motions it plays, each with the clock time it starts.
    pub script: Vec<(f32, Act)>,
    /// A walk to make: the points after `at`, and when it sets off.
    pub trip: Option<(Vec<Vec2>, f32)>,
    /// Ground it covers per second on that walk, when it hurries.
    pub haste: Option<f32>,
    /// The clock time it is created, if it is seen to arrive.
    pub born: Option<f32>,
}

pub struct Set {
    pub name: &'static str,
    pub cast: Vec<Actor>,
    /// The point looked at, from the middle of the set.
    pub focus: Vec3,
    pub dist: f32,
    pub pitch: f32,
    pub orbit: f32,
    pub night: bool,
    /// The clock times worth a still.
    pub moments: Vec<f32>,
}

/// A robot that looks like one of the town but keeps the plain
/// timing, so that a still at a given time is the same every run.
fn plain(looks: u32) -> Who {
    Who {
        seed: 0,
        ..Who::citizen(looks)
    }
}

fn actor(who: Who, x: f32, z: f32, yaw: f32) -> Actor {
    Actor {
        who,
        at: Vec2::new(x, z),
        yaw,
        script: Vec::new(),
        trip: None,
        haste: None,
        born: None,
    }
}

fn solo(who: Who, yaw: f32, script: &[(f32, Act)]) -> Vec<Actor> {
    vec![Actor {
        script: script.to_vec(),
        ..actor(who, 0.0, 0.0, yaw)
    }]
}

fn set(name: &'static str, cast: Vec<Actor>, moments: &[f32]) -> Set {
    Set {
        name,
        cast,
        focus: Vec3::new(0.0, 0.62, 0.0),
        dist: 6.4,
        pitch: 20.0,
        orbit: 0.0,
        night: false,
        moments: moments.to_vec(),
    }
}

/// Moments spread evenly across `span` seconds from `from`.
fn spread(from: f32, span: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| from + span * i as f32 / n as f32).collect()
}

const SIDE: f32 = std::f32::consts::FRAC_PI_2;

pub fn all() -> Vec<Set> {
    let mut sets = Vec::new();

    sets.push(set(
        "idle",
        solo(plain(3), 0.3, &[]),
        &[0.15, 0.3, 1.0, 4.0, 8.0, 13.25, 14.0],
    ));

    // The same robot twice, in step: from the side and head on.
    let walker = plain(5);
    let motion = Motion::default();
    let stride = motion.stride * walker.size / motion.walk_speed;
    sets.push(Set {
        dist: 7.4,
        ..set(
            "walk",
            vec![
                Actor {
                    script: vec![(0.0, Act::Walk)],
                    ..actor(walker, -0.7, 0.0, SIDE * 0.75)
                },
                Actor {
                    script: vec![(0.0, Act::Walk)],
                    ..actor(walker, 0.8, 0.0, 0.0)
                },
            ],
            &spread(2.0, stride, 8),
        )
    });

    // A robot in a hurry rolls: across the set, and head on.
    sets.push(Set {
        dist: 7.4,
        ..set(
            "roll",
            vec![
                Actor {
                    script: vec![(0.0, Act::Walk)],
                    trip: Some((vec![Vec2::new(0.6, 0.4)], 0.0)),
                    haste: Some(3.6),
                    ..actor(walker, -1.6, 0.4, SIDE)
                },
                Actor {
                    script: vec![(0.0, Act::Walk)],
                    trip: Some((vec![Vec2::new(2.0, -0.4)], 0.0)),
                    haste: Some(3.6),
                    ..actor(walker, 2.0, -3.0, 0.0)
                },
            ],
            &[0.1, 0.2, 0.35, 0.5, 0.65, 0.8, 0.95, 1.02],
        )
    });

    // A letter carried along a road and handed over.
    let from = Vec2::new(-2.4, 0.0);
    let to = Vec2::new(0.45 - TALK_GAP, 0.0);
    let arrive =
        (to.x - from.x) / motion.walk_speed + motion.get_going;
    sets.push(Set {
        dist: 7.6,
        focus: Vec3::new(-0.6, 0.62, 0.0),
        orbit: 0.25,
        ..set(
            "deliver",
            vec![
                Actor {
                    script: vec![
                        (0.0, Act::Carry),
                        (arrive, Act::HandOver),
                    ],
                    trip: Some((vec![to], 0.0)),
                    ..actor(plain(7), from.x, from.y, SIDE)
                },
                Actor {
                    script: vec![(arrive, Act::Receive)],
                    ..actor(plain(12), 0.45, 0.0, -SIDE)
                },
            ],
            &[
                1.2,
                2.4,
                arrive + 0.25,
                arrive + 0.62,
                arrive + 0.9,
                arrive + 1.05,
                arrive + 1.45,
                arrive + 2.4,
            ],
        )
    });

    let turns = |first: Act, other: Act| {
        (0..8)
            .map(|i| {
                (
                    i as f32 * 2.4,
                    if i % 2 == 0 { first } else { other },
                )
            })
            .collect::<Vec<_>>()
    };
    sets.push(Set {
        orbit: 0.3,
        ..set(
            "talk",
            vec![
                Actor {
                    script: turns(Act::Talk, Act::Listen),
                    ..actor(plain(9), -TALK_GAP / 2.0, 0.0, SIDE)
                },
                Actor {
                    script: turns(Act::Listen, Act::Talk),
                    ..actor(Who::leader(), TALK_GAP / 2.0, 0.0, -SIDE)
                },
            ],
            &[0.9, 1.5, 2.1, 2.55, 3.3, 4.0],
        )
    });

    sets.push(set(
        "think",
        solo(plain(14), 0.35, &[(0.0, Act::Think)]),
        &[1.0, 2.0, 3.1, 4.9, 6.42],
    ));
    sets.push(set(
        "hammer",
        solo(plain(4), 0.8, &[(0.0, Act::Hammer)]),
        &spread(1.9, 0.95, 8),
    ));
    sets.push(set(
        "lift",
        solo(plain(16), 0.7, &[(0.0, Act::Lift)]),
        &spread(4.3, 4.2, 9),
    ));
    sets.push(set(
        "wait",
        solo(plain(2), 0.2, &[(0.0, Act::Wait)]),
        &[1.0, 5.75, 6.05, 7.2],
    ));
    sets.push(set(
        "object",
        solo(plain(11), 0.25, &[(1.0, Act::Object)]),
        &[0.9, 1.3, 1.44, 1.52, 1.9, 2.2, 2.45, 4.0],
    ));
    sets.push(set(
        "fault",
        solo(plain(6), 0.25, &[(1.0, Act::Fault)]),
        &[0.95, 1.05, 1.12, 1.6, 2.3, 3.4],
    ));
    sets.push(set(
        "failed",
        solo(plain(6), 0.4, &[(0.2, Act::Fault), (1.0, Act::Failed)]),
        &[0.95, 1.3, 1.6, 1.82, 1.97, 2.2, 2.7, 3.6],
    ));
    sets.push(set(
        "appear",
        vec![Actor {
            born: Some(0.0),
            ..actor(plain(8), 0.0, 0.0, 0.2)
        }],
        &[0.08, 0.25, 0.45, 0.8, 1.15, 1.75, 2.2, 2.65, 3.1],
    ));
    sets.push(set(
        "leave",
        solo(plain(8), 0.2, &[(0.0, Act::Leave)]),
        &[0.3, 0.75, 1.05, 1.7, 2.3, 2.8, 3.15, 3.4],
    ));
    sets.push(Set {
        orbit: 0.35,
        ..set(
            "merge",
            vec![
                Actor {
                    script: vec![(0.0, Act::MergeInto)],
                    ..actor(plain(13), -MERGE_GAP / 2.0, 0.0, SIDE)
                },
                Actor {
                    script: vec![(0.0, Act::MergeHost)],
                    ..actor(plain(10), MERGE_GAP / 2.0, 0.0, -SIDE)
                },
            ],
            &[0.1, 0.8, 1.25, 1.45, 1.6, 1.78, 2.0, 2.45],
        )
    });
    sets.push(Set {
        focus: Vec3::new(0.0, 0.75, 0.0),
        ..set(
            "celebrate",
            solo(plain(15), 0.2, &[(0.0, Act::Celebrate)]),
            &[
                0.05, 0.24, 0.42, 0.6, 0.76, 1.24, 1.42, 1.6, 1.78,
                2.4,
            ],
        )
    });
    sets.push(set(
        "vote",
        solo(plain(1), 0.2, &[(0.0, Act::Vote)]),
        &[0.6, 1.0, 1.45, 1.75, 2.3, 3.2],
    ));
    sets.push(Set {
        night: true,
        ..set(
            "night",
            vec![
                Actor {
                    script: vec![(0.0, Act::Sleep)],
                    ..actor(plain(3), -0.7, 0.0, 0.3)
                },
                Actor {
                    script: vec![(0.0, Act::Reflect)],
                    ..actor(plain(9), 0.7, 0.0, -0.3)
                },
            ],
            &[3.0, 5.6],
        )
    });
    // One motion easing into the next.
    sets.push(set(
        "blend",
        solo(plain(2), 0.3, &[(1.0, Act::Wait), (3.0, Act::Think)]),
        &[1.0, 1.1, 1.2, 1.3, 1.45, 3.0, 3.1, 3.2, 3.3, 3.45],
    ));

    // The leader among six of its town.
    let mut family = vec![actor(Who::leader(), 0.0, 0.0, 0.0)];
    let looks = [
        (Hat::Cap, Paint::Moss, Emblem::Dot),
        (Hat::Cone, Paint::Slate, Emblem::Bars),
        (Hat::Bowler, Paint::Plum, Emblem::Diamond),
        (Hat::Pipe, Paint::Charcoal, Emblem::Plus),
        (Hat::Knit, Paint::Sage, Emblem::Chevron),
        (Hat::Straw, Paint::Navy, Emblem::Dot),
    ];
    for (i, (hat, paint, emblem)) in looks.into_iter().enumerate() {
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let rank = (i / 2 + 1) as f32;
        family.push(actor(
            Who {
                hat,
                paint,
                emblem,
                ..Who::citizen(i as u32 + 40)
            },
            side * rank * 1.0,
            0.15 * rank,
            -side * 0.12 * rank,
        ));
    }
    sets.push(Set {
        dist: 11.5,
        ..set("family", family, &[1.0])
    });

    sets.push(Set {
        dist: 21.0,
        pitch: 34.0,
        focus: CROWD_AT.extend(0.0).xzy() + Vec3::Y * 0.4,
        ..set("crowd", crowd(48), &[2.0])
    });
    sets.push(Set {
        dist: 21.0,
        pitch: 34.0,
        focus: CROWD_AT.extend(0.0).xzy() + Vec3::Y * 0.4,
        ..set("crowd-later", crowd(48), &[5.5])
    });
    sets.push(Set {
        dist: 21.0,
        pitch: 34.0,
        focus: CROWD_AT.extend(0.0).xzy() + Vec3::Y * 0.4,
        night: true,
        ..set("crowd-night", crowd(48), &[2.0])
    });
    // Towns of several sizes, for timing.
    for (name, n) in
        [("town-1", 1), ("town-48", 48), ("town-192", 192)]
    {
        sets.push(Set {
            dist: 34.0,
            pitch: 34.0,
            focus: CROWD_AT.extend(0.0).xzy(),
            ..set(name, crowd(n), &[])
        });
    }
    sets
}

/// The middle of the crowd, clear of the canal.
const CROWD_AT: Vec2 = Vec2::new(3.4, 1.2);

/// A town's worth of robots, each busy with something of its own.
pub fn crowd(n: usize) -> Vec<Actor> {
    let loops = [
        Act::Idle,
        Act::Think,
        Act::Idle,
        Act::Hammer,
        Act::Idle,
        Act::Lift,
        Act::Idle,
        Act::Wait,
        Act::Idle,
        Act::Celebrate,
        Act::Idle,
        Act::Fault,
    ];
    let across = (n as f32 * 1.5).sqrt().ceil() as usize;
    let mut cast = vec![Actor {
        script: vec![(0.0, Act::Think)],
        ..actor(Who::leader(), CROWD_AT.x, CROWD_AT.y + 0.6, 0.0)
    }];
    for i in 1..n {
        let who = Who::citizen(i as u32 * 7 + 3);
        let r = |k: u32| {
            let h = (i as u32)
                .wrapping_mul(2654435761)
                .wrapping_add(k.wrapping_mul(40503));
            ((h >> 8) & 0xffff) as f32 / 65535.0
        };
        let x = ((i % across) as f32 - across as f32 / 2.0) * 1.1
            + r(1) * 0.5
            + CROWD_AT.x;
        let z = ((i / across) as f32
            - n as f32 / across as f32 / 2.0)
            * 1.2
            + r(2) * 0.5
            + CROWD_AT.y;
        let mut a = actor(who, x, z, (r(3) - 0.5) * 1.6);
        match i % 6 {
            // Pairs in talk.
            0 if i % across != 0 => {
                a.at = cast[i - 1].at + Vec2::X * TALK_GAP;
                a.yaw = -SIDE;
                a.script = vec![(0.0, Act::Listen)];
                cast[i - 1].yaw = SIDE;
                cast[i - 1].script = vec![(0.0, Act::Talk)];
                cast[i - 1].trip = None;
            }
            // Walkers crossing the square.
            1 => {
                let far = Vec2::new(
                    2.0 * CROWD_AT.x - a.at.x,
                    a.at.y + r(4) - 0.5,
                );
                let act =
                    if i % 4 == 1 { Act::Carry } else { Act::Walk };
                a.script = vec![(0.0, act)];
                a.trip = Some((vec![far], r(5) * 2.0));
            }
            _ => {
                let act = loops[(i * 7 + i / 5) % loops.len()];
                let again =
                    if act.length().is_some() { 12 } else { 1 };
                a.script = (0..again)
                    .map(|k| (r(6) * 3.0 - 3.0 + k as f32 * 4.0, act))
                    .collect();
            }
        }
        cast.push(a);
    }
    cast
}
