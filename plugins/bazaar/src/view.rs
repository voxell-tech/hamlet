//! The bazaar's panel: a tab to each stall over what the stall shown
//! sells. Each stall is drawn as one [`shelf`](crate::view::shelf) of
//! its listings, with a page of its own for one of them, out of the
//! [`parts`](crate::view::parts) Hamlet's own panels are made of.

mod desk;
mod diorama;
mod hamlets;
mod listings;
mod models;
mod parts;
mod shelf;
mod skills;
mod trade;
mod wares;
mod words;

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Space, UiExt,
    ViewExt, column, icon_button, keyed, resource, row,
};

use crate::listings::{Agents, Hamlets, Tools};
use crate::market::lot::Kind;
use crate::models::Ware;
use crate::samples::agents::Hireling;
use crate::samples::hamlets::Template;
use crate::samples::plugins::Contraption;
use crate::skills::skills_sh::Scroll;
pub(crate) use crate::view::desk::plugin;
pub(crate) use crate::view::shelf::Shown;
use crate::{BazaarWorldExt, Stall, StallShown};

/// The tabs, a line each: what the agents use, then who works.
const TABS: [&[Stall]; 2] = [
    &[Stall::Models, Stall::Skills, Stall::Plugins],
    &[Stall::Community, Stall::Hamlets],
];

impl Stall {
    /// The icon and the words of its tab.
    fn tab(self) -> (&'static str, &'static str) {
        match self {
            Stall::Models => ("chip", "MODELS"),
            Stall::Community => ("robot", "AGENTS"),
            Stall::Plugins => ("bolt", "PLUGINS"),
            Stall::Skills => ("book", "SKILLS"),
            Stall::Hamlets => ("house", "HAMLETS"),
        }
    }
}

/// The bazaar's panel. What the stall shown sells is asked for as it
/// is first shown.
pub(crate) fn bazaar(world: &mut World) -> AnyView {
    let stall = world.resource::<StallShown>().0;
    world.stocked(stall);
    column((
        tabs(),
        keyed(resource::<StallShown, _>(|shown| shown.0), |stall| {
            wares(*stall)
        }),
    ))
    .space(Space::Double)
    .boxed()
}

/// A tab to each stall, the one shown lit, on lines short enough
/// for a narrow panel.
fn tabs() -> AnyView {
    keyed(resource::<StallShown, _>(|shown| shown.0), |now| {
        let line = |stalls: &[Stall]| {
            let tabs = stalls
                .iter()
                .map(|&stall| {
                    let (which, words) = stall.tab();
                    icon_button(which, words)
                        .style(if stall == *now {
                            ButtonStyle::Primary
                        } else {
                            ButtonStyle::Quiet
                        })
                        .on_click(move |world| world.show(stall))
                        .boxed()
                })
                .collect::<Vec<_>>();
            row(tabs).space(Space::Half).boxed()
        };
        column(TABS.into_iter().map(line).collect::<Vec<_>>())
            .space(Space::Half)
            .boxed()
    })
    .boxed()
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
    }
}
