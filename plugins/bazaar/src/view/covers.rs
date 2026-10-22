//! Pictures drawn: a listing's cover, and the cover chosen at the
//! seller's desk.

use std::sync::atomic::AtomicU64;

use agents_ui::{either, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{AnyView, Fit, ViewExt, derived, image};

use crate::covers::{Covers, Pick, Picked, Reveal};
use crate::market::Market;
use crate::view::Desk;

/// A cover on its listing's page and at the desk.
pub(super) const PAGE: (f32, f32) = (320.0, 180.0);

/// The cover at `url` on the bazaar's server, once it came, filling
/// `extent`.
pub(super) fn cover(url: String, extent: (f32, f32)) -> AnyView {
    let shown = AtomicU64::new(0);
    let picture = derived(move |world: &World| {
        let covers = world.resource::<Covers>();
        world.resource::<Reveal>().show(covers.get(&url), &shown)
    })
    .when(either(moved::<Covers>(), moved::<Reveal>()));
    image(picture)
        .extent(extent)
        .fit(Fit::Cover)
        .rounded(true)
        .boxed()
}

/// The cover chosen at the desk, else that of the listing changed
/// there.
pub(super) fn chosen() -> AnyView {
    let shown = AtomicU64::new(0);
    let picture = derived(move |world: &World| {
        let covers = world.resource::<Covers>();
        let picture = match &world.resource::<Pick>().state {
            Picked::Got(picture) => Some(picture),
            Picked::Nothing => {
                now_of(world).and_then(|url| covers.get(&url))
            }
            Picked::Fetching | Picked::Failed(_) => None,
        };
        world.resource::<Reveal>().show(picture, &shown)
    })
    .when(either(
        either(moved::<Covers>(), moved::<Reveal>()),
        either(moved::<Pick>(), moved::<Desk>()),
    ));
    image(picture)
        .extent(PAGE)
        .fit(Fit::Cover)
        .rounded(true)
        .boxed()
}

/// The address of the cover of the listing changed at the desk, if it
/// has one.
pub(super) fn now_of(world: &World) -> Option<String> {
    let id = world.resource::<Desk>().form.id.clone()?;
    world.resource::<Market>().lot(&id)?.cover_url.clone()
}
