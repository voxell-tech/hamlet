//! The bazaar's own listings drawn as the samples are: a listing's
//! row, and its page with its cover, or a hamlet's town, under its
//! head, the way it is had, what it holds, its details and what
//! owners say.

use agents_ui::{markdown, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, Tone, ViewExt, column, derived, description, keyed,
    label,
};

use crate::BazaarWorldExt;
use crate::listings::{Agents, Hamlets, Sells, Tools, Wares};
use crate::market::lot::{
    AgentMeta, HamletMeta, Kind, Lot, Maker, PluginMeta, Said,
    price_words,
};
use crate::market::{self, Market};
use crate::view::covers::{PAGE, cover};
use crate::view::parts::{
    Line, fact, page_head, rows, section, sections, told, top,
};
use crate::view::shelf::{Shown, browse, kinds};
use crate::view::words::count;
use crate::view::{Desk, diorama, trade};

/// The most reviews a page shows.
const REVIEWS_SHOWN: usize = 5;

/// What a listing of `K` shows that others do not.
pub(crate) trait Shows: Sells {
    /// The dim line of its row under what it is for.
    fn row_meta(lot: &Lot) -> String {
        format!("{}, by {}", rating(lot), lot.seller.shown())
    }

    /// Under the head of its page, in place of its cover.
    fn head(_lot: &Lot) -> Option<AnyView> {
        None
    }

    /// The parts of its page of what it holds.
    fn inside(lot: &Lot) -> Vec<AnyView>;

    /// Its details past those every listing has.
    fn facts(_lot: &Lot) -> Vec<AnyView> {
        Vec::new()
    }
}

impl<K: Shows> Shown for Wares<K> {
    const BACK: &'static str = K::BACK;
    const ABOUT: &'static str = K::ABOUT;
    const NOUN: &'static str = K::NOUN;
    const FIND: &'static str = K::FIND;
    const FROM: Option<&'static str> = Some("the bazaar");

    fn notice() -> AnyView {
        trade::selling(K::KIND)
    }

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

    fn foot() -> AnyView {
        trade::account("to buy, and to sell what you made.")
    }

    fn title(&self) -> String {
        self.0.title.clone()
    }

    fn row(&self) -> Line {
        let lot = &self.0;
        Line {
            name: lot.title.clone(),
            price: price(lot.price_cents),
            purpose: lot.summary.clone(),
            meta: K::row_meta(lot),
            mark: Some(trade::mark(lot.id.clone())),
        }
    }

    fn page(&self) -> AnyView {
        let lot = &self.0;
        let head = page_head(
            &lot.title,
            description(lot.summary.clone()).boxed(),
            format!(
                "{}, {} own it, by {}",
                rating(lot),
                count(lot.owners.into()),
                seller_words(&lot.seller)
            ),
            Some(trade::mark(lot.id.clone())),
        );
        let mut first = vec![head];
        match K::head(lot) {
            Some(shown) => first.push(shown),
            None => first.extend(
                lot.cover_url
                    .as_ref()
                    .map(|url| cover(url.clone(), PAGE)),
            ),
        }
        first.push(trade::deal(
            lot.kind,
            lot.id.clone(),
            lot.price_cents,
            lot.seller.shown().to_string(),
        ));
        let mut parts = vec![top(first)];
        if !lot.description.trim().is_empty() {
            parts.push(section(
                "ABOUT",
                markdown(&lot.description, Tone::Body),
            ));
        }
        parts.extend(K::inside(lot));
        parts.push(section("DETAILS", details::<K>(lot)));
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
        let agent = lot.meta::<AgentMeta>();
        match agent.character.trim() {
            "" => Vec::new(),
            character => vec![section(
                "HOW IT WORKS",
                markdown(character, Tone::Body),
            )],
        }
    }

    fn facts(lot: &Lot) -> Vec<AnyView> {
        let agent = lot.meta::<AgentMeta>();
        let mut facts = Vec::new();
        if !agent.skills.is_empty() {
            facts.push(fact("Skills", agent.skills.join(", ")));
        }
        if agent.notes {
            facts.push(told(
                fact("Notes", "Included"),
                "It comes with the notes its maker's agent kept.",
            ));
        }
        facts
    }
}

impl Shows for Tools {
    fn row_meta(lot: &Lot) -> String {
        format!(
            "{}, {} installs",
            rating(lot),
            count(lot.owners.into())
        )
    }

    fn inside(lot: &Lot) -> Vec<AnyView> {
        let tools = lot.meta::<PluginMeta>().tools;
        if tools.is_empty() {
            return Vec::new();
        }
        let adds = tools
            .iter()
            .map(|tool| {
                told(
                    fact(tool.name.clone(), "Tool"),
                    &tool.description,
                )
            })
            .collect();
        let mut asks = Vec::<&str>::new();
        for asked in tools.iter().flat_map(|tool| &tool.permissions) {
            if !asks.contains(&asked.as_str()) {
                asks.push(asked);
            }
        }
        let mut parts = vec![section("WHAT IT ADDS", rows(adds))];
        if !asks.is_empty() {
            parts.push(section(
                "WHAT IT MAY DO",
                rows(asks.into_iter().map(permission).collect()),
            ));
        }
        parts
    }
}

impl Shows for Hamlets {
    fn row_meta(lot: &Lot) -> String {
        let hamlet = lot.meta::<HamletMeta>();
        let agents = match hamlet.agents.len() {
            0 => format!("1 agent, {}", rating(lot)),
            more => format!("{} agents, {}", more + 1, rating(lot)),
        };
        match hamlet.kind.trim() {
            "" => agents,
            kind => format!("{kind}, {agents}"),
        }
    }

    fn head(lot: &Lot) -> Option<AnyView> {
        let agents = lot.meta::<HamletMeta>().agents.len() as u32;
        Some(diorama::view(lot.id.clone(), agents))
    }

    fn inside(lot: &Lot) -> Vec<AnyView> {
        let hamlet = lot.meta::<HamletMeta>();
        let settler = |agent: &AgentMeta, mayor: bool| {
            let role = if mayor { "Mayor" } else { "" };
            told(fact(agent.name.clone(), role), &agent.purpose)
        };
        let mut team = vec![settler(&hamlet.mayor, true)];
        team.extend(
            hamlet.agents.iter().map(|agent| settler(agent, false)),
        );
        vec![section("COMES WITH", rows(team))]
    }
}

/// The details of `lot`, a fact a line: its kind, what is told of it
/// as a listing of `K`, its tags, its newest version and when it
/// changed.
fn details<K: Shows>(lot: &Lot) -> AnyView {
    let mut all = Vec::new();
    if !lot.category.trim().is_empty() {
        all.push(fact("Kind", lot.category.clone()));
    }
    all.extend(K::facts(lot));
    if !lot.tags.is_empty() {
        all.push(fact("Tags", lot.tags.join(", ")));
    }
    if let Some(latest) = &lot.latest {
        all.push(told(
            fact("Version", latest.version.clone()),
            latest.changes.trim(),
        ));
    }
    all.push(fact("Updated", lot.updated()));
    rows(all)
}

/// Opens the page of `lot` at its stall, from wherever the panel is.
pub(crate) fn open_lot(world: &mut World, lot: &Lot) {
    match lot.kind {
        Kind::Agent => open::<Agents>(world, lot),
        Kind::Plugin => open::<Tools>(world, lot),
        Kind::Hamlet => open::<Hamlets>(world, lot),
    }
}

fn open<K: Shows>(world: &mut World, lot: &Lot) {
    world.resource_mut::<Desk>().page = None;
    world.show(market::stall_of(lot.kind));
    let id = lot.id.clone();
    browse::<Wares<K>>(world, |shelf| shelf.open = Some(id.clone()));
    <Wares<K> as Shown>::opened(world, &id);
}

/// A price in a few words.
pub(crate) fn price(cents: u32) -> String {
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
            "{:.1} from {} reviews",
            lot.stars,
            count(reviews.into())
        ),
    }
}

fn seller_words(seller: &Maker) -> String {
    match seller.verified {
        true => format!("{}, verified seller", seller.shown()),
        false => format!("{}, not verified", seller.shown()),
    }
}

/// What a tool asks to be let do, as the samples say it; what could
/// change what is the chief's in red.
fn permission(asks: &str) -> AnyView {
    let (words, grave) = match asks {
        "run_program" => ("Runs a program on this computer", true),
        "network" => ("Reaches the network", false),
        "read_files" => ("Reads files", false),
        "write_files" => ("Writes files", true),
        other => (other, false),
    };
    let tone = if grave { Tone::Critical } else { Tone::Body };
    label(words.to_string()).tone(tone).boxed()
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
            .take(REVIEWS_SHOWN)
            .map(|said| {
                told(
                    label(format!("\"{}\"", said.words)).boxed(),
                    format!(
                        "{} of 5, from {}",
                        said.stars, said.by.login
                    ),
                )
            })
            .collect::<Vec<_>>();
        section("WHAT OWNERS SAY", rows(all))
    })
    .boxed()
}
