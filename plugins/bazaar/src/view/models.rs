//! The Models stall drawn: a ware's row, the choice of where the
//! models come from and their price, and a ware's page with all that
//! is told of it, who serves it and the ways to add it.

use agents_ui::{AgentsWorldExt, either, moved, name, note};
use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, FrameProps, Size, Space,
    Tone, UiExt, ViewExt, button, column, derived, description, icon,
    icon_button, keyed, label, open, resource, row, switch,
};

use crate::models::openrouter::Serving;
use crate::models::{
    self, Buying, Catalogued, Configured, Providers, Source, Then,
    Ware, WareId, acquire, added, endpoint_of, is_default,
};
use crate::shelf::{Arranged, Listing, Shelf, Stock};
use crate::view::shelf::{Shown, browse, choice, part, summary};
use crate::view::words::{dollars, price_words, tokens};

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
                label("You have no OpenRouter endpoint yet.")
                    .tone(Tone::Critical),
                description(
                    "OpenRouter's models run through an endpoint of \
                     your own, under Endpoints in Settings. Set one \
                     up, and the model is added once you have.",
                ),
                row((
                    icon_button("signal", "SET UP OPENROUTER")
                        .style(ButtonStyle::Primary)
                        .on_click(models::set_up),
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
            .space(Space::Half)
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
            "Only those that use tools, as every agent does",
            resource::<Shelf<Ware>, _>(|shelf| shelf.filter.tools),
        )
        .on_toggle(|world, on| {
            browse::<Ware>(world, |shelf| shelf.filter.tools = on)
        })
        .boxed();
        vec![row((sources, price)).space(Space::Half).boxed(), tools]
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
                format!("{name} could not be asked: {why}")
            }
            _ if !any && listing.asking > 0 => {
                format!("Asking {name}...")
            }
            _ if !any => {
                format!(
                    "No models from {name} yet. Choose Look Again."
                )
            }
            _ => format!("{found} models from {name}."),
        }
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn row(&self) -> AnyView {
        column((
            row((
                label(self.title.clone()).bold(true),
                badge(self.id()),
                note(price_words(self.input, self.output)),
            ))
            .space(Space::Half),
            description(facts(self)),
        ))
        .space(Space::None)
        .boxed()
    }

    fn page(&self) -> AnyView {
        let mut parts = vec![note(self.id.clone()).boxed()];
        if self.tools == Some(false) {
            parts.push(
                label(
                    "This model can't use tools, so agents can only \
                     chat with it.",
                )
                .tone(Tone::Critical)
                .boxed(),
            );
        }
        parts.push(deals(self.id()));
        if let Some(first) = self.description.split("\n\n").next()
            && !first.trim().is_empty()
        {
            parts.push(description(first.trim().to_string()).boxed());
        }
        parts.push(about(self));
        if self.source == Source::OpenRouter {
            parts.push(part("PROVIDERS", providers(self.id.clone())));
        }
        column(parts).boxed()
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

/// ADDED beside a ware the home allows.
fn badge(id: WareId) -> AnyView {
    keyed(
        derived(move |world: &World| added(world, &id)).when(either(
            moved::<Catalogued>(),
            moved::<Configured>(),
        )),
        |added| match added {
            true => row((
                icon("check").tone(Tone::Accent),
                label("ADDED")
                    .size(Size::Small)
                    .tone(Tone::Accent)
                    .face(Face::Display),
            ))
            .space(Space::Half)
            .boxed(),
            false => column(()).boxed(),
        },
    )
    .boxed()
}

/// The facts a row tells of a ware besides its name and price.
fn facts(ware: &Ware) -> String {
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
    facts.join(", ")
}

/// The deals of the ware `id`: adding it and making it the default.
fn deals(id: WareId) -> AnyView {
    let read = id.clone();
    let now = derived(move |world: &World| {
        (added(world, &read), is_default(world, &read))
    })
    .when(either(moved::<Catalogued>(), moved::<Configured>()));
    keyed(now, move |(added, default)| {
        let deal = |then: Then| {
            let id = id.clone();
            move |world: &mut World| acquire(world, &id, then)
        };
        let mut all = Vec::new();
        if *added {
            all.push(badge(id.clone()));
        } else {
            all.push(
                icon_button("plus", "ADD")
                    .style(ButtonStyle::Primary)
                    .on_click(deal(Then::Keep))
                    .boxed(),
            );
        }
        if *default {
            all.push(note("Your default model.").boxed());
        } else {
            all.push(
                icon_button("sliders", "MAKE DEFAULT")
                    .on_click(deal(Then::MakeDefault))
                    .boxed(),
            );
        }
        row(all).space(Space::Half).boxed()
    })
    .boxed()
}

/// A fact of a ware: its name and what it is.
fn fact(text: &str, value: String) -> AnyView {
    row((name(text.to_string()), label(value)))
        .space(Space::Half)
        .boxed()
}

fn about(ware: &Ware) -> AnyView {
    let mut all = Vec::new();
    let price = price_words(ware.input, ware.output);
    let price = match ware.input.zip(ware.output) {
        Some(_) => format!("{price} per million tokens"),
        None => price,
    };
    all.push(fact("PRICE", price));
    if let Some(context) = ware.context {
        all.push(fact(
            "CONTEXT",
            format!("{} tokens", tokens(context)),
        ));
    }
    if !ware.inputs.is_empty() {
        all.push(fact("READS", ware.inputs.join(", ")));
    }
    if let Some(index) = ware.intelligence {
        all.push(fact("INTELLIGENCE", format!("{index:.0} of 100")));
    }
    column(all).space(Space::Half).boxed()
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
    column((
        label(serving.provider.clone()).bold(true),
        description(told.join(", ")),
    ))
    .space(Space::None)
    .boxed()
}

/// The providers that serve the ware `id` through OpenRouter.
fn providers(id: String) -> AnyView {
    let known = derived(move |world: &World| {
        world.resource::<Providers>().0.get(&id).cloned()
    })
    .when(moved::<Providers>());
    keyed(known, |known| match known {
        None | Some(None) => {
            description("Asking OpenRouter...").boxed()
        }
        Some(Some(Err(why))) => description(why.clone()).boxed(),
        Some(Some(Ok(servings))) if servings.is_empty() => {
            description("No provider serves it now.").boxed()
        }
        Some(Some(Ok(servings))) => column(
            servings
                .iter()
                .take(PROVIDERS)
                .map(serving)
                .collect::<Vec<_>>(),
        )
        .space(Space::Half)
        .boxed(),
    })
    .boxed()
}
