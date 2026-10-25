//! The looks agents and hamlets wear (docs/looks.md): how an asset
//! is listed, and what the file of each kind holds.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The format of the skins and emotes read.
pub const FORMAT: u32 = 1;

/// What an asset is worn by.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Agent,
    Hamlet,
}

impl Target {
    pub const ALL: [Target; 2] = [Target::Agent, Target::Hamlet];

    pub fn name(self) -> &'static str {
        match self {
            Target::Agent => "Agents",
            Target::Hamlet => "Hamlets",
        }
    }
}

/// What kind of asset it is.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// A motion: an agent's gesture, or what moves about a hamlet's
    /// houses.
    Emote,
    /// Colours and a finish.
    Skin,
    /// A whole other model.
    Mesh,
}

impl Category {
    pub const ALL: [Category; 3] =
        [Category::Emote, Category::Skin, Category::Mesh];

    pub fn name(self) -> &'static str {
        match self {
            Category::Emote => "Emotes",
            Category::Skin => "Skins",
            Category::Mesh => "Meshes",
        }
    }

    /// Its name for one asset.
    pub fn one(self) -> &'static str {
        match self {
            Category::Emote => "Emote",
            Category::Skin => "Skin",
            Category::Mesh => "Mesh",
        }
    }
}

/// An asset as the bazaar's server lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// A stable slug, such as `agent-skin-sakura`.
    pub id: String,
    pub target: Target,
    pub category: Category,
    pub title: String,
    pub summary: String,
    pub price_cents: u32,
    pub version: String,
    #[serde(default)]
    pub owned: bool,
}

/// A colour as `"#rrggbb"`, in sRGB.
pub type Hex = String;

/// A skin: colours of the world's palette to wear in place of its
/// own, and for an agent, the finish of its paint.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub struct Skin {
    pub format: u32,
    /// Colours by the name of the palette's field they replace, such
    /// as `plaster` or `roof`. [`AGENT_SLOTS`] and [`HAMLET_SLOTS`]
    /// are those read.
    pub colours: BTreeMap<String, Hex>,
    pub finish: Option<Finish>,
}

/// How an agent's paint takes the light.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Finish {
    /// 0 for paint, 1 for bare metal.
    pub metallic: f32,
    /// 0 for a mirror, 1 for chalk.
    pub roughness: f32,
    /// How much the paint glows of its own colour, 0 for none.
    pub glow: f32,
}

impl Default for Finish {
    fn default() -> Self {
        Finish {
            metallic: 0.0,
            roughness: 0.6,
            glow: 0.0,
        }
    }
}

/// The colours an agent's skin may set: its paint, the brass of its
/// fittings, the steel of its joints, the glass of its face and the
/// light of its lamp.
pub const AGENT_SLOTS: [&str; 5] =
    ["paint", "brass", "steel", "glass", "lamp"];

/// The colours a hamlet's skin may set: the palette's fields its
/// houses are built of, the roof, roof drift and banner of its
/// town, and `livery` for every door and shutter.
pub const HAMLET_SLOTS: [&str; 15] = [
    "plaster",
    "plaster_worn",
    "whitewash",
    "ochre",
    "limestone",
    "limestone_shade",
    "timber",
    "timber_dark",
    "terracotta",
    "slate_roof",
    "moss",
    "roof",
    "drift",
    "banner",
    "livery",
];

impl Skin {
    /// Reads a skin, refusing colours it does not know for `target`.
    pub fn read(bytes: &[u8], target: Target) -> Result<Skin> {
        let skin = serde_json::from_slice::<Skin>(bytes)
            .context("a skin is not as the format says")?;
        if skin.format != FORMAT {
            bail!("a skin of format {} is not known", skin.format);
        }
        let slots: &[&str] = match target {
            Target::Agent => &AGENT_SLOTS,
            Target::Hamlet => &HAMLET_SLOTS,
        };
        for (slot, colour) in &skin.colours {
            if !slots.contains(&slot.as_str()) {
                bail!("a skin cannot set `{slot}`");
            }
            rgb(colour)?;
        }
        Ok(skin)
    }

    /// The colour set for `slot`, in sRGB from 0 to 1.
    pub fn colour(&self, slot: &str) -> Option<[f32; 3]> {
        self.colours.get(slot).and_then(|hex| rgb(hex).ok())
    }
}

/// `"#rrggbb"` in sRGB from 0 to 1.
pub fn rgb(hex: &str) -> Result<[f32; 3]> {
    let digits = hex
        .strip_prefix('#')
        .filter(|d| d.len() == 6 && d.is_ascii())
        .with_context(|| {
            format!("`{hex}` is not a colour as #rrggbb")
        })?;
    let part = |at: usize| {
        u8::from_str_radix(&digits[at..at + 2], 16)
            .map(|v| f32::from(v) / 255.0)
            .with_context(|| format!("`{hex}` is not a colour"))
    };
    Ok([part(0)?, part(2)?, part(4)?])
}

/// An emote, by what wears it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Emote {
    /// A gesture of an agent's parts over time.
    Clip(Clip),
    /// Smoke from every chimney of a hamlet.
    Smoke(Smoke),
    /// Paper lanterns swaying under every eave of a hamlet.
    Lanterns(Lanterns),
    /// Banners fluttering from every roof of a hamlet.
    Banners(Banners),
    /// Petals drifting down about every house of a hamlet.
    Petals(Petals),
    /// Fireflies about every house of a hamlet, glowing after dark.
    Fireflies(Fireflies),
    /// Pinwheels turning in the wind along every eave of a hamlet.
    Pinwheels(Pinwheels),
}

/// The parts of an agent a clip may move.
pub const PARTS: [&str; 8] = [
    "rig", "body", "head", "hat", "arm_l", "arm_r", "leg_l", "leg_r",
];

/// An agent's gesture: poses of its parts at times, eased between,
/// added to whatever it is doing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Its length.
    pub seconds: f32,
    /// The seconds between plays while the agent is idle, at the
    /// least.
    pub every: f32,
    /// The poses, by time, the first at 0 and the last at `seconds`.
    pub keys: Vec<Key>,
}

/// The pose of a clip at `at` seconds, by part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub at: f32,
    pub parts: BTreeMap<String, Move>,
}

/// How far one part is turned and moved from where it would be.
#[derive(
    Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub struct Move {
    /// About X, Y and Z in turn, in degrees.
    pub turn: [f32; 3],
    /// Along X, Y and Z, in the part's own units.
    pub shift: [f32; 3],
}

impl Clip {
    /// Each part's move at `t` seconds into the clip, eased between
    /// its poses. Parts no pose names are left out.
    pub fn at(&self, t: f32) -> BTreeMap<&str, Move> {
        let t = t.clamp(0.0, self.seconds);
        let after = self
            .keys
            .iter()
            .position(|key| key.at > t)
            .unwrap_or(self.keys.len());
        let (from, to) = match after {
            0 => (&self.keys[0], &self.keys[0]),
            n if n == self.keys.len() => {
                (&self.keys[n - 1], &self.keys[n - 1])
            }
            n => (&self.keys[n - 1], &self.keys[n]),
        };
        let span = to.at - from.at;
        let u = match span > f32::EPSILON {
            true => (t - from.at) / span,
            false => 0.0,
        };
        // Smoothstep: each pose is held a moment, not passed through.
        let u = u * u * (3.0 - 2.0 * u);
        let mut parts = BTreeMap::new();
        for name in PARTS {
            let (a, b) = (from.parts.get(name), to.parts.get(name));
            if a.is_none() && b.is_none() {
                continue;
            }
            let (a, b) = (
                a.copied().unwrap_or_default(),
                b.copied().unwrap_or_default(),
            );
            let mix = |x: [f32; 3], y: [f32; 3]| {
                [0, 1, 2].map(|i| x[i] + (y[i] - x[i]) * u)
            };
            parts.insert(
                name,
                Move {
                    turn: mix(a.turn, b.turn),
                    shift: mix(a.shift, b.shift),
                },
            );
        }
        parts
    }
}

/// Smoke rising from a hamlet's chimneys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Smoke {
    pub colour: Hex,
    /// Puffs a second from each chimney.
    pub rate: f32,
    /// A puff's size as it leaves.
    pub size: f32,
    /// How high a puff rises before it is gone.
    pub rise: f32,
}

/// Paper lanterns swaying under a hamlet's eaves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lanterns {
    /// Their paper and the light inside them.
    pub paper: Hex,
    pub light: Hex,
    /// How many hang from each house.
    pub count: u32,
    /// How far they swing, in degrees.
    pub sway: f32,
}

/// Banners fluttering from a hamlet's roofs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Banners {
    /// The cloth and its stripe.
    pub colours: [Hex; 2],
    /// Its length.
    pub long: f32,
}

/// Petals drifting down about a hamlet's houses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Petals {
    /// A petal's two colours, half the petals in each.
    pub colours: [Hex; 2],
    /// Petals falling about each house at once.
    pub count: u32,
    /// A petal's size.
    pub size: f32,
    /// Seconds a petal takes to fall from the eaves to the ground.
    pub fall: f32,
}

/// Fireflies about a hamlet's houses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fireflies {
    pub colour: Hex,
    /// How many fly about each house.
    pub count: u32,
    /// How brightly they glow after dark.
    pub glow: f32,
}

/// Pinwheels turning along a hamlet's eaves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pinwheels {
    /// The colours of their sails, in turn.
    pub colours: [Hex; 2],
    /// How many turn on each house.
    pub count: u32,
    /// Turns a second.
    pub speed: f32,
}

/// An emote as it is stored: its format beside it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmoteFile {
    pub format: u32,
    #[serde(flatten)]
    pub emote: Emote,
}

impl Emote {
    /// Reads an emote, refusing one not worn by `target` or with
    /// poses that do not make sense.
    pub fn read(bytes: &[u8], target: Target) -> Result<Emote> {
        let file = serde_json::from_slice::<EmoteFile>(bytes)
            .context("an emote is not as the format says")?;
        if file.format != FORMAT {
            bail!("an emote of format {} is not known", file.format);
        }
        let colours = match (&file.emote, target) {
            (Emote::Clip(clip), Target::Agent) => {
                if clip.keys.is_empty() || clip.seconds <= 0.0 {
                    bail!("a clip needs poses and a length");
                }
                if clip.keys.windows(2).any(|w| w[0].at > w[1].at) {
                    bail!("a clip's poses are out of order");
                }
                if let Some(part) = clip
                    .keys
                    .iter()
                    .flat_map(|key| key.parts.keys())
                    .find(|part| !PARTS.contains(&part.as_str()))
                {
                    bail!("a clip cannot move `{part}`");
                }
                vec![]
            }
            (Emote::Smoke(smoke), Target::Hamlet) => {
                vec![&smoke.colour]
            }
            (Emote::Lanterns(lanterns), Target::Hamlet) => {
                vec![&lanterns.paper, &lanterns.light]
            }
            (
                Emote::Banners(Banners { colours, .. }),
                Target::Hamlet,
            )
            | (
                Emote::Petals(Petals { colours, .. }),
                Target::Hamlet,
            )
            | (
                Emote::Pinwheels(Pinwheels { colours, .. }),
                Target::Hamlet,
            ) => colours.iter().collect(),
            (Emote::Fireflies(fireflies), Target::Hamlet) => {
                vec![&fireflies.colour]
            }
            _ => bail!("that emote is not worn by that"),
        };
        for colour in colours {
            rgb(colour)?;
        }
        Ok(file.emote)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skin_sets_only_the_colours_its_wearer_has() {
        let skin = br##"{"format":1,"colours":{"paint":"#ff8000"},
            "finish":{"metallic":1.0,"roughness":0.3}}"##;
        let read = Skin::read(skin, Target::Agent).unwrap();
        assert_eq!(
            read.colour("paint"),
            Some([1.0, 128.0 / 255.0, 0.0])
        );
        assert_eq!(read.finish.unwrap().glow, 0.0);
        assert!(Skin::read(skin, Target::Hamlet).is_err());
        let bad = br##"{"format":1,"colours":{"roof":"red"}}"##;
        assert!(Skin::read(bad, Target::Hamlet).is_err());
    }

    #[test]
    fn a_clip_eases_between_its_poses_and_holds_its_ends() {
        let clip =
            br#"{"format":1,"kind":"clip","seconds":2.0,"every":30,
            "keys":[{"at":0,"parts":{}},
                    {"at":1,"parts":{"arm_r":{"turn":[0,0,90]}}},
                    {"at":2,"parts":{}}]}"#;
        let Emote::Clip(clip) =
            Emote::read(clip, Target::Agent).unwrap()
        else {
            panic!("a clip");
        };
        assert_eq!(clip.at(1.0)["arm_r"].turn, [0.0, 0.0, 90.0]);
        // Halfway there, eased: exactly half by symmetry.
        assert_eq!(clip.at(0.5)["arm_r"].turn, [0.0, 0.0, 45.0]);
        // A quarter of the way, eased, is less than a quarter.
        assert!(clip.at(0.25)["arm_r"].turn[2] < 22.5);
        assert!(!clip.at(5.0).contains_key("arm_r"));
        assert!(!clip.at(1.0).contains_key("head"));
        // A clip is an agent's only.
        assert!(Emote::read(br#"{"format":1,"kind":"clip","seconds":1,"every":1,"keys":[{"at":0,"parts":{}}]}"#, Target::Hamlet).is_err());
        let smoke =
            br##"{"format":1,"kind":"smoke","colour":"#dddddd",
            "rate":4,"size":0.3,"rise":1.5}"##;
        assert!(matches!(
            Emote::read(smoke, Target::Hamlet),
            Ok(Emote::Smoke(_))
        ));
    }
}
