//! The Hamlets stall drawn: each hamlet as a row, and its page with
//! who comes with it and the way to found it.

use agents_ui::{Rostered, either, markdown, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, Tone, UiExt, ViewExt, column, derived, description,
    icon_button, keyed,
};

use crate::hamlets::{self, Founding, Most, full, owns};
use crate::listings::SELLERS_SHARE;
use crate::samples::Price;
use crate::samples::hamlets::{Settler, Template};
use crate::view::diorama;
use crate::view::listings::{rating, seller_words};
use crate::view::parts::{
    Line, deal, fact, mark, page_head, problem, rows, section,
    sections, told, top,
};
use crate::view::shelf::{Shown, said};
use crate::view::trade::samples_notice;
use crate::view::words::grouped;

impl Shown for Template {
    const BACK: &'static str = "HAMLETS";
    const ABOUT: &'static str = "Whole hamlets, to found at once.";
    const NOUN: &'static str = "hamlets";
    const FIND: &'static str = "Search hamlets";

    fn notice() -> AnyView {
        column((samples_notice(), full_notice())).boxed()
    }

    fn title(&self) -> String {
        self.title.to_string()
    }

    fn row(&self) -> Line {
        Line {
            name: self.title.to_string(),
            price: self.price.words(),
            purpose: self.kind.to_string(),
            meta: format!(
                "{} agents, {}",
                self.agents.len() + 1,
                rating(self.standing)
            ),
            mark: Some(yours(self.name)),
        }
    }

    fn page(&self) -> AnyView {
        let head = page_head(
            self.title,
            description(self.description).boxed(),
            format!(
                "{}, {} own it, by {}",
                rating(self.standing),
                grouped(self.standing.owners.into()),
                seller_words(&self.seller)
            ),
            Some(yours(self.name)),
        );
        let mut team = vec![settler(&self.mayor, true)];
        team.extend(
            self.agents.iter().map(|one| settler(one, false)),
        );
        sections(vec![
            top(vec![head, diorama::view(self), founding(self)]),
            section(
                "WHAT IT IS FOR",
                markdown(self.purpose, Tone::Body),
            ),
            section("COMES WITH", rows(team)),
        ])
    }
}

/// Why no more hamlets can be founded, when the realm is full.
fn full_notice() -> AnyView {
    let full = derived(full)
        .when(either(moved::<Rostered>(), moved::<Most>()));
    keyed(full, |full| match full {
        Some(why) => problem(why.clone()),
        None => column(()).boxed(),
    })
    .boxed()
}

/// The accent line of a hamlet in the realm.
fn yours(name: &'static str) -> AnyView {
    keyed(
        derived(move |world: &World| owns(world, name))
            .when(moved::<Rostered>()),
        |has| match has {
            true => mark("This hamlet is yours"),
            false => column(()).boxed(),
        },
    )
    .boxed()
}

/// One who comes with the hamlet: its name and what it does.
fn settler(settler: &Settler, mayor: bool) -> AnyView {
    let role = if mayor { "Mayor" } else { "" };
    told(fact(settler.name, role), settler.purpose)
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

/// The way to found `template`, or to go to it once it is the
/// chief's.
fn founding(template: &Template) -> AnyView {
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
    let buttons = keyed(now, move |deal_now| match deal_now {
        Deal::Open(full) => {
            let mut notes = vec![description(comes.clone()).boxed()];
            if price != Price::Free {
                notes.push(
                    description(format!(
                        "A sample, founded for nothing. At the \
                         bazaar's server {} would receive \
                         {SELLERS_SHARE}% of each sale.",
                        seller.name
                    ))
                    .boxed(),
                );
            }
            notes.extend(full.clone().map(problem));
            deal(
                None,
                vec![
                    icon_button("house", "FOUND THIS HAMLET")
                        .on_click(move |world| {
                            hamlets::buy(world, name)
                        })
                        .boxed(),
                ],
                notes,
            )
        }
        Deal::Founding => deal(
            None,
            Vec::new(),
            vec![description(format!("Founding {title}...")).boxed()],
        ),
        Deal::Owned => deal(
            None,
            vec![
                icon_button("aim", "GO TO THE HAMLET")
                    .on_click(move |world| {
                        hamlets::visit(world, name)
                    })
                    .boxed(),
            ],
            Vec::new(),
        ),
    });
    deal(
        Some(price.words()),
        Vec::new(),
        vec![buttons.boxed(), said::<Template>()],
    )
}
