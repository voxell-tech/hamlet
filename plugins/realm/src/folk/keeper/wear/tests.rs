//! A worn clip plays on top of a robot's own motion, only on the
//! parts it names, now and then while the robot is idle.

use std::collections::BTreeMap;

use looks::wardrobe::{Key, Move};

use super::*;

const SET: WearSettings = WearSettings {
    emote_blend: 0.3,
    emote_spread: 0.5,
};

/// A clip that turns the right arm 90 degrees about Z and lifts the
/// head 0.1, at its middle.
fn clip() -> Clip {
    let still = || Key {
        at: 0.0,
        parts: BTreeMap::new(),
    };
    Clip {
        seconds: 2.0,
        every: 10.0,
        keys: vec![
            still(),
            Key {
                at: 1.0,
                parts: BTreeMap::from([
                    (
                        "arm_r".into(),
                        Move {
                            turn: [0.0, 0.0, 90.0],
                            ..Move::default()
                        },
                    ),
                    (
                        "head".into(),
                        Move {
                            shift: [0.0, 0.1, 0.0],
                            ..Move::default()
                        },
                    ),
                ]),
            },
            Key { at: 2.0, ..still() },
        ],
    }
}

#[test]
fn a_clip_moves_only_the_parts_it_names() {
    let mut gesture = Gesture::default();
    gesture.play(0.0);
    let moves = gesture.moves(&clip(), 1.0, false, 3, &SET).unwrap();
    let posed = PARTS.map(|_| {
        Frame::at(Vec3::new(0.1, 0.2, 0.3))
            .with_turn(Quat::from_rotation_y(0.4))
    });
    let mut now = posed;
    let [rig, body, head, hat, arm_l, arm_r, leg_l, leg_r] = &mut now;
    moved([rig, body, head, hat, arm_l, arm_r, leg_l, leg_r], moves);
    let at =
        |name: &str| PARTS.iter().position(|p| *p == name).unwrap();
    let turned = posed[at("arm_r")].turn
        * Quat::from_rotation_z(90f32.to_radians());
    assert!(now[at("arm_r")].turn.angle_between(turned) < 1e-3);
    let lifted = now[at("head")].at - posed[at("head")].at;
    assert!((lifted - Vec3::Y * 0.1).length() < 1e-5);
    for name in ["rig", "body", "hat", "arm_l", "leg_l", "leg_r"] {
        assert_eq!(now[at(name)], posed[at(name)], "{name}");
    }
    // Over, it moves nothing.
    assert!(gesture.moves(&clip(), 2.5, false, 3, &SET).is_none());
}

#[test]
fn an_idle_robot_plays_its_clip_at_most_every_every() {
    let clip = clip();
    let starts = |seed: u32, idle: bool| {
        let mut gesture = Gesture::default();
        let mut starts = Vec::new();
        for i in 0..2000 {
            let now = i as f32 * 0.05;
            let was = gesture.since;
            gesture.moves(&clip, now, idle, seed, &SET);
            if gesture.since.is_some() && gesture.since != was {
                starts.push(now);
            }
        }
        starts
    };
    let (a, b) = (starts(5, true), starts(6, true));
    assert!(a[0] < clip.every && a[0] != b[0]);
    for w in a.windows(2) {
        let gap = w[1] - w[0];
        assert!(gap >= clip.every);
        assert!(gap <= clip.every * (1.0 + SET.emote_spread) + 0.05);
    }
    assert!(starts(5, false).is_empty());
}
