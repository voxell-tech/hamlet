//! The Models stall drawn: a ware's row, the choice of where the
//! models come from and their price, and a ware's page with all that
//! is told of it, who serves it and the ways to add it.

use agents_ui::{AgentsWorldExt, either, moved};
use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, UiExt, ViewExt, button, column, derived,
    description, icon_button, keyed, open, resource, switch,
};

use crate::models::openrouter::Serving;
use crate::models::{
    self, Buying, Catalogued, Configured, Providers, Source, Then,
    Ware, WareId, acquire, added, endpoint_of, is_default,
};
use crate::shelf::{Arranged, Listing, Shelf, Stock};
use crate::view::parts::{
    self, Line, deal, fact, mark, note, page_head, problem, rows,
    section, sections, told, top,
};
use crate::view::shelf::{
    Shown, browse, choice, counted, said, summary,
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
    const ABOUT: &'static str = "Models to add to your catalogue.";
    const NOUN: &'static str = "models";
    const FIND: &'static str = "Search models";
    const FROM: Option<&'static str> = Some("OpenRouter");

    fn notice() -> AnyView {
        waiting()
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

    fn again_words() -> String {
        "Asks OpenRouter and your endpoints for their models again."
            .into()
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
        let asking = world
            .resource::<Configured>()
            .0
            .as_ref()
            .is_some_and(|settings| settings.listing.asking > 0);
        match any {
            false if asking => format!("Asking {name}..."),
            false => format!("No models from {name} yet."),
            true => {
                format!("{} from {name}.", counted::<Ware>(found))
            }
        }
    }

    fn trouble(world: &World) -> Option<String> {
        let source = &world.resource::<Shelf<Ware>>().filter.source;
        let Source::Endpoint(name) = source else {
            return world.resource::<Stock<Ware>>().problem.clone();
        };
        let settings = world.resource::<Configured>().0.as_ref()?;
        let listed = settings
            .listing
            .said
            .iter()
            .find(|listed| listed.endpoint == *name)?;
        match &listed.models {
            Err(why) => Some(format!(
                "{name} could not be asked: {why} Check it in \
                 Settings."
            )),
            Ok(_) => None,
        }
    }

    fn title(&self) -> String {
        made(&self.title).1.to_string()
    }

    fn row(&self) -> Line {
        let (maker, name) = made(&self.title);
        let mut told =
            maker.map(str::to_string).into_iter().collect::<Vec<_>>();
        told.extend(facts(self));
        Line {
            name: name.to_string(),
            price: row_price(self),
            purpose: String::new(),
            meta: told.join(", "),
            mark: Some(badge(self.id())),
        }
    }

    fn page(&self) -> AnyView {
        let (maker, name) = made(&self.title);
        let facts = match maker {
            Some(maker) => format!("By {maker}, {}", self.id),
            None => self.id.clone(),
        };
        let about = match self.description.split("\n\n").next() {
            Some(first) if !first.trim().is_empty() => {
                description(first.trim().to_string()).boxed()
            }
            _ => column(()).boxed(),
        };
        let mut head = vec![page_head(
            name,
            about,
            facts,
            Some(badge(self.id())),
        )];
        if self.tools == Some(false) {
            head.push(problem(
                "This model can't use tools, so agents can only \
                 chat with it.",
            ));
        }
        head.push(deals(self));
        let mut parts =
            vec![top(head), section("DETAILS", details(self))];
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

/// A ware's price on its row: a million tokens in, as wares are
/// ordered by it.
fn row_price(ware: &Ware) -> String {
    match ware.input.zip(ware.output) {
        None => "Varies".into(),
        Some((0.0, 0.0)) => "Free".into(),
        Some((input, _)) => format!("{} in", dollars(input)),
    }
}

/// A ware's price on its page.
fn page_price(ware: &Ware) -> String {
    match ware.input.zip(ware.output) {
        None => "Price varies by provider".into(),
        Some((0.0, 0.0)) => "Free".into(),
        Some((input, output)) => format!(
            "{} in, {} out per million tokens",
            dollars(input),
            dollars(output)
        ),
    }
}

/// The accent line of a ware the home allows, and of the default.
fn badge(id: WareId) -> AnyView {
    keyed(
        derived(move |world: &World| {
            (added(world, &id), is_default(world, &id))
        })
        .when(either(moved::<Catalogued>(), moved::<Configured>())),
        |now| match now {
            (_, true) => mark("Your default model"),
            (true, false) => mark("In your catalogue"),
            (false, false) => column(()).boxed(),
        },
    )
    .boxed()
}

/// What waits while a ware is to be added from OpenRouter and there
/// is no OpenRouter endpoint yet.
fn waiting() -> AnyView {
    let waiting = derived(|world: &World| {
        world.resource::<Buying>().waiting.is_some()
    })
    .when(moved::<Buying>());
    keyed(waiting, |waiting| {
        if !*waiting {
            return column(()).boxed();
        }
        parts::waiting(
            "NO OPENROUTER ENDPOINT YET",
            "OpenRouter's models run through an endpoint of your \
             own, under Endpoints in Settings. Set one up, and the \
             model is added once you have.",
            vec![
                icon_button("signal", "SET UP")
                    .on_click(models::set_up)
                    .boxed(),
                icon_button("sliders", "SETTINGS")
                    .on_click(|_| open(SETTINGS))
                    .boxed(),
                button("CANCEL")
                    .style(ButtonStyle::Quiet)
                    .on_click(|world| {
                        world.resource_mut::<Buying>().waiting = None
                    })
                    .boxed(),
            ],
        )
    })
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

/// The way a ware is had: its price, adding it and making it the
/// default, what adding does, and what came of it.
fn deals(ware: &Ware) -> AnyView {
    let id = ware.id();
    let read = id.clone();
    let price = page_price(ware);
    let now = derived(move |world: &World| {
        (added(world, &read), is_default(world, &read))
    })
    .when(either(moved::<Catalogued>(), moved::<Configured>()));
    let buttons = keyed(now, move |(added, default)| {
        let having = |then: Then| {
            let id = id.clone();
            move |world: &mut World| acquire(world, &id, then)
        };
        let mut buttons = Vec::new();
        if !*added {
            buttons.push(
                icon_button("plus", "ADD")
                    .on_click(having(Then::Keep))
                    .boxed(),
            );
        }
        if !*default {
            buttons.push(
                icon_button("sliders", "MAKE DEFAULT")
                    .on_click(having(Then::MakeDefault))
                    .boxed(),
            );
        }
        let notes = match added {
            true => Vec::new(),
            false => vec![
                description(
                    "Adding it puts it in your catalogue, where you \
                     can give it to any agent.",
                )
                .boxed(),
            ],
        };
        deal(None, buttons, notes)
    });
    deal(
        Some(price),
        Vec::new(),
        vec![buttons.boxed(), waiting(), said::<Ware>()],
    )
}

/// What a ware's page tells of it past its price, a fact a line.
fn details(ware: &Ware) -> AnyView {
    let mut all = Vec::new();
    if let Some(context) = ware.context {
        all.push(fact(
            "Context",
            format!("{} tokens", tokens(context)),
        ));
    }
    if !ware.inputs.is_empty() {
        all.push(fact("Reads", ware.inputs.join(", ")));
    }
    if let Some(tools) = ware.tools {
        all.push(fact(
            "Uses tools",
            if tools { "Yes" } else { "No" },
        ));
    }
    if ware.reasoning == Some(true) {
        all.push(fact("Reasons", "Before it answers"));
    }
    if let Some(index) = ware.intelligence {
        all.push(told(
            fact("Intelligence", format!("{index:.0} of 100")),
            "On Artificial Analysis's index.",
        ));
    }
    if all.is_empty() {
        return note("Nothing more is told of it.").boxed();
    }
    rows(all)
}

/// One provider that serves the ware: its name, how often it is up,
/// and what it charges and holds.
fn serving(serving: &Serving) -> AnyView {
    let mut about = Vec::new();
    if let (Some(input), Some(output)) =
        (serving.input, serving.output)
    {
        about.push(format!(
            "{} in, {} out",
            dollars(input),
            dollars(output)
        ));
    }
    if let Some(context) = serving.context {
        about.push(format!("{} context", tokens(context)));
    }
    if let Some(quantization) = &serving.quantization {
        about.push(quantization.clone());
    }
    let up = serving
        .uptime
        .map(|uptime| format!("{uptime:.1}% up"))
        .unwrap_or_default();
    told(fact(serving.provider.clone(), up), about.join(", "))
}

/// The providers that serve the ware `id` through OpenRouter.
fn providers(id: String) -> AnyView {
    let known = derived(move |world: &World| {
        world.resource::<Providers>().0.get(&id).cloned()
    })
    .when(moved::<Providers>());
    keyed(known, |known| match known {
        None | Some(None) => note("Asking OpenRouter...").boxed(),
        Some(Some(Err(why))) => problem(format!(
            "OpenRouter could not say who serves it: {why}"
        )),
        Some(Some(Ok(servings))) if servings.is_empty() => {
            note("No provider serves it now.").boxed()
        }
        Some(Some(Ok(servings))) => rows(
            servings
                .iter()
                .take(PROVIDERS)
                .map(serving)
                .collect::<Vec<_>>(),
        ),
    })
    .boxed()
}
