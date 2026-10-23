//! The Models stall drawn: a ware's row, the choice of where the
//! models come from and their price, and a ware's page with all that
//! is told of it, who serves it and the ways to add it.

use agents_ui::{AgentsWorldExt, either, moved};
use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Fill, FrameProps, Space, Tone,
    UiExt, ViewExt, button, column, derived, icon_button, keyed,
    open, resource, row, switch,
};

use crate::models::openrouter::Serving;
use crate::models::{
    self, Buying, Catalogued, Configured, Providers, Source, Then,
    Ware, WareId, acquire, added, endpoint_of, is_default,
};
use crate::shelf::{Arranged, Listing, Shelf, Stock};
use crate::view::shelf::{Shown, browse, choice, counted, summary};
use crate::view::style::{
    Cost, Line, action_box, aside, lines, meta, named, owned, prose,
    section, sections,
};
use crate::view::words::{dollars, tokens};

/// The engine's Settings panel.
const SETTINGS: &str = "engine/settings";

/// The prices a ware's price in can be kept under, in dollars for a
/// million tokens.
const PRICES: [(&str, Option<f64>); 5] = [
    ("Any price", None),
    ("Free", Some(0.0)),
    ("Up to $1", Some(1.0)),
    ("Up to $5", Some(5.0)),
    ("Up to $20", Some(20.0)),
];

/// The providers shown on a ware's page.
const PROVIDERS: usize = 3;

impl Shown for Ware {
    const BACK: &'static str = "MODELS";
    const NOUN: &'static str = "models";
    const FIND: &'static str = "Search models";
    const FROM: Option<&'static str> = Some("OpenRouter");

    fn heading() -> AnyView {
        let waiting = derived(|world: &World| {
            world.resource::<Buying>().waiting.is_some()
        })
        .when(moved::<Buying>());
        keyed(waiting, |waiting| {
            if !*waiting {
                return column(()).boxed();
            }
            column((
                prose("You have no OpenRouter endpoint yet.")
                    .tone(Tone::Critical),
                aside(
                    "OpenRouter's models run through an endpoint of \
                     your own, under Endpoints in Settings. Set one \
                     up, and the model is added once you have.",
                ),
                row((icon_button("signal", "SET UP OPENROUTER")
                    .style(ButtonStyle::Primary)
                    .on_click(models::set_up),)),
                row((
                    icon_button("sliders", "OPEN SETTINGS")
                        .on_click(|_| open(SETTINGS)),
                    button("CANCEL")
                        .style(ButtonStyle::Quiet)
                        .on_click(|world| {
                            world.resource_mut::<Buying>().waiting =
                                None
                        }),
                ))
                .space(Space::Half),
            ))
            .fill(Fill::Card)
            .space(Space::Gap)
            .boxed()
        })
        .boxed()
    }

    fn controls() -> Vec<AnyView> {
        let sources = keyed(
            derived(sources).when(moved::<Configured>()),
            |all| {
                let names = all
                    .iter()
                    .map(|source| match source {
                        Source::OpenRouter => {
                            "OpenRouter".to_string()
                        }
                        Source::Endpoint(name) => {
                            format!("Endpoint: {name}")
                        }
                    })
                    .collect();
                let (at, set) = (all.clone(), all.clone());
                choice::<Ware>(
                    names,
                    move |shelf| {
                        at.iter()
                            .position(|s| *s == shelf.filter.source)
                            .unwrap_or(0)
                    },
                    move |shelf, chosen| {
                        if let Some(source) = set.get(chosen) {
                            shelf.filter.source = source.clone();
                        }
                    },
                )
            },
        )
        .boxed();
        let price = choice::<Ware>(
            PRICES.iter().map(|(name, _)| name.to_string()).collect(),
            |shelf| {
                PRICES
                    .iter()
                    .position(|(_, most)| {
                        *most == shelf.filter.most_input
                    })
                    .unwrap_or(0)
            },
            |shelf, chosen| {
                shelf.filter.most_input = PRICES[chosen].1
            },
        );
        let tools = switch(
            "Uses tools",
            resource::<Shelf<Ware>, _>(|shelf| shelf.filter.tools),
        )
        .on_toggle(|world, on| {
            browse::<Ware>(world, |shelf| shelf.filter.tools = on)
        })
        .boxed()
        .tip(
            "Every agent calls tools, so only these can do its work.",
        );
        vec![sources, price.boxed(), tools.boxed()]
    }

    fn again(world: &mut World) {
        models::fetch_wares(world);
        world.ask(AgentsRequest::ListModels);
    }

    fn summary(world: &World) -> String {
        let source = &world.resource::<Shelf<Ware>>().filter.source;
        let Source::Endpoint(name) = source else {
            return summary::<Ware>(world);
        };
        let found = world.resource::<Arranged<Ware>>().found;
        let any = world
            .resource::<Stock<Ware>>()
            .items
            .iter()
            .any(|ware| ware.source == *source);
        let listing = world
            .resource::<Configured>()
            .0
            .as_ref()
            .map(|settings| settings.listing.clone())
            .unwrap_or_default();
        let said = listing
            .said
            .iter()
            .find(|listed| listed.endpoint == *name);
        match said.map(|listed| &listed.models) {
            Some(Err(why)) => {
                format!(
                    "{name} could not be asked: {why} Check it in \
                     Settings, then press Look again."
                )
            }
            _ if !any && listing.asking > 0 => {
                format!("Asking {name}...")
            }
            _ if !any => {
                format!(
                    "No models from {name} yet. Press Look again to \
                     ask it."
                )
            }
            _ => format!("{} from {name}.", counted::<Ware>(found)),
        }
    }

    fn title(&self) -> String {
        made(&self.title).1.to_string()
    }

    fn byline(&self) -> String {
        match made(&self.title) {
            (Some(maker), _) => format!("By {maker}, {}", self.id),
            (None, name) if name != self.id => self.id.clone(),
            (None, _) => String::new(),
        }
    }

    fn row(&self) -> Line {
        let (maker, name) = made(&self.title);
        let mut told =
            maker.map(str::to_string).into_iter().collect::<Vec<_>>();
        told.extend(facts(self));
        Line {
            cover: "chip",
            name: name.to_string(),
            purpose: lead(self).unwrap_or_default(),
            meta: told.join(", "),
            cost: row_cost(self),
            mark: Some(badge(self.id())),
        }
    }

    fn page(&self) -> AnyView {
        let mut parts = vec![deals(self)];
        if let Some(first) = self.description.split("\n\n").next()
            && !first.trim().is_empty()
        {
            parts.push(prose(first.trim().to_string()).boxed());
        }
        parts.push(section("DETAILS", details(self)));
        if self.source == Source::OpenRouter {
            parts.push(section(
                "PROVIDERS",
                providers(self.id.clone()),
            ));
        }
        sections(parts)
    }

    fn opened(world: &mut World, id: &WareId) {
        if id.source == Source::OpenRouter {
            models::look(world, &id.id);
        }
    }
}

/// The endpoints whose models can be browsed: OpenRouter first, then
/// each other endpoint of the settings, by name.
fn sources(world: &World) -> Vec<Source> {
    let mut all = vec![Source::OpenRouter];
    if let Some(settings) = &world.resource::<Configured>().0 {
        let openrouter = endpoint_of(settings, &Source::OpenRouter);
        all.extend(
            settings
                .endpoints
                .iter()
                .filter(|endpoint| {
                    Some(&endpoint.name) != openrouter.as_ref()
                })
                .map(|endpoint| {
                    Source::Endpoint(endpoint.name.clone())
                }),
        );
    }
    all
}

/// The maker a title of OpenRouter's names before its colon, and the
/// rest.
fn made(title: &str) -> (Option<&str>, &str) {
    match title.split_once(": ") {
        Some((maker, name)) if !name.is_empty() => {
            (Some(maker), name)
        }
        _ => (None, title),
    }
}

/// The first sentence of what a ware is told to be.
fn lead(ware: &Ware) -> Option<String> {
    let first = ware.description.lines().next()?.trim();
    let first = first.split_once(". ").map_or(first, |(one, _)| one);
    let first = first.trim_end_matches('.');
    (!first.is_empty()).then(|| format!("{first}."))
}

/// A ware's price on its row: a million tokens in, as wares are
/// ordered by it.
fn row_cost(ware: &Ware) -> Cost {
    match ware.input.zip(ware.output) {
        None => Cost::new("Varies", ""),
        Some((0.0, 0.0)) => Cost::free(),
        Some((input, _)) => Cost::new(dollars(input), "input"),
    }
}

/// A ware's price on its page.
fn page_cost(ware: &Ware) -> Cost {
    match ware.input.zip(ware.output) {
        None => Cost::new("Price varies", "by provider"),
        Some((0.0, 0.0)) => Cost::free(),
        Some((input, output)) => Cost::new(
            format!("{} in, {} out", dollars(input), dollars(output)),
            "per million tokens",
        ),
    }
}

/// In your catalogue, beside a ware the home allows.
fn badge(id: WareId) -> AnyView {
    keyed(
        derived(move |world: &World| added(world, &id)).when(either(
            moved::<Catalogued>(),
            moved::<Configured>(),
        )),
        |added| match added {
            true => owned("In your catalogue"),
            false => column(()).boxed(),
        },
    )
    .boxed()
}

/// The facts a row tells of a ware besides its name and price.
fn facts(ware: &Ware) -> Vec<String> {
    let mut facts = Vec::new();
    if let Some(context) = ware.context {
        facts.push(format!("{} context", tokens(context)));
    }
    if ware.tools == Some(false) {
        facts.push("no tools".into());
    }
    if ware.reasoning == Some(true) {
        facts.push("reasoning".into());
    }
    if ware.sees() {
        facts.push("images".into());
    }
    facts
}

/// The box a ware is had from: its price, adding it and making it the
/// default.
fn deals(ware: &Ware) -> AnyView {
    let id = ware.id();
    let read = id.clone();
    let blind = ware.tools == Some(false);
    let cost = page_cost(ware);
    let now = derived(move |world: &World| {
        (added(world, &read), is_default(world, &read))
    })
    .when(either(moved::<Catalogued>(), moved::<Configured>()));
    keyed(now, move |(added, default)| {
        let deal = |then: Then| {
            let id = id.clone();
            move |world: &mut World| acquire(world, &id, then)
        };
        let mut actions = Vec::new();
        if *added {
            actions.push(owned("In your catalogue"));
        } else {
            actions.push(
                icon_button("plus", "ADD TO THE CATALOGUE")
                    .style(ButtonStyle::Primary)
                    .on_click(deal(Then::Keep))
                    .boxed(),
            );
        }
        if *default {
            actions.push(owned("Your default model"));
        } else {
            actions.push(
                icon_button("sliders", "MAKE IT THE DEFAULT")
                    .on_click(deal(Then::MakeDefault))
                    .boxed(),
            );
        }
        let mut notes = Vec::new();
        if blind {
            notes.push(
                prose(
                    "This model can't use tools, so agents can only \
                     chat with it.",
                )
                .tone(Tone::Critical)
                .boxed(),
            );
        }
        if !*added {
            notes.push(
                meta(
                    "Adding it puts it in your catalogue, where you \
                     can give it to any agent.",
                )
                .boxed(),
            );
        }
        action_box(Some(cost.clone()), actions, notes)
    })
    .boxed()
}

/// What a ware's page tells of it past its price, a line each.
fn details(ware: &Ware) -> AnyView {
    let mut all = Vec::new();
    if let Some(context) = ware.context {
        all.push(format!("{} tokens of context", tokens(context)));
    }
    if !ware.inputs.is_empty() {
        all.push(format!("Reads {}", ware.inputs.join(", ")));
    }
    if ware.reasoning == Some(true) {
        all.push("Reasons before it answers".into());
    }
    if let Some(index) = ware.intelligence {
        all.push(format!(
            "Scores {index:.0} of 100 on Artificial Analysis's index"
        ));
    }
    lines(all.into_iter().map(|line| aside(line).boxed()).collect())
}

/// One provider that serves the ware.
fn serving(serving: &Serving) -> AnyView {
    let mut told = Vec::new();
    if let Some(uptime) = serving.uptime {
        told.push(format!("{uptime:.1}% up over a day"));
    }
    if let (Some(input), Some(output)) =
        (serving.input, serving.output)
    {
        told.push(format!(
            "{} in, {} out",
            dollars(input),
            dollars(output)
        ));
    }
    if let Some(context) = serving.context {
        told.push(format!("{} context", tokens(context)));
    }
    if let Some(quantization) = &serving.quantization {
        told.push(quantization.clone());
    }
    named(serving.provider.clone(), told.join(", "))
}

/// The providers that serve the ware `id` through OpenRouter.
fn providers(id: String) -> AnyView {
    let known = derived(move |world: &World| {
        world.resource::<Providers>().0.get(&id).cloned()
    })
    .when(moved::<Providers>());
    keyed(known, |known| match known {
        None | Some(None) => aside("Asking OpenRouter...").boxed(),
        Some(Some(Err(why))) => aside(format!(
            "OpenRouter could not say who serves it: {why}"
        ))
        .boxed(),
        Some(Some(Ok(servings))) if servings.is_empty() => {
            aside("No provider serves it now.").boxed()
        }
        Some(Some(Ok(servings))) => lines(
            servings
                .iter()
                .take(PROVIDERS)
                .map(serving)
                .collect::<Vec<_>>(),
        ),
    })
    .boxed()
}
