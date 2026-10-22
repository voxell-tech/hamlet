//! The colours the world is made of. Their values are in
//! `assets/palette.toml`.

use glam::Vec3;

use crate::world::tint;

/// The roof colours a town's houses share, and its banner.
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TownStyle {
    #[serde(with = "tint::linear")]
    pub roof: Vec3,
    #[serde(with = "tint::linear")]
    pub drift: Vec3,
    #[serde(with = "tint::linear")]
    pub banner: Vec3,
}

/// The colours of one of the bazaar's stalls.
#[derive(
    Clone, Copy, PartialEq, Debug, Default, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct StallStyle {
    /// Its cloth, striped with `whitewash`, and the face of its
    /// sign.
    #[serde(with = "tint::linear")]
    pub cloth: Vec3,
    /// What its sign and its edges are picked out in.
    #[serde(with = "tint::linear")]
    pub trim: Vec3,
    /// The light of what it sells.
    #[serde(with = "tint::linear")]
    pub glow: Vec3,
}

/// The bazaar's own colours, which `plugins/realm/assets/bazaar.toml`
/// keeps beside the realm's palette.
#[derive(Clone, PartialEq, Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BazaarColours {
    /// The cloth of its awnings, each striped with `whitewash`.
    #[serde(with = "tint::linear_list")]
    pub awnings: [Vec3; 4],
    /// The bulbs of its fairy lights, in their turn along a string.
    #[serde(with = "tint::linear_list")]
    pub fairy: [Vec3; 6],
    /// Its stalls, in the order of `bazaar::Stall::ALL`.
    pub stalls: [StallStyle; 6],
}

/// Every colour of the world. Those baked into meshes are linear
/// RGB.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    #[serde(with = "tint::linear")]
    pub limestone: Vec3,
    #[serde(with = "tint::linear")]
    pub limestone_shade: Vec3,
    #[serde(with = "tint::linear")]
    pub plaster: Vec3,
    #[serde(with = "tint::linear")]
    pub plaster_worn: Vec3,
    #[serde(with = "tint::linear")]
    pub whitewash: Vec3,
    #[serde(with = "tint::linear")]
    pub ochre: Vec3,
    #[serde(with = "tint::linear")]
    pub timber: Vec3,
    #[serde(with = "tint::linear")]
    pub timber_dark: Vec3,
    /// Cut logs and bare planks.
    #[serde(with = "tint::linear")]
    pub log: Vec3,
    #[serde(with = "tint::linear")]
    pub terracotta: Vec3,
    #[serde(with = "tint::linear")]
    pub tile_drift: Vec3,
    #[serde(with = "tint::linear")]
    pub moss: Vec3,
    #[serde(with = "tint::linear")]
    pub slate_roof: Vec3,
    #[serde(with = "tint::linear")]
    pub dirt: Vec3,
    #[serde(with = "tint::linear")]
    pub dirt_shade: Vec3,
    /// Wet ground at the waterline.
    #[serde(with = "tint::linear")]
    pub mud: Vec3,
    #[serde(with = "tint::linear")]
    pub gravel: Vec3,
    #[serde(with = "tint::linear")]
    pub grass: Vec3,
    #[serde(with = "tint::linear")]
    pub grass_shade: Vec3,
    #[serde(with = "tint::linear")]
    pub grass_sun: Vec3,
    #[serde(with = "tint::linear")]
    pub grass_tip: Vec3,
    /// The thin grass of high ground.
    #[serde(with = "tint::linear")]
    pub grass_high: Vec3,
    #[serde(with = "tint::linear")]
    pub foliage: Vec3,
    #[serde(with = "tint::linear")]
    pub foliage_shade: Vec3,
    #[serde(with = "tint::linear")]
    pub autumn: Vec3,
    #[serde(with = "tint::linear")]
    pub autumn_shade: Vec3,
    #[serde(with = "tint::linear")]
    pub conifer: Vec3,
    #[serde(with = "tint::linear")]
    pub conifer_snow: Vec3,
    #[serde(with = "tint::linear")]
    pub bark: Vec3,
    #[serde(with = "tint::linear")]
    pub rock: Vec3,
    #[serde(with = "tint::linear")]
    pub rock_dark: Vec3,
    #[serde(with = "tint::linear")]
    pub rock_warm: Vec3,
    #[serde(with = "tint::linear")]
    pub snow: Vec3,
    #[serde(with = "tint::linear")]
    pub water: Vec3,
    #[serde(with = "tint::linear")]
    pub water_shade: Vec3,
    /// The slate bed of a groove, and the pewter strip down its
    /// middle.
    #[serde(with = "tint::linear")]
    pub bed: Vec3,
    /// The light of the hamlet's magic, and of an ember.
    #[serde(with = "tint::linear")]
    pub rune: Vec3,
    #[serde(with = "tint::linear")]
    pub ember: Vec3,
    #[serde(with = "tint::linear")]
    pub firefly: Vec3,
    /// The lines the wind draws on the air.
    #[serde(with = "tint::linear")]
    pub wind: Vec3,
    #[serde(with = "tint::linear")]
    pub pewter: Vec3,
    #[serde(with = "tint::linear")]
    pub brass: Vec3,
    #[serde(with = "tint::linear")]
    pub steel: Vec3,
    #[serde(with = "tint::linear")]
    pub steel_bright: Vec3,
    #[serde(with = "tint::linear")]
    pub parchment: Vec3,
    #[serde(with = "tint::linear")]
    pub ink: Vec3,
    #[serde(with = "tint::linear")]
    pub wax: Vec3,
    /// The cream cloth only a mayor wears.
    #[serde(with = "tint::linear")]
    pub mantle: Vec3,
    /// The worn top of a stool.
    #[serde(with = "tint::linear")]
    pub seat: Vec3,
    /// A book's cover and its pages.
    #[serde(with = "tint::linear")]
    pub leather: Vec3,
    #[serde(with = "tint::linear")]
    pub page: Vec3,
    /// The second colour of a kite.
    #[serde(with = "tint::linear")]
    pub kite: Vec3,
    #[serde(with = "tint::linear_list")]
    pub flowers: [Vec3; 5],
    /// One per town, by site.
    pub towns: [TownStyle; 8],
    /// Paints for doors and shutters: an agent's own colour.
    #[serde(with = "tint::linear_list")]
    pub livery: [Vec3; 8],
    /// A robot's body paint, in the order of `keeper::Paint::ALL`.
    #[serde(with = "tint::linear_list")]
    pub paints: [Vec3; 7],
    /// A chief's cloak, in the order of `chief::Cloak::ALL`.
    #[serde(with = "tint::linear_list")]
    pub cloaks: [Vec3; 8],
    /// The light of a budget's meter by how much is left, from
    /// nothing to all of it: red, orange, yellow, green.
    #[serde(with = "tint::linear_list")]
    pub meter: [Vec3; 4],
    /// A chief's own light, in the order of `chief::Light::ALL`.
    #[serde(with = "tint::linear_list")]
    pub wicks: [Vec3; 5],
    /// A chief's face, its cheeks and its hair.
    #[serde(with = "tint::linear")]
    pub face: Vec3,
    #[serde(with = "tint::linear")]
    pub blush: Vec3,
    #[serde(with = "tint::linear")]
    pub hair: Vec3,
    /// A window pane by day and after dark.
    #[serde(with = "tint")]
    pub pane_dark: Vec3,
    #[serde(with = "tint")]
    pub pane_lit: Vec3,
    /// The glass of a lantern that is out, and of one that burns.
    #[serde(with = "tint")]
    pub glass_dark: Vec3,
    #[serde(with = "tint")]
    pub glass_lit: Vec3,
    #[serde(with = "tint")]
    pub river: Vec3,
    /// The footprint of a place in a chief's hand, where it could
    /// be set down and where it could not.
    #[serde(with = "tint::linear")]
    pub place_fits: Vec3,
    #[serde(with = "tint::linear")]
    pub place_unfit: Vec3,
    #[serde(skip)]
    pub bazaar: BazaarColours,
}
