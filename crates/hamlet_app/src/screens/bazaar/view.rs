//! The bazaar's panel: a tab to each stall over what the stall shown
//! sells. Each stall is drawn as one [`shelf`](self::shelf) of its
//! listings, with a page of its own for one of them, out of the
//! [`parts`](self::parts) Hamlet's own panels are made of.

mod covers;
mod desk;
mod diorama;
mod hamlets;
mod listings;
mod models;
mod parts;
mod shelf;
mod skills;
mod trade;
mod wardrobe;
mod wares;
mod words;

use bevy::app::{App, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::world::World;
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_realm::diorama::Dioramas;
use hamlet_realm::wardrobe::Asset;
use hamlet_ui::styles::{Fills, filling};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, Cx};

use self::desk::Desk;
pub(crate) use self::shelf::Shown;
use crate::bazaar::listings::{Agents, Hamlets, Tools, Wares};
use crate::bazaar::market::lot::Kind;
use crate::bazaar::models::Ware;
use crate::bazaar::samples::agents::Hireling;
use crate::bazaar::samples::hamlets::Template;
use crate::bazaar::samples::plugins::Contraption;
use crate::bazaar::skills::skills_sh::Scroll;
use crate::bazaar::{Stall, StallShown, show, stocked};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        shelf::typing::<Ware>,
        shelf::typing::<Hireling>,
        shelf::typing::<Contraption>,
        shelf::typing::<Scroll>,
        shelf::typing::<Template>,
        shelf::typing::<Wares<Agents>>,
        shelf::typing::<Wares<Tools>>,
        shelf::typing::<Wares<Hamlets>>,
        shelf::typing::<Asset>,
    ))
    .init_resource::<Desk>()
    .add_systems(
        Update,
        (
            tabs::mark::<StallShown>,
            diorama::turn.run_if(resource_exists::<Dioramas>),
        ),
    );
}

/// The tabs of the stalls, a line each: what the agents use, then
/// who works and what they wear.
const TABS: [&[(StallShown, Glyph, &str)]; 2] = [
    &[
        (StallShown(Stall::Models), Glyph::Chip, "MODELS"),
        (StallShown(Stall::Skills), Glyph::Book, "SKILLS"),
        (StallShown(Stall::Plugins), Glyph::Bolt, "PLUGINS"),
    ],
    &[
        (StallShown(Stall::Community), Glyph::Robot, "AGENTS"),
        (StallShown(Stall::Hamlets), Glyph::Hamlet, "HAMLETS"),
        (StallShown(Stall::Assets), Glyph::Mayor, "LOOKS"),
    ],
];

/// The bazaar's panel. What the stall shown sells is asked for as it
/// is first shown.
pub(crate) fn screen() -> AnyView {
    let tabs = column(
        TABS.into_iter()
            .map(|line| {
                tabs::strip(line, |world: &mut World, shown| {
                    // A tab leaves the seller's desk too.
                    world.resource_mut::<Desk>().page = None;
                    show(world, shown.0);
                })
                .boxed()
            })
            .collect::<Vec<_>>(),
    )
    .gap(4.0);
    AnyView::new(move |cx: &mut Cx| {
        let stall = cx.world.resource::<StallShown>().0;
        stocked(cx.world, stall);
        let wares = keyed(
            resource::<StallShown, _>(|shown| shown.0),
            |stall| wares(*stall),
        )
        .within(filling());
        cx.build(column((tabs, wares)).gap(16.0).fills_height())
    })
}

/// Opens the page of what the chief sells and has bought.
pub(crate) fn yours(world: &mut World) {
    desk::open_yours(world);
}

/// Opens the seller's desk at `stall`, to sell what it sells.
pub(crate) fn sell(world: &mut World, stall: Stall) {
    let kind = match stall {
        Stall::Plugins => Kind::Plugin,
        Stall::Hamlets => Kind::Hamlet,
        _ => Kind::Agent,
    };
    desk::open_form(world, kind, None);
}

/// The shelf of `stall`.
fn wares(stall: Stall) -> AnyView {
    match stall {
        Stall::Models => shelf::stall::<Ware>(),
        Stall::Community => trade::stall::<Agents, Hireling>(),
        Stall::Plugins => trade::stall::<Tools, Contraption>(),
        Stall::Skills => shelf::stall::<Scroll>(),
        Stall::Hamlets => trade::stall::<Hamlets, Template>(),
        Stall::Assets => wardrobe::stall(),
    }
    .boxed()
}
