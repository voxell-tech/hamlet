//! The names shown over things: a sign over each house, common and
//! the Hearth while the camera is near enough to read it, and a tag
//! over whatever else the pointer is on.

use std::collections::HashMap;

use glam::Vec3;
use hamlet_engine_sdk::scene::Entity;

use crate::frame::Frame;
use crate::pick::Thing;

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
pub struct Signs {
    signs: HashMap<Thing, Sign>,
    tag: Entity,
    tagged: Option<Words>,
}

impl Signs {
    pub fn make() -> Result<Self, String> {
        let tag = Entity::make(None, SIGN)?;
        tag.set_visible(false);
        Ok(Self {
            signs: HashMap::new(),
            tag,
            tagged: None,
        })
    }

    /// Puts up a sign over each thing of `signs`, and takes down the
    /// others.
    pub fn put_up(&mut self, signs: Vec<(Thing, Words)>) {
        let mut kept = HashMap::new();
        for (thing, words) in signs {
            let sign = match self.signs.remove(&thing) {
                Some(mut sign) => {
                    if sign.words.at != words.at {
                        sign.entity.set_transform(
                            Frame::at(words.at).scene(),
                        );
                    }
                    if sign.words.says != words.says {
                        sign.entity.set_label(Some(&words.says));
                    }
                    sign.words = words;
                    sign
                }
                None => {
                    let Ok(entity) = Entity::make(None, SIGN) else {
                        continue;
                    };
                    entity.set_transform(Frame::at(words.at).scene());
                    entity.set_label(Some(&words.says));
                    entity.set_visible(false);
                    Sign {
                        entity,
                        words,
                        near: false,
                    }
                }
            };
            kept.insert(thing, sign);
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
    ) {
        for sign in self.signs.values_mut() {
            let near = eye.distance(sign.words.at) < far;
            if near != sign.near {
                sign.entity.set_visible(near);
                sign.near = near;
            }
        }
        if tagged == self.tagged {
            return;
        }
        match &tagged {
            Some(words) => {
                self.tag.set_transform(Frame::at(words.at).scene());
                if self.tagged.as_ref().map(|was| &was.says)
                    != Some(&words.says)
                {
                    self.tag.set_label(Some(&words.says));
                }
                if self.tagged.is_none() {
                    self.tag.set_visible(true);
                }
            }
            None => self.tag.set_visible(false),
        }
        self.tagged = tagged;
    }
}
