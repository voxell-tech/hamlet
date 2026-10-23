//! The Hamlets stall drawn: each hamlet as a row, and its page with
//! who comes with it and the way to found it.

use agents_ui::{Rostered, either, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Tone, UiExt, ViewExt, column,
    derived, icon_button, keyed,
};

use crate::hamlets::{self, Founding, Most, full, owns};
use crate::samples::Price;
use crate::samples::hamlets::{Settler, Template};
use crate::view::diorama;
use crate::view::listings::{rating, seller_words, share};
use crate::view::shelf::Shown;
use crate::view::style::{
    Line, action_box, lines, meta, named, notice, owned, prose,
    section, sections,
};
use crate::view::words::grouped;

impl Shown for Template {
    const BACK: &'static str = "HAMLETS";
    const NOUN: &'static str = "hamlets";
    const FIND: &'static str = "Search hamlets";

    fn heading() -> AnyView {
        let full = derived(full)
            .when(either(moved::<Rostered>(), moved::<Most>()));
        keyed(full, |full| match full {
            Some(why) => notice(why.clone(), true),
            None => column(()).boxed(),
        })
        .boxed()
    }

    fn title(&self) -> String {
        self.title.to_string()
    }

    fn byline(&self) -> String {
        format!(
            "{}, {} own it, by {}",
            rating(self.standing),
            grouped(self.standing.owners.into()),
            seller_words(&self.seller)
        )
    }

    fn row(&self) -> Line {
        let name = self.name;
        let mark = keyed(
            derived(move |world: &World| owns(world, name))
                .when(moved::<Rostered>()),
            |has| match has {
                true => owned("Yours"),
                false => column(()).boxed(),
            },
        );
        Line {
            cover: "house",
            name: self.title.to_string(),
            purpose: self.kind.to_string(),
            meta: format!(
                "{} agents, {}",
                self.agents.len() + 1,
                rating(self.standing)
            ),
            cost: self.price.cost(),
            mark: Some(mark.boxed()),
        }
    }

    fn page(&self) -> AnyView {
        let mut team = vec![settler(&self.mayor, true)];
        team.extend(
            self.agents.iter().map(|one| settler(one, false)),
        );
        sections(vec![
            diorama::view(self),
            deal(self),
            prose(self.description).boxed(),
            section("COMES WITH", lines(team)),
        ])
    }
}

/// One who comes with the hamlet: its name and what it does.
fn settler(settler: &Settler, mayor: bool) -> AnyView {
    let name = match mayor {
        true => format!("{}, the mayor", settler.name),
        false => settler.name.to_string(),
    };
    named(name, settler.purpose.to_string())
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
    let agents = match template.agents.len() {
        1 => "one agent".to_string(),
        count => format!("{count} agents"),
    };
    let comes = format!(
        "Founding it brings {}, its mayor, and {agents} into your \
         realm, with all they know.",
        template.mayor.name
    );
    keyed(now, move |deal| match deal {
        Deal::Open(full) => {
            let mut notes = vec![meta(comes.clone()).boxed()];
            if price != Price::Free {
                notes.push(
                    meta(format!(
                        "Paying comes with the marketplace, where \
                         {} Until then it is founded for nothing.",
                        share(&seller)
                    ))
                    .boxed(),
                );
            }
            if let Some(full) = full {
                notes.push(
                    prose(full.clone()).tone(Tone::Critical).boxed(),
                );
            }
            action_box(
                Some(price.cost()),
                vec![
                    icon_button("house", "FOUND THIS HAMLET")
                        .style(ButtonStyle::Primary)
                        .on_click(move |world| {
                            hamlets::buy(world, name)
                        })
                        .boxed(),
                ],
                notes,
            )
        }
        Deal::Founding => action_box(
            None,
            Vec::new(),
            vec![meta(format!("Founding {title}...")).boxed()],
        ),
        Deal::Owned => action_box(
            None,
            vec![
                owned("This hamlet is yours"),
                icon_button("aim", "GO TO THE HAMLET")
                    .on_click(move |world| {
                        hamlets::visit(world, name)
                    })
                    .boxed(),
            ],
            Vec::new(),
        ),
    })
    .boxed()
}
