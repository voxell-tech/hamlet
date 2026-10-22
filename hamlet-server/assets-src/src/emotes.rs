//! The emotes: an agent's gestures, and what moves about a hamlet's
//! houses.

use std::collections::BTreeMap;

use looks::wardrobe::{
    Banners, Clip, Emote, Fireflies, Key, Lanterns, Move, Petals,
    Pinwheels, Smoke,
};

/// A pose: parts turned by degrees about X, Y and Z.
fn pose(at: f32, parts: &[(&str, [f32; 3])]) -> Key {
    Key {
        at,
        parts: parts
            .iter()
            .map(|(part, turn)| {
                (
                    part.to_string(),
                    Move {
                        turn: *turn,
                        shift: [0.0; 3],
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    }
}

/// A pose with a part moved too.
fn lifted(mut key: Key, part: &str, shift: [f32; 3]) -> Key {
    key.parts.entry(part.to_string()).or_default().shift = shift;
    key
}

/// The emote of the asset `id`, if it is one.
pub fn of(id: &str) -> Option<Emote> {
    Some(match id {
        // The right arm raised high, waved twice, and lowered.
        "agent-emote-wave" => Emote::Clip(Clip {
            seconds: 2.4,
            every: 40.0,
            keys: vec![
                pose(0.0, &[]),
                pose(
                    0.45,
                    &[
                        ("arm_r", [0.0, 0.0, 150.0]),
                        ("head", [0.0, -10.0, 0.0]),
                    ],
                ),
                pose(
                    0.8,
                    &[
                        ("arm_r", [0.0, 0.0, 125.0]),
                        ("head", [0.0, -10.0, 0.0]),
                    ],
                ),
                pose(
                    1.15,
                    &[
                        ("arm_r", [0.0, 0.0, 155.0]),
                        ("head", [0.0, -10.0, 0.0]),
                    ],
                ),
                pose(
                    1.5,
                    &[
                        ("arm_r", [0.0, 0.0, 125.0]),
                        ("head", [0.0, -10.0, 0.0]),
                    ],
                ),
                pose(1.85, &[("arm_r", [0.0, 0.0, 150.0])]),
                pose(2.4, &[]),
            ],
        }),
        // Bent forward from the body, head low, an arm across.
        "agent-emote-bow" => Emote::Clip(Clip {
            seconds: 2.8,
            every: 60.0,
            keys: vec![
                pose(0.0, &[]),
                pose(
                    0.7,
                    &[
                        ("body", [35.0, 0.0, 0.0]),
                        ("head", [20.0, 0.0, 0.0]),
                        ("arm_r", [-60.0, 0.0, 30.0]),
                        ("hat", [10.0, 0.0, 0.0]),
                    ],
                ),
                pose(
                    1.6,
                    &[
                        ("body", [35.0, 0.0, 0.0]),
                        ("head", [20.0, 0.0, 0.0]),
                        ("arm_r", [-60.0, 0.0, 30.0]),
                        ("hat", [10.0, 0.0, 0.0]),
                    ],
                ),
                pose(2.8, &[]),
            ],
        }),
        // A bob and a sway from side to side, the arms swinging in
        // turn, a quarter turn and a hop at the end.
        "agent-emote-dance" => Emote::Clip(Clip {
            seconds: 3.2,
            every: 50.0,
            keys: vec![
                pose(0.0, &[]),
                lifted(
                    pose(
                        0.4,
                        &[
                            ("rig", [0.0, 25.0, 0.0]),
                            ("body", [0.0, 0.0, 10.0]),
                            ("arm_l", [0.0, 0.0, -70.0]),
                            ("arm_r", [0.0, 0.0, 20.0]),
                        ],
                    ),
                    "rig",
                    [0.0, 0.05, 0.0],
                ),
                pose(
                    0.8,
                    &[
                        ("rig", [0.0, -25.0, 0.0]),
                        ("body", [0.0, 0.0, -10.0]),
                        ("arm_l", [0.0, 0.0, -20.0]),
                        ("arm_r", [0.0, 0.0, 70.0]),
                    ],
                ),
                lifted(
                    pose(
                        1.2,
                        &[
                            ("rig", [0.0, 25.0, 0.0]),
                            ("body", [0.0, 0.0, 10.0]),
                            ("arm_l", [0.0, 0.0, -70.0]),
                            ("arm_r", [0.0, 0.0, 20.0]),
                        ],
                    ),
                    "rig",
                    [0.0, 0.05, 0.0],
                ),
                pose(
                    1.6,
                    &[
                        ("rig", [0.0, -25.0, 0.0]),
                        ("body", [0.0, 0.0, -10.0]),
                        ("arm_l", [0.0, 0.0, -20.0]),
                        ("arm_r", [0.0, 0.0, 70.0]),
                    ],
                ),
                pose(2.1, &[("rig", [0.0, 90.0, 0.0])]),
                // Back to face the front for the hop: a clip is
                // eased out from its last pose, so it ends where it
                // began.
                lifted(
                    pose(
                        2.6,
                        &[
                            ("arm_l", [0.0, 0.0, -150.0]),
                            ("arm_r", [0.0, 0.0, 150.0]),
                        ],
                    ),
                    "rig",
                    [0.0, 0.12, 0.0],
                ),
                pose(3.2, &[]),
            ],
        }),
        "hamlet-emote-smoke" => Emote::Smoke(Smoke {
            colour: "#d9d4cb".into(),
            rate: 2.5,
            size: 0.22,
            rise: 1.8,
        }),
        "hamlet-emote-lanterns" => Emote::Lanterns(Lanterns {
            paper: "#d23a2a".into(),
            light: "#ffb45c".into(),
            count: 2,
            sway: 9.0,
        }),
        "hamlet-emote-banners" => Emote::Banners(Banners {
            colours: ["#2f6db3".into(), "#f2d35b".into()],
            long: 0.7,
        }),
        // Both arms flung up, a hop at the top, and down again.
        "agent-emote-cheer" => Emote::Clip(Clip {
            seconds: 2.0,
            every: 45.0,
            keys: vec![
                pose(0.0, &[]),
                lifted(
                    pose(
                        0.35,
                        &[
                            ("arm_l", [0.0, 0.0, -160.0]),
                            ("arm_r", [0.0, 0.0, 160.0]),
                            ("head", [-15.0, 0.0, 0.0]),
                        ],
                    ),
                    "rig",
                    [0.0, 0.14, 0.0],
                ),
                pose(
                    0.6,
                    &[
                        ("arm_l", [0.0, 0.0, -140.0]),
                        ("arm_r", [0.0, 0.0, 140.0]),
                        ("head", [-10.0, 0.0, 0.0]),
                    ],
                ),
                lifted(
                    pose(
                        0.9,
                        &[
                            ("arm_l", [0.0, 0.0, -165.0]),
                            ("arm_r", [0.0, 0.0, 165.0]),
                            ("head", [-15.0, 0.0, 0.0]),
                        ],
                    ),
                    "rig",
                    [0.0, 0.1, 0.0],
                ),
                pose(
                    1.3,
                    &[
                        ("arm_l", [0.0, 0.0, -40.0]),
                        ("arm_r", [0.0, 0.0, 40.0]),
                    ],
                ),
                pose(2.0, &[]),
            ],
        }),
        // Shoulders up, forearms out and palms up, the head tilted.
        "agent-emote-shrug" => Emote::Clip(Clip {
            seconds: 2.2,
            every: 55.0,
            keys: vec![
                pose(0.0, &[]),
                lifted(
                    pose(
                        0.4,
                        &[
                            ("arm_l", [-50.0, 0.0, -45.0]),
                            ("arm_r", [-50.0, 0.0, 45.0]),
                            ("head", [0.0, 0.0, 14.0]),
                        ],
                    ),
                    "body",
                    [0.0, 0.03, 0.0],
                ),
                lifted(
                    pose(
                        1.3,
                        &[
                            ("arm_l", [-50.0, 0.0, -45.0]),
                            ("arm_r", [-50.0, 0.0, 45.0]),
                            ("head", [0.0, 0.0, 14.0]),
                        ],
                    ),
                    "body",
                    [0.0, 0.03, 0.0],
                ),
                pose(2.2, &[]),
            ],
        }),
        // A hand to the chin, the head down and turning slowly from
        // side to side, then a nod: it has it.
        "agent-emote-ponder" => Emote::Clip(Clip {
            seconds: 3.6,
            every: 70.0,
            keys: vec![
                pose(0.0, &[]),
                pose(
                    0.6,
                    &[
                        ("arm_r", [-100.0, 0.0, -25.0]),
                        ("head", [12.0, -18.0, 0.0]),
                    ],
                ),
                pose(
                    1.6,
                    &[
                        ("arm_r", [-105.0, 0.0, -25.0]),
                        ("head", [12.0, 18.0, 0.0]),
                    ],
                ),
                pose(
                    2.4,
                    &[
                        ("arm_r", [-100.0, 0.0, -25.0]),
                        ("head", [12.0, -10.0, 0.0]),
                    ],
                ),
                pose(2.8, &[("head", [-12.0, 0.0, 0.0])]),
                pose(3.0, &[("head", [10.0, 0.0, 0.0])]),
                pose(3.6, &[]),
            ],
        }),
        "hamlet-emote-petals" => Emote::Petals(Petals {
            colours: ["#f48fb1".into(), "#fbd3e0".into()],
            count: 14,
            size: 0.11,
            fall: 7.0,
        }),
        "hamlet-emote-fireflies" => Emote::Fireflies(Fireflies {
            colour: "#e8ff8a".into(),
            count: 10,
            glow: 16.0,
        }),
        "hamlet-emote-pinwheels" => Emote::Pinwheels(Pinwheels {
            colours: ["#e2405a".into(), "#f2d35b".into()],
            count: 2,
            speed: 0.7,
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use looks::wardrobe::{Category, EmoteFile, FORMAT};

    use super::*;
    use crate::catalog::ALL;

    #[test]
    fn every_emote_reads_back_as_the_client_reads_it() {
        for entry in
            ALL.iter().filter(|e| e.category == Category::Emote)
        {
            let emote = of(entry.id).expect(entry.id);
            let bytes = serde_json::to_vec(&EmoteFile {
                format: FORMAT,
                emote: emote.clone(),
            })
            .unwrap();
            assert_eq!(
                Emote::read(&bytes, entry.target).unwrap(),
                emote,
                "{}",
                entry.id
            );
        }
    }
}
