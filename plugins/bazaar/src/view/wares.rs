//! The bazaar's own listings drawn: a listing's card with how it
//! stands with the chief, and its page with its maker, what it holds,
//! what owners say and the box it is had from. Covers and avatars are
//! kept in what the server says, and drawn once a plugin's view can
//! show a picture.

use agents_ui::{moved, name, note};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, FrameProps, Space, Tone, ViewExt, column, derived,
    description, icon, keyed, label, row,
};

use crate::listings::{Agents, Hamlets, Sells, Tools, Wares};
use crate::market::lot::{
    AgentMeta, HamletMeta, Kind, Lot, PluginMeta, Said, price_words,
};
use crate::market::{self, Market};
use crate::view::parts::{
    Line, page_head, section as part, sections, top,
};
use crate::view::shelf::{Shown, kinds};
use crate::view::trade;
use crate::view::words::count;

/// What a listing of `K` shows that others do not.
pub(crate) trait Shows: Sells {
    /// Under its words, on its page.
    fn inside(lot: &Lot) -> Vec<AnyView>;
}

impl<K: Shows> Shown for Wares<K> {
    const BACK: &'static str = K::BACK;
    const ABOUT: &'static str = K::ABOUT;
    const NOUN: &'static str = K::NOUN;
    const FIND: &'static str = K::FIND;
    const FROM: Option<&'static str> = Some("the bazaar");

    fn controls() -> Vec<AnyView> {
        match K::CATEGORIES.is_empty() {
            true => Vec::new(),
            false => {
                vec![kinds::<Wares<K>, _>(
                    "All kinds",
                    K::CATEGORIES,
                    K::category,
                )]
            }
        }
    }

    fn again(world: &mut World) {
        market::ask_shelves(world);
    }

    fn title(&self) -> String {
        self.0.title.clone()
    }

    fn row(&self) -> Line {
        let lot = &self.0;
        let meta = match lot.kind {
            Kind::Hamlet => {
                let meta = lot.meta::<HamletMeta>();
                format!(
                    "{}, {} agents, by {}",
                    rating(lot),
                    meta.agents.len() + 1,
                    lot.seller.shown()
                )
            }
            _ => {
                format!("{}, by {}", rating(lot), lot.seller.shown())
            }
        };
        Line {
            name: lot.title.clone(),
            price: price(lot.price_cents),
            purpose: lot.summary.clone(),
            meta,
            mark: Some(trade::mark(lot.id.clone())),
        }
    }

    fn page(&self) -> AnyView {
        let lot = &self.0;
        let seller = match lot.seller.verified {
            true => {
                format!("{}, verified seller", lot.seller.shown())
            }
            false => format!("{}, not verified", lot.seller.shown()),
        };
        let head = page_head(
            &lot.title,
            description(lot.summary.clone()).boxed(),
            format!(
                "{}, {} own it, by {seller}",
                rating(lot),
                count(lot.owners.into())
            ),
            Some(trade::mark(lot.id.clone())),
        );
        let mut parts = vec![top(vec![
            head,
            trade::deal(
                lot.kind,
                lot.id.clone(),
                lot.price_cents,
                lot.seller.shown().to_string(),
            ),
        ])];
        if !lot.description.trim().is_empty() {
            parts.push(description(lot.description.clone()).boxed());
        }
        parts.extend(K::inside(lot));
        if !lot.tags.is_empty() {
            parts.push(part(
                "TAGS",
                note(lot.tags.join(", ")).boxed(),
            ));
        }
        if let Some(latest) = &lot.latest {
            let words = match latest.changes.trim() {
                "" => {
                    format!("{}, {}", latest.version, lot.updated())
                }
                changes => format!(
                    "{}, {}: {changes}",
                    latest.version,
                    lot.updated()
                ),
            };
            parts.push(part(
                "LATEST VERSION",
                description(words).boxed(),
            ));
        }
        parts.push(reviews(lot.id.clone()));
        sections(parts)
    }

    fn opened(world: &mut World, id: &String) {
        if world.resource::<Market>().reach == market::Reach::Reached
        {
            market::ask_detail(world, id);
        }
    }
}

impl Shows for Agents {
    fn inside(lot: &Lot) -> Vec<AnyView> {
        let meta = lot.meta::<AgentMeta>();
        let mut parts = Vec::new();
        if !meta.character.trim().is_empty() {
            parts.push(part(
                "HOW IT WORKS",
                description(meta.character.clone()).boxed(),
            ));
        }
        if !meta.skills.is_empty() {
            parts.push(part(
                "SKILLS",
                note(meta.skills.join(", ")).boxed(),
            ));
        }
        if meta.notes {
            parts.push(
                description(
                    "It comes with the notes its maker's agent kept.",
                )
                .boxed(),
            );
        }
        parts
    }
}

impl Shows for Tools {
    fn inside(lot: &Lot) -> Vec<AnyView> {
        let tools = lot.meta::<PluginMeta>().tools;
        if tools.is_empty() {
            return Vec::new();
        }
        let all = tools
            .iter()
            .map(|tool| {
                let asks = tool
                    .permissions
                    .iter()
                    .map(|asks| permission(asks))
                    .collect::<Vec<_>>();
                column((
                    label(tool.name.clone()).bold(true),
                    description(tool.description.clone()),
                    column(asks).space(Space::None),
                ))
                .space(Space::None)
                .boxed()
            })
            .collect::<Vec<_>>();
        vec![part("TOOLS", column(all).space(Space::Half).boxed())]
    }
}

impl Shows for Hamlets {
    fn inside(lot: &Lot) -> Vec<AnyView> {
        let meta = lot.meta::<HamletMeta>();
        let settler = |agent: &AgentMeta, mayor: bool| {
            let name = match mayor {
                true => format!("{}, the mayor", agent.name),
                false => agent.name.clone(),
            };
            column((
                label(name).bold(true),
                description(agent.purpose.clone()),
            ))
            .space(Space::None)
            .boxed()
        };
        let mut team = vec![settler(&meta.mayor, true)];
        team.extend(
            meta.agents.iter().map(|agent| settler(agent, false)),
        );
        let mut parts = Vec::new();
        if !meta.kind.trim().is_empty() {
            parts.push(note(meta.kind.clone()).boxed());
        }
        parts.push(part(
            "COMES WITH",
            column(team).space(Space::Half).boxed(),
        ));
        parts
    }
}

/// A price in a few words.
fn price(cents: u32) -> String {
    match cents {
        0 => "Free".into(),
        cents => format!("{} once", price_words(cents)),
    }
}

/// Stars out of five and the reviews they come from.
fn rating(lot: &Lot) -> String {
    match lot.reviews {
        0 => "No reviews yet".into(),
        reviews => format!(
            "{:.1} of 5 from {} reviews",
            lot.stars,
            count(reviews.into())
        ),
    }
}

/// What a tool asks to be let do; what could change what is the
/// chief's in red.
fn permission(asks: &str) -> AnyView {
    let (words, grave) = match asks {
        "run_program" => ("Runs a program on this computer", true),
        "network" => ("Reaches the network", false),
        "read_files" => ("Reads files", false),
        "write_files" => ("Writes files", true),
        other => (other, false),
    };
    let tone = if grave { Tone::Critical } else { Tone::Body };
    row((
        icon("shield").tone(tone),
        label(words.to_string()).tone(tone),
    ))
    .space(Space::Half)
    .boxed()
}

/// What owners said of the listing `id`, once its page has asked.
fn reviews(id: String) -> AnyView {
    let said = derived(move |world: &World| {
        world
            .resource::<Market>()
            .details
            .get(&id)
            .map(|detail| detail.reviews.clone())
            .unwrap_or_default()
    })
    .when(moved::<Market>());
    keyed(said, |said: &Vec<Said>| {
        if said.is_empty() {
            return column(()).boxed();
        }
        let all = said
            .iter()
            .take(5)
            .map(|said| {
                column((
                    label(format!("\"{}\"", said.words)),
                    note(format!(
                        "{} of 5, {}",
                        said.stars, said.by.login
                    )),
                ))
                .space(Space::None)
                .boxed()
            })
            .collect::<Vec<_>>();
        column((
            name("WHAT OWNERS SAY"),
            column(all).space(Space::Half),
        ))
        .space(Space::Half)
        .boxed()
    })
    .boxed()
}
