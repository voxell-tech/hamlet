//! The bazaar's lights: its lanterns, what glows among its wares, the
//! fairy lights and their glows, and the flare after a sale.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::window::RequestRedraw;

use super::build::FAIRY;
use super::{BazaarSettings, Lod, Merchant};
use crate::{Clock, Sky};

/// Seconds the lights are still tended after a sale is cheered, so
/// that they come back to rest.
const SETTLE: f32 = 0.5;

/// The lit glass of the bazaar's lanterns and its light at rest, and
/// what glows among its wares.
#[derive(Component, Clone, Debug)]
pub(super) struct Lanterns {
    pub glass: Handle<StandardMaterial>,
    pub glow: LinearRgba,
    pub gleam: Handle<StandardMaterial>,
}

/// The bulbs of the fairy lights, a material to each colour, and
/// each colour at full glow.
#[derive(Component, Clone, Debug)]
pub(super) struct Fairies {
    pub bulbs: [Handle<StandardMaterial>; FAIRY],
    pub colours: [LinearRgba; FAIRY],
}

/// A light over the avenue that takes on the fairy lights' colours
/// in turn after dark; its number sets where in the turn it is.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Glow(pub usize);

/// The light under the pavilion that flares after a sale.
#[derive(Component)]
pub(super) struct Flare;

/// Brightness of the bazaar `since` seconds after a sale cheered for
/// `cheers` seconds: up at once, flickering while the merchant
/// cheers, and down again.
fn flare(since: f32, cheers: f32) -> f32 {
    if !(0.0..cheers).contains(&since) {
        return 0.0;
    }
    let up = (since / 0.2).min(1.0);
    let down = ((cheers - since) / 1.5).min(1.0);
    up * down * (0.75 + 0.25 * (since * 12.0).sin())
}

/// The flare of the latest sale now.
fn level(
    clock: &Clock,
    set: &BazaarSettings,
    merchants: &Query<&Merchant>,
) -> f32 {
    merchants
        .iter()
        .filter_map(|merchant| merchant.sold)
        .map(|at| flare(clock.t - at, set.cheers))
        .fold(0.0, f32::max)
}

/// True while a sale is cheered, and a moment after.
fn cheering(
    clock: &Clock,
    set: &BazaarSettings,
    merchants: &Query<&Merchant>,
) -> bool {
    merchants
        .iter()
        .filter_map(|merchant| merchant.sold)
        .any(|at| {
            (0.0..set.cheers + SETTLE).contains(&(clock.t - at))
        })
}

/// True when the lanterns' light may differ from what it was: the
/// sky or the settings changed, the bazaar is new or a sale is
/// cheered.
pub(super) fn lit(
    clock: Res<Clock>,
    sky: Res<Sky>,
    set: Res<BazaarSettings>,
    merchants: Query<&Merchant>,
    new: Query<(), Added<Lanterns>>,
) -> bool {
    sky.is_changed()
        || set.is_changed()
        || !new.is_empty()
        || cheering(&clock, &set, &merchants)
}

/// True when the fairy lights move or may differ from what they
/// were: after dark, while a sale is cheered, or when the sky, the
/// settings or the bazaar are new.
pub(super) fn astir(
    clock: Res<Clock>,
    sky: Res<Sky>,
    set: Res<BazaarSettings>,
    merchants: Query<&Merchant>,
    new: Query<(), Added<Fairies>>,
) -> bool {
    dark(&sky)
        || sky.is_changed()
        || set.is_changed()
        || !new.is_empty()
        || cheering(&clock, &set, &merchants)
}

/// True once it is dark enough for the fairy lights to chase.
fn dark(sky: &Sky) -> bool {
    sky.look.candle >= 0.05
}

/// Sets the emissive light of `material` to `to`, if it differs, so
/// that a material at rest is not marked changed.
fn burn(
    materials: &mut Assets<StandardMaterial>,
    material: &Handle<StandardMaterial>,
    to: LinearRgba,
) {
    let differs = materials
        .get(material)
        .is_some_and(|glass| glass.emissive != to);
    if differs && let Some(mut glass) = materials.get_mut(material) {
        glass.emissive = to;
    }
}

/// Burns the lanterns and what glows among the wares, brighter after
/// dark and brighter still after a sale, and the flare under the
/// pavilion.
pub(super) fn shine(
    clock: Res<Clock>,
    sky: Res<Sky>,
    set: Res<BazaarSettings>,
    merchants: Query<&Merchant>,
    lanterns: Query<&Lanterns>,
    mut flares: Query<&mut PointLight, With<Flare>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let level = level(&clock, &set, &merchants);
    let candle = sky.look.candle;
    for mut flare in &mut flares {
        let lumens = set.flare * level;
        if flare.intensity != lumens {
            flare.intensity = lumens;
        }
    }
    for lantern in &lanterns {
        let (day, night) = set.glass;
        let to =
            lantern.glow * (day + night * candle + set.flash * level);
        burn(&mut materials, &lantern.glass, to);
        let (day, night) = set.gleam;
        let bright = day + night * candle + level;
        let to = Color::linear_rgb(bright, bright, bright);
        let differs = materials
            .get(&lantern.gleam)
            .is_some_and(|gleam| gleam.base_color != to);
        if differs
            && let Some(mut gleam) = materials.get_mut(&lantern.gleam)
        {
            gleam.base_color = to;
        }
    }
    if level > 0.0 {
        redraw.write(RequestRedraw);
    }
}

/// The share of its glow the bulb of colour `k` of `FAIRY` has at
/// `t`, of a chase that runs along every string `rounds` times a
/// second: dim between the light passing and bright as it passes.
fn chased(k: usize, t: f32, rounds: f32) -> f32 {
    let wave = 0.5
        + 0.5 * (TAU * (t * rounds - k as f32 / FAIRY as f32)).sin();
    0.3 + 0.7 * wave * wave
}

/// Runs the fairy lights: steady by day, chasing along their strings
/// after dark and faster after a sale, with the glows over the avenue
/// taking on their colours in turn.
pub(super) fn twinkle(
    clock: Res<Clock>,
    sky: Res<Sky>,
    set: Res<BazaarSettings>,
    merchants: Query<&Merchant>,
    fairies: Query<(&Fairies, &ViewVisibility, &Lod)>,
    mut glows: Query<(&Glow, &mut PointLight)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let level = level(&clock, &set, &merchants);
    let night = sky.look.candle;
    // By day and with no sale the lights stand still, and ask for no
    // frames.
    let still = !dark(&sky) && level == 0.0;
    let (day, gain, sale) = set.bulb;
    let (rounds, faster) = set.chase;
    let rounds = rounds * (1.0 + faster * level);
    let mut seen = false;
    for (fairies, view, lod) in &fairies {
        seen |= view.get() && lod.close;
        for (k, bulb) in fairies.bulbs.iter().enumerate() {
            let chase = match still {
                true => 1.0,
                false => chased(k, clock.t, rounds),
            };
            let to = fairies.colours[k]
                * (day + (gain * night + sale * level) * chase);
            burn(&mut materials, bulb, to);
        }
        let lumens = set.glow * (night + level);
        for (glow, mut light) in &mut glows {
            let turn = clock.t / set.glow_holds + glow.0 as f32;
            let (from, share) = (turn.floor() as usize, turn.fract());
            let (a, b) = (
                fairies.colours[from % FAIRY],
                fairies.colours[(from + 1) % FAIRY],
            );
            let colour = Color::from(a * (1.0 - share) + b * share);
            if light.intensity != lumens {
                light.intensity = lumens;
            }
            if light.color != colour {
                light.color = colour;
            }
        }
    }
    if !still && seen {
        redraw.write(RequestRedraw);
    }
}
