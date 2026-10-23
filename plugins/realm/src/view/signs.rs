//! The names shown over things: a sign over each house, common,
//! workbench and the Hearth while the camera is near enough to read
//! it.

use std::collections::HashMap;

use glam::Vec3;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::LabelLook;

use crate::build::frame::Frame;
use crate::build::upload::Draw;
use crate::view::pick::Thing;

/// The key of the entities the words hang from.
const SIGN: u32 = 7;

/// Pixels between a thing's top and the foot of its sign.
const GAP: f32 = 4.0;

/// The words over one thing, and the point they hang from.
#[derive(Clone, PartialEq, Debug)]
pub struct Words {
    pub at: Vec3,
    pub says: String,
}

/// A sign as it is shown.
struct Sign {
    entity: Entity,
    words: Words,
}

/// Every sign.
#[derive(Resource, Default)]
pub struct Signs {
    signs: HashMap<Thing, Sign>,
    /// The signs are to be put up anew.
    pub stale: bool,
}

impl Signs {
    /// Puts up a sign over each thing of `signs`, and takes down the
    /// others. A sign is read within `far` of the camera.
    pub fn put_up(
        &mut self,
        signs: Vec<(Thing, Words)>,
        far: f32,
        draw: &mut Draw,
    ) {
        self.stale = false;
        let mut kept = HashMap::new();
        for (thing, words) in signs {
            let sign = match self.signs.remove(&thing) {
                Some(mut sign) => {
                    if sign.words.at != words.at {
                        draw.place(sign.entity, Frame::at(words.at));
                    }
                    if sign.words.says != words.says {
                        draw.commands
                            .entity(sign.entity)
                            .insert(SceneLabel(words.says.clone()));
                    }
                    sign.words = words;
                    sign
                }
                None => {
                    let entity = draw.spawn(None, SIGN);
                    draw.place(entity, Frame::at(words.at));
                    draw.commands.entity(entity).insert((
                        SceneLabel(words.says.clone()),
                        LabelStyle {
                            look: LabelLook::Sign,
                            gap: GAP,
                            far,
                        },
                    ));
                    Sign { entity, words }
                }
            };
            kept.insert(thing, sign);
        }
        for sign in self.signs.values() {
            draw.despawn(sign.entity);
        }
        self.signs = kept;
    }
}
