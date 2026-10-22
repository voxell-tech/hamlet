//! The Hamlets stall drawn: each hamlet as a row, and its page with
//! who comes with it and the way to found it.

use agents_ui::{Rostered, either, moved, note};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, Fill, FrameProps, Size,
    Space, Tone, UiExt, ViewExt, column, derived, description, icon,
    icon_button, keyed, label, row,
};

use crate::hamlets::{self, Founding, Most, full, owns};
use crate::samples::hamlets::{Settler, Template};
use crate::samples::{Price, SELLERS_SHARE};
use crate::view::shelf::{Shown, part};
use crate::view::words::grouped;

impl Shown for Template {
    const BACK: &'static str = "HAMLETS";
    const NOUN: &'static str = "hamlets";
    const FIND: &'static str = "Search hamlets";

    fn heading() -> AnyView {
        let full = derived(full)
            .when(either(moved::<Rostered>(), moved::<Most>()));
        keyed(full, |full| match full {
            Some(why) => description(why.clone()).boxed(),
            None => column(()).boxed(),
        })
        .boxed()
    }

    fn title(&self) -> String {
        self.title.to_string()
    }

    fn row(&self) -> AnyView {
        let name = self.name;
        let owned = keyed(
            derived(move |world: &World| owns(world, name))
                .when(moved::<Rostered>()),
            |owned| match owned {
                true => yours(),
                false => column(()).boxed(),
            },
        );
        column((
            row((label(self.title).bold(true), owned))
                .space(Space::Half),
            description(self.kind),
            note(format!(
                "{}, {} agents",
                self.price.words(),
                self.agents.len() + 1
            )),
        ))
        .space(Space::None)
        .boxed()
    }

    fn page(&self) -> AnyView {
        let standing = &self.standing;
        let seller = match self.seller.verified {
            true => format!("{}, verified seller", self.seller.name),
            false => format!("{}, not verified", self.seller.name),
        };
        let told = format!(
            "{:.1} of 5 from {} reviews, {} own it, by {seller}",
            standing.stars,
            grouped(standing.reviews.into()),
            grouped(standing.owners.into()),
        );
        let mut team = vec![settler(&self.mayor, true)];
        team.extend(
            self.agents.iter().map(|one| settler(one, false)),
        );
        column((
            note(told),
            deal(self),
            label(self.description),
            part(
                "COMES WITH",
                column(team).space(Space::Half).boxed(),
            ),
        ))
        .boxed()
    }
}

fn yours() -> AnyView {
    row((
        icon("check").tone(Tone::Accent),
        label("YOURS")
            .size(Size::Small)
            .tone(Tone::Accent)
            .face(Face::Display),
    ))
    .space(Space::Half)
    .boxed()
}

/// One who comes with the hamlet: its name and what it does.
fn settler(settler: &Settler, mayor: bool) -> AnyView {
    let name = match mayor {
        true => format!("{}, the mayor", settler.name),
        false => settler.name.to_string(),
    };
    column((label(name).bold(true), description(settler.purpose)))
        .space(Space::None)
        .boxed()
}

/// The state of a hamlet at the stall, for its buttons.
#[derive(Clone, PartialEq)]
enum Deal {
    /// Offered; with the reason it cannot be founded when the realm
    /// is full.
    Open(Option<String>),
    Founding,
    /// In the realm.
    Owned,
}

/// The way to found `template`, or to see it once it is the chief's.
fn deal(template: &Template) -> AnyView {
    let Template {
        name,
        title,
        price,
        seller,
        ..
    } = *template;
    let now = derived(move |world: &World| {
        if owns(world, name) {
            return Deal::Owned;
        }
        if world.resource::<Founding>().0.as_deref() == Some(name) {
            return Deal::Founding;
        }
        Deal::Open(full(world))
    })
    .when(either(
        either(moved::<Rostered>(), moved::<Founding>()),
        moved::<Most>(),
    ));
    keyed(now, move |deal| match deal {
        Deal::Open(full) => {
            let mut parts = vec![
                row((
                    icon_button("house", "FOUND IT")
                        .style(ButtonStyle::Primary)
                        .on_click(move |world| {
                            hamlets::buy(world, name)
                        }),
                    label(price.words()).bold(true),
                ))
                .space(Space::Half)
                .boxed(),
            ];
            if price != Price::Free {
                parts.push(
                    description(format!(
                        "Paying comes with the marketplace, where \
                         {} receives {SELLERS_SHARE}% of each sale. \
                         Until then it is founded for nothing.",
                        seller.name
                    ))
                    .boxed(),
                );
            }
            if let Some(full) = full {
                parts.push(
                    label(full.clone()).tone(Tone::Critical).boxed(),
                );
            }
            column(parts).fill(Fill::Card).space(Space::Half).boxed()
        }
        Deal::Founding => {
            description(format!("Founding {title}...")).boxed()
        }
        Deal::Owned => row((
            yours(),
            icon_button("aim", "GO TO IT")
                .on_click(move |world| hamlets::visit(world, name)),
        ))
        .space(Space::Half)
        .boxed(),
    })
    .boxed()
}
