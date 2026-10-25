//! The Hamlets stall drawn: each hamlet as a row, and its page with
//! its town turning on a plinth, who comes with it and the way to
//! found it.

use bevy::ecs::world::World;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_ui::styles::{action, bare, good_action, note};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, markdown};

use super::diorama;
use super::listings::{rating, seller_words};
use super::parts::{
    Line, deal, fact, mark, nothing, page_head, problem, rows,
    section, sections, told, top,
};
use super::shelf::{Shown, said};
use super::trade::{samples_foot, samples_notice};
use super::words::grouped;
use crate::bazaar::hamlets::{self, Founding, full, owns};
use crate::bazaar::listings::SELLERS_SHARE;
use crate::bazaar::samples::Price;
use crate::bazaar::samples::hamlets::{Settler, Template};
use crate::roll::{either, hamlets_moved, moved};

impl Shown for Template {
    const BACK: &'static str = "HAMLETS";
    const ABOUT: &'static str = "Whole hamlets, to found at once.";
    const NOUN: &'static str = "hamlets";
    const FIND: &'static str =
        "Search by a hamlet's name or what it does.";

    fn notice() -> AnyView {
        column((samples_notice(), full_notice())).gap(10.0).boxed()
    }

    fn foot() -> AnyView {
        samples_foot()
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
            note(self.description).boxed(),
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
            top(vec![
                head,
                diorama::view(self.seed, self.agents.len() as u32),
                founding(self),
            ]),
            section(
                "WHAT IT IS FOR",
                markdown::fixed(self.purpose, Tone::Body).boxed(),
            ),
            section("COMES WITH", rows(team)),
        ])
    }
}

/// Why no more hamlets can be founded, when the realm is full.
pub(super) fn full_notice() -> AnyView {
    keyed(derived(full).when(hamlets_moved()), |full| match full {
        Some(why) => problem(why.clone()),
        None => nothing(),
    })
    .within(bare())
    .boxed()
}

/// The accent line of a hamlet in the realm.
fn yours(name: &'static str) -> AnyView {
    keyed(
        derived(move |world: &World| owns(world, name))
            .when(hamlets_moved()),
        |has| match has {
            true => mark("This hamlet is yours"),
            false => nothing(),
        },
    )
    .within(bare())
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
        if world.resource::<Founding>().is(name) {
            return Deal::Founding;
        }
        Deal::Open(full(world))
    })
    .when(either(hamlets_moved(), moved::<Founding>()));
    let agents = match template.agents.len() {
        1 => "one agent".to_string(),
        count => format!("{count} agents"),
    };
    let comes = format!(
        "Founding it brings {}, its mayor, and {agents} into your \
         realm, with all they know.",
        template.mayor.name
    );
    let pressed = keyed(now, move |deal_now| match deal_now {
        Deal::Open(full) => {
            let mut notes = vec![note(comes.clone()).boxed()];
            if price != Price::Free {
                notes.push(
                    note(format!(
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
                vec![good_action(
                    Glyph::Hamlet,
                    "FOUND THIS HAMLET",
                    move |world| hamlets::buy(world, name),
                )],
                notes,
            )
        }
        Deal::Founding => deal(
            None,
            Vec::new(),
            vec![note(format!("Founding {title}...")).boxed()],
        ),
        Deal::Owned => deal(
            None,
            vec![action(
                Glyph::Aim,
                "GO TO THE HAMLET",
                move |world| {
                    hamlets::visit(world, name);
                },
            )],
            Vec::new(),
        ),
    })
    .within(bare());
    deal(
        Some(price.words()),
        Vec::new(),
        vec![pressed.boxed(), said::<Template>()],
    )
}
