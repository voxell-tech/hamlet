//! The turntable of an asset's page: its preview pictures, turned
//! evenly about it, shown one after another as the plugin is ticked
//! while it turns, or one at a time as the chief turns it by hand.
//! Each picture is fetched as it is wanted, with the listings'
//! covers, and let go of once its page closes.

use hamlet_engine_bevy::prelude::*;
use looks::wardrobe::Asset;

use crate::covers::Covers;
use crate::shelf::{Shelf, Stock};
use crate::wardrobe::Fitting;
use crate::{Stall, StallShown};

/// The seconds each picture is shown while the turntable turns: 24
/// pictures go round in three seconds.
pub(crate) const STEP: f32 = 0.125;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Turning>()
        .add_systems(Update, (watch, turn).chain());
}

/// The turntable of the asset whose page is open.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Turning {
    /// The asset whose page it is, by id.
    pub id: Option<String>,
    /// The picture shown, of the asset's frames.
    pub frame: u32,
    /// It turns by itself.
    pub spinning: bool,
    /// Seconds since the picture shown was.
    since: f32,
}

impl Turning {
    /// It turns, and the plugin is called each frame for it.
    pub fn moving(&self) -> bool {
        self.id.is_some() && self.spinning
    }

    /// Turns the turntable of `asset` by `by` pictures, by hand.
    pub fn step(&mut self, asset: &Asset, by: i64) {
        let frames = i64::from(asset.frames.max(1));
        self.id = Some(asset.id.clone());
        self.spinning = false;
        self.frame =
            (i64::from(self.frame) + by).rem_euclid(frames) as u32;
    }

    /// Has the turntable of `asset` turn by itself, or stop.
    pub fn spin(&mut self, asset: &Asset, on: bool) {
        if self.id.as_deref() != Some(&asset.id) {
            self.frame = 0;
        }
        self.id = Some(asset.id.clone());
        self.spinning = on && asset.frames > 1;
        self.since = 0.0;
    }

    /// The picture of `asset` shown.
    pub fn shown(&self, asset: &Asset) -> u32 {
        match self.id.as_deref() == Some(&asset.id) {
            true => self.frame.min(asset.frames.saturating_sub(1)),
            false => 0,
        }
    }
}

/// The addresses of the pictures of `asset` wanted on its page: all
/// of them while its turntable turns, else the one shown and the
/// first.
pub(crate) fn frames(
    asset: &Asset,
    turning: &Turning,
) -> Vec<String> {
    if asset.frames == 0 || asset.preview_url.is_empty() {
        return Vec::new();
    }
    if turning.spinning && turning.id.as_deref() == Some(&asset.id) {
        return (0..asset.frames).map(|at| asset.frame(at)).collect();
    }
    let shown = turning.shown(asset);
    let mut wanted = vec![asset.frame(shown)];
    if shown != 0 {
        wanted.push(asset.frame(0));
    }
    wanted
}

/// The pictures of the Asset stall shown: the open asset's on its
/// page.
pub(crate) fn shown(world: &World) -> Vec<String> {
    open(world).map_or_else(Vec::new, |asset| {
        frames(&asset, world.resource::<Turning>())
    })
}

/// The asset whose page is shown, if one is.
fn open(world: &World) -> Option<Asset> {
    if world.resource::<StallShown>().0 != Stall::Assets
        || world.resource::<Fitting>().on
    {
        return None;
    }
    let id = world.resource::<Shelf<Asset>>().open.clone()?;
    world.resource::<Stock<Asset>>().get(&id).cloned()
}

/// Lets go of the turntable once its page is not shown.
fn watch(world: &mut World) {
    let Some(id) = world.resource::<Turning>().id.clone() else {
        return;
    };
    if open(world).is_none_or(|asset| asset.id != id) {
        *world.resource_mut::<Turning>() = Turning::default();
    }
}

/// Shows the next picture that came, while the turntable turns.
fn turn(world: &mut World) {
    if !world.resource::<Turning>().moving() {
        return;
    }
    let Some(asset) = open(world) else {
        return;
    };
    let delta = world.resource::<Time>().delta_secs();
    let covers = world.resource::<Covers>();
    let turning = world.resource::<Turning>();
    let mut since = turning.since + delta;
    let mut frame = turning.frame;
    while since >= STEP {
        since -= STEP;
        frame = next(&asset, frame, |url| covers.get(url).is_some());
    }
    let mut turning = world.resource_mut::<Turning>();
    // The time alone changes nothing shown.
    turning.bypass_change_detection().since = since;
    if turning.frame != frame {
        turning.frame = frame;
    }
}

/// The picture of `asset` after `frame` that `came`, or `frame` when
/// no other did.
pub(crate) fn next(
    asset: &Asset,
    frame: u32,
    came: impl Fn(&str) -> bool,
) -> u32 {
    let frames = asset.frames.max(1);
    (1..frames)
        .map(|by| (frame + by) % frames)
        .find(|at| came(&asset.frame(*at)))
        .unwrap_or(frame)
}
