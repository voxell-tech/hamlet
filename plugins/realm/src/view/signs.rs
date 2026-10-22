//! The names shown over things: a sign over each house, common and
//! the Hearth while the camera is near enough to read it, and a tag
//! over whatever else the pointer is on.

use std::collections::HashMap;

use glam::Vec3;
use hamlet_engine_bevy::prelude::*;

use crate::build::frame::Frame;
use crate::build::upload::Draw;
use crate::view::pick::Thing;

/// The key of the entities the words hang from.
const SIGN: u32 = 7;

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
    /// True while the camera is near enough to read it.
    near: bool,
}

/// Every sign and the tag over the thing under the pointer.
#[derive(Resource)]
pub struct Signs {
    signs: HashMap<Thing, Sign>,
    tag: Entity,
    tagged: Option<Words>,
    /// The signs are to be put up anew.
    pub stale: bool,
}

impl Signs {
    pub fn make(draw: &mut Draw) -> Self {
        let tag = draw.spawn(None, SIGN);
        draw.show(tag, false);
        Self {
            signs: HashMap::new(),
            tag,
            tagged: None,
            stale: false,
        }
    }

    /// Puts up a sign over each thing of `signs`, and takes down the
    /// others.
    pub fn put_up(
        &mut self,
        signs: Vec<(Thing, Words)>,
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
                    draw.commands
                        .entity(entity)
                        .insert(SceneLabel(words.says.clone()));
                    draw.show(entity, false);
                    Sign {
                        entity,
                        words,
                        near: false,
                    }
                }
            };
            kept.insert(thing, sign);
        }
        for sign in self.signs.values() {
            draw.despawn(sign.entity);
        }
        self.signs = kept;
    }

    /// Shows the signs within `far` of `eye`, and the tag over
    /// `tagged`, if anything is.
    pub fn show(
        &mut self,
        eye: Vec3,
        far: f32,
        tagged: Option<Words>,
        draw: &mut Draw,
    ) {
        for sign in self.signs.values_mut() {
            let near = eye.distance(sign.words.at) < far;
            if near != sign.near {
                draw.show(sign.entity, near);
                sign.near = near;
            }
        }
        if tagged == self.tagged {
            return;
        }
        match &tagged {
            Some(words) => {
                draw.place(self.tag, Frame::at(words.at));
                if self.tagged.as_ref().map(|was| &was.says)
                    != Some(&words.says)
                {
                    draw.commands
                        .entity(self.tag)
                        .insert(SceneLabel(words.says.clone()));
                }
                if self.tagged.is_none() {
                    draw.show(self.tag, true);
                }
            }
            None => draw.show(self.tag, false),
        }
        self.tagged = tagged;
    }
}
