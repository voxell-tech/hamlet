//! The bazaar's panel: a tab to each stall over what the stall shown
//! sells. Each stall is drawn as one [`shelf`](crate::view::shelf) of
//! its listings, with a page of its own for one of them, out of the
//! pieces of [`style`](crate::view::style).

mod hamlets;
mod listings;
mod models;
mod shelf;
mod skills;
mod style;
mod words;

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Space, UiExt,
    ViewExt, column, icon_button, keyed, resource, row,
};

use crate::models::Ware;
use crate::samples::agents::Hireling;
use crate::samples::hamlets::Template;
use crate::samples::plugins::Contraption;
use crate::skills::skills_sh::Scroll;
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

/// The shelf of `stall`.
fn wares(stall: Stall) -> AnyView {
    match stall {
        Stall::Models => shelf::stall::<Ware>(),
        Stall::Community => shelf::stall::<Hireling>(),
        Stall::Plugins => shelf::stall::<Contraption>(),
        Stall::Skills => shelf::stall::<Scroll>(),
        Stall::Hamlets => shelf::stall::<Template>(),
    }
}
