//! An asset's pictures on its page, under its head as a hamlet's
//! town is: the one shown of those turned evenly about it, turned by
//! hand or by itself, the ways to turn them quiet in a row under it.

use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::ui::AlignItems;
use bevy_fynix::views::{FrameProps, column, row};
use bevy_fynix::{ViewExt, derived, keyed, resource};
use hamlet_realm::wardrobe::Asset;
use hamlet_ui::AnyView;
use hamlet_ui::styles::{bare, note};
use hamlet_ui::widgets::icons::Glyph;

use super::quiet;
use crate::bazaar::covers::{Covers, Held};
use crate::bazaar::view::covers::picture;
use crate::bazaar::wardrobe::turntable::Turning;
use crate::roll::{either, moved};

/// The room the pictures are shown in, in the interface's pixels: as
/// wide as a listing's cover on its page, and as tall as the pictures
/// are for that width.
const STAGE: Vec2 = Vec2::new(320.0, 240.0);

/// The pictures of `asset`, the one shown in its room, and the ways
/// to turn them.
pub(super) fn turntable(asset: Asset) -> AnyView {
    if asset.frames == 0 || asset.preview_url.is_empty() {
        return note("There are no pictures of it yet.").boxed();
    }
    let read = asset.clone();
    let shown = derived(move |world: &World| {
        let covers = world.resource::<Covers>();
        let frame = world.resource::<Turning>().shown(&read);
        // The first stands in while the one turned to comes.
        [read.frame(frame), read.frame(0)].into_iter().find_map(
            |url| match covers.get(&url) {
                Some(Held::Shown(drawn)) => Some(drawn.clone()),
                _ => None,
            },
        )
    })
    .when(either(moved::<Covers>(), moved::<Turning>()));
    let picture = keyed(shown, |drawn| match drawn {
        Some(drawn) => picture(drawn.clone(), STAGE),
        None => note("Fetching its pictures...").boxed(),
    })
    .within(bare());
    let frames = asset.frames;
    let read = asset.clone();
    let state = resource::<Turning, _>(move |turning| {
        (turning.shown(&read), turning.moving())
    });
    let controls = keyed(state, move |(frame, spinning)| {
        let turn = |which: Glyph, words: &'static str, by: i64| {
            let asset = asset.clone();
            quiet(which, words, move |world| {
                world.resource_mut::<Turning>().step(&asset, by);
            })
        };
        let spin = {
            let asset = asset.clone();
            let (which, words) = match spinning {
                true => (Glyph::Stop, "STOP"),
                false => (Glyph::Play, "SPIN"),
            };
            let on = !*spinning;
            quiet(which, words, move |world| {
                world.resource_mut::<Turning>().spin(&asset, on);
            })
        };
        let mut all = Vec::new();
        if frames > 1 {
            all.extend([
                turn(Glyph::Back, "LEFT", -1),
                spin,
                turn(Glyph::Into, "RIGHT", 1),
            ]);
        }
        all.push(note(format!("{} of {frames}", frame + 1)).boxed());
        row(all).gap(6.0).align(AlignItems::Center).boxed()
    })
    .within(bare());
    column((picture, controls)).gap(6.0).boxed()
}
