//! An asset's pictures on its page, under its head as a hamlet's
//! town is: the one shown of those turned evenly about it, turned by
//! hand or by itself, the ways to turn them quiet in a row under it.

use std::sync::atomic::AtomicU64;

use agents_ui::{either, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Fit, FrameProps, Space, UiExt,
    ViewExt, column, derived, icon_button, image, keyed, resource,
    row,
};
use looks::wardrobe::Asset;

use crate::covers::{Covers, Reveal};
use crate::view::parts::note;
use crate::wardrobe::turntable::Turning;

/// The room the pictures are shown in, in the interface's pixels: as
/// wide as a listing's cover on its page, and as tall as the pictures
/// are for that width.
const STAGE: (f32, f32) = (320.0, 240.0);

/// The pictures of `asset`, the one shown in its room, and the ways
/// to turn them.
pub(super) fn turntable(asset: Asset) -> AnyView {
    if asset.frames == 0 || asset.preview_url.is_empty() {
        return note("There are no pictures of it yet.").boxed();
    }
    let shown = AtomicU64::new(0);
    let read = asset.clone();
    let picture = derived(move |world: &World| {
        let covers = world.resource::<Covers>();
        let frame = world.resource::<Turning>().shown(&read);
        // The first stands in while the one turned to comes.
        let picture = covers
            .get(&read.frame(frame))
            .or_else(|| covers.get(&read.frame(0)));
        world.resource::<Reveal>().show(picture, &shown)
    })
    .when(either(
        either(moved::<Covers>(), moved::<Reveal>()),
        moved::<Turning>(),
    ));
    let frames = asset.frames;
    let read = asset.clone();
    let state = resource::<Turning, _>(move |turning| {
        (turning.shown(&read), turning.moving())
    });
    let controls = keyed(state, move |(frame, spinning)| {
        let turn = |which: &str, words: &str, by: i64| {
            let asset = asset.clone();
            icon_button(which.to_string(), words.to_string())
                .style(ButtonStyle::Quiet)
                .on_click(move |world| {
                    world.resource_mut::<Turning>().step(&asset, by)
                })
                .boxed()
        };
        let spin = {
            let asset = asset.clone();
            let (which, words) = match spinning {
                true => ("stop", "STOP"),
                false => ("play", "SPIN"),
            };
            let on = !*spinning;
            icon_button(which, words)
                .style(ButtonStyle::Quiet)
                .on_click(move |world| {
                    world.resource_mut::<Turning>().spin(&asset, on)
                })
                .boxed()
        };
        let mut all = Vec::new();
        if frames > 1 {
            all.extend([
                turn("back", "LEFT", -1),
                spin,
                turn("chevron_right", "RIGHT", 1),
            ]);
        }
        all.push(note(format!("{} of {frames}", frame + 1)).boxed());
        row(all).space(Space::Half).boxed()
    });
    column((
        image(picture).extent(STAGE).fit(Fit::Cover).rounded(true),
        controls,
    ))
    .space(Space::Half)
    .boxed()
}
