//! The realm's effects and overlays, each played against the realm's
//! clock: sparks, the wind, the small magic and the debug overlay.

pub mod debug;
pub mod magic;
pub mod motes;
pub mod sparks;
pub mod wind;

use hamlet_engine_bevy::prelude::*;

use crate::build::upload::Draw;
use crate::fx::motes::Motes;
use crate::tuning::VfxTuning;
use crate::world::land::Land;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        debug::plugin,
        sparks::plugin,
        wind::plugin,
        magic::plugin,
    ))
    .add_systems(
        Update,
        make.run_if(ticked).run_if(not(resource_exists::<Motes>)),
    );
}

/// The effects' tuning and the motes they are drawn with, made on the
/// realm's first frame. They are not tried again once they failed.
fn make(mut tried: Local<bool>, mut draw: Draw) {
    if std::mem::replace(&mut *tried, true) {
        return;
    }
    let made = VfxTuning::read().and_then(|tuning| {
        let motes = Motes::make(&mut draw)?;
        Ok((tuning, motes))
    });
    match made {
        Ok((tuning, motes)) => {
            draw.commands.insert_resource(tuning);
            draw.commands.insert_resource(motes);
        }
        Err(why) => hamlet_engine_bevy::error(&format!(
            "the realm's effects: {why}"
        )),
    }
}

/// True on a frame of the realm: a call that moves time on, once the
/// realm is made.
fn ticked(time: Res<Time>, land: Option<Res<Land>>) -> bool {
    !time.delta().is_zero() && land.is_some()
}

/// True on a frame of the realm once its effects are made.
fn ready(
    time: Res<Time>,
    land: Option<Res<Land>>,
    motes: Option<Res<Motes>>,
) -> bool {
    !time.delta().is_zero() && land.is_some() && motes.is_some()
}

/// A number from 0 to 1 that depends only on `n`.
fn chance(n: f32) -> f32 {
    (n * 12.9898 + 4.1414)
        .sin()
        .mul_add(43_758.547, 0.0)
        .fract()
        .abs()
}
