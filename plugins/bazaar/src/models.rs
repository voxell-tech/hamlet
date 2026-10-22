//! The Models stall: every model OpenRouter offers and those each
//! other endpoint serves, as the catalogue has them; and a model
//! added to the catalogue, or made the default.

pub(crate) mod openrouter;

use std::cmp::Ordering;
use std::collections::BTreeMap;

use agents_ui::{AgentsWorldExt, FailureExt, Heard};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, CatalogueModel,
    ListedModel, Priced, Settings,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Failure, Reply};
use serde::{Deserialize, Serialize};

use crate::errand::{Errand, ErrandWorldExt, json};
use crate::models::openrouter::Serving;
use crate::shelf::{self, Listing, Said, Shelf, Stock, now};
use crate::{BazaarWorldExt, Stall};

/// The most pages of OpenRouter's models asked for in one go.
const PAGES_MOST: usize = 20;

/// The host in the address of an OpenRouter endpoint.
const OPENROUTER: &str = "openrouter.ai";

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(shelf::plugin::<Ware>)
        .init_resource::<Gathered>()
        .init_resource::<Providers>()
        .init_resource::<Buying>()
        .init_resource::<Configured>()
        .init_resource::<Catalogued>();
}

/// The place a ware is served.
#[derive(
    Clone, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize,
)]
pub(crate) enum Source {
    #[default]
    OpenRouter,
    /// The endpoint of the settings of this name.
    Endpoint(String),
}

/// A ware by where it is served and its id there.
#[derive(
    Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize,
)]
pub(crate) struct WareId {
    pub source: Source,
    pub id: String,
}

/// One model on offer.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Ware {
    pub source: Source,
    /// OpenRouter's id, as `maker/model`, or an endpoint's own.
    pub id: String,
    /// The name it is shown by.
    pub title: String,
    pub description: String,
    /// Its release, in seconds since 1970.
    pub created: Option<u64>,
    /// Dollars for a million tokens in, and out.
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub context: Option<u64>,
    /// What it reads: `text`, `image`, `file`, `audio`, `video`.
    pub inputs: Vec<String>,
    pub tools: Option<bool>,
    pub reasoning: Option<bool>,
    /// Artificial Analysis's index of it.
    pub intelligence: Option<f64>,
}

impl Ware {
    /// True when it reads pictures.
    pub fn sees(&self) -> bool {
        self.inputs.iter().any(|input| input == "image")
    }

    /// It as the catalogue is told of it.
    fn listed(&self) -> ListedModel {
        let note = self
            .description
            .split(['\n', '.'])
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        ListedModel {
            id: self.id.clone(),
            context: self.context,
            input: self.input,
            output: self.output,
            note,
        }
    }

    /// A model of the catalogue that the endpoint `endpoint` serves
    /// as `id`.
    fn served(
        endpoint: &str,
        id: &str,
        model: &CatalogueModel,
    ) -> Ware {
        let priced = model.priced != Priced::Unknown;
        Ware {
            source: Source::Endpoint(endpoint.to_string()),
            id: id.to_string(),
            title: id.to_string(),
            description: model.note.clone(),
            input: priced.then_some(model.input),
            output: priced.then_some(model.output),
            context: model.context,
            ..Ware::default()
        }
    }
}

/// The order wares are shown in.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize,
)]
pub(crate) enum Sort {
    #[default]
    Newest,
    Cheapest,
    Roomiest,
    Smartest,
    Name,
}

/// The filter of the wares besides words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Narrow {
    pub source: Source,
    /// It calls tools, which every agent needs. What nobody said of
    /// a ware does not keep it out.
    pub tools: bool,
    /// The most dollars for a million tokens in. A ware of no known
    /// price is kept out.
    pub most_input: Option<f64>,
}

impl Default for Narrow {
    fn default() -> Self {
        Narrow {
            source: Source::OpenRouter,
            tools: true,
            most_input: None,
        }
    }
}

impl Listing for Ware {
    type Id = WareId;
    type Filter = Narrow;
    type Order = Sort;

    const ORDERS: &'static [(Sort, &'static str)] = &[
        (Sort::Newest, "Newest"),
        (Sort::Cheapest, "Cheapest"),
        (Sort::Roomiest, "Largest context"),
        (Sort::Smartest, "Smartest"),
        (Sort::Name, "Name"),
    ];

    fn id(&self) -> WareId {
        WareId {
            source: self.source.clone(),
            id: self.id.clone(),
        }
    }

    fn told(&self) -> String {
        format!("{} {}", self.id, self.title)
    }

    fn admits(&self, narrow: &Narrow) -> bool {
        self.source == narrow.source
            && (!narrow.tools || self.tools != Some(false))
            && narrow.most_input.is_none_or(|most| {
                self.input.is_some_and(|input| input <= most)
            })
    }

    fn order(&self, other: &Ware, sort: Sort) -> Ordering {
        // Lowest first, with nothing after every number.
        fn rising(v: Option<f64>) -> f64 {
            v.unwrap_or(f64::INFINITY)
        }
        fn falling(v: Option<f64>) -> f64 {
            -v.unwrap_or(f64::NEG_INFINITY)
        }
        fn by(
            a: &Ware,
            b: &Ware,
            key: impl Fn(&Ware) -> f64,
        ) -> Ordering {
            key(a).total_cmp(&key(b))
        }
        let (a, b) = (self, other);
        match sort {
            Sort::Newest => {
                by(a, b, |w| falling(w.created.map(|c| c as f64)))
            }
            Sort::Cheapest => {
                by(a, b, |w| rising(w.input))
                    .then(by(a, b, |w| rising(w.output)))
            }
            Sort::Roomiest => {
                by(a, b, |w| falling(w.context.map(|c| c as f64)))
            }
            Sort::Smartest => by(a, b, |w| falling(w.intelligence)),
            Sort::Name => Ordering::Equal,
        }
        .then_with(|| {
            self.title.to_lowercase().cmp(&other.title.to_lowercase())
        })
    }
}

/// OpenRouter's wares come so far, while the pages after are asked
/// for.
#[derive(Resource, Default)]
struct Gathered(Vec<Ware>);

/// The providers of each ware looked at, by its id. `None` while
/// they are asked for.
#[derive(Resource, Default)]
pub(crate) struct Providers(
    pub BTreeMap<String, Option<Result<Vec<Serving>, String>>>,
);

/// The step after a ware is added.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Then {
    Keep,
    MakeDefault,
}

/// A ware being added, and one that waits for an OpenRouter endpoint
/// to be set up.
#[derive(Resource, Default)]
pub(crate) struct Buying {
    adding: Option<(WareId, Then)>,
    pub waiting: Option<(WareId, Then)>,
}

/// The agents' settings, as last told.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct Configured(pub Option<Settings>);

/// The catalogue, as last told.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct Catalogued(pub Vec<CatalogueModel>);

/// Asks OpenRouter for its wares again, from the first page, and the
/// endpoints for theirs.
pub(crate) fn fetch_wares(world: &mut World) {
    if world.resource::<Stock<Ware>>().asking {
        return;
    }
    world.resource_mut::<Gathered>().0.clear();
    if world.send(openrouter::page(0), Errand::Models(0)) {
        world.resource_mut::<Stock<Ware>>().ask();
    }
}

/// Takes in the page of OpenRouter's wares from `from` on, and asks
/// for the next while the pages are full.
pub(crate) fn came(
    world: &mut World,
    from: usize,
    answer: Result<Reply, Failure>,
) {
    let page = match json(answer, "OpenRouter") {
        Ok(body) => openrouter::read(&body),
        Err(why) => {
            return world
                .resource_mut::<Stock<Ware>>()
                .came(Err(why), now());
        }
    };
    let mut gathered = world.resource_mut::<Gathered>();
    // A page that begins with what came already is the first again,
    // where the places are not heard.
    let again = page.first().is_some_and(|first| {
        gathered.0.iter().any(|ware| ware.id == first.id)
    });
    let full = page.len() >= openrouter::PAGE;
    if !again {
        gathered.0.extend(page);
    }
    let next = from + openrouter::PAGE;
    if full
        && !again
        && next < openrouter::PAGE * PAGES_MOST
        && world.send(openrouter::page(next), Errand::Models(next))
    {
        return;
    }
    let mut wares =
        std::mem::take(&mut world.resource_mut::<Gathered>().0);
    wares.extend(
        world
            .resource::<Stock<Ware>>()
            .items
            .iter()
            .filter(|ware| ware.source != Source::OpenRouter)
            .cloned(),
    );
    world.resource_mut::<Stock<Ware>>().came(Ok(wares), now());
}

/// Puts the wares each endpoint other than OpenRouter serves, as the
/// catalogue has them, on the shelf in place of those before.
fn shelve_served(world: &mut World) {
    let Some(settings) = &world.resource::<Configured>().0 else {
        return;
    };
    let others = settings
        .endpoints
        .iter()
        .filter(|endpoint| !endpoint.url.contains(OPENROUTER))
        .map(|endpoint| endpoint.name.clone())
        .collect::<Vec<_>>();
    let served = world
        .resource::<Catalogued>()
        .0
        .iter()
        .filter_map(|model| {
            let (endpoint, id) = model.name.split_once("::")?;
            others
                .iter()
                .any(|other| other == endpoint)
                .then(|| Ware::served(endpoint, id, model))
        })
        .collect::<Vec<_>>();
    let mut stock = world.resource_mut::<Stock<Ware>>();
    let mut wares = std::mem::take(&mut stock.items);
    wares.retain(|ware| ware.source == Source::OpenRouter);
    wares.extend(served);
    stock.stock(wares);
}

/// Asks OpenRouter which providers serve the ware `id`, unless that
/// is known already.
pub(crate) fn look(world: &mut World, id: &str) {
    if matches!(
        world.resource::<Providers>().0.get(id),
        Some(Some(Ok(_)) | None)
    ) {
        return;
    }
    if world
        .send(openrouter::servings(id), Errand::Servings(id.into()))
    {
        world.resource_mut::<Providers>().0.insert(id.into(), None);
    }
}

/// Takes in who serves the ware `id`.
pub(crate) fn served(
    world: &mut World,
    id: String,
    answer: Result<Reply, Failure>,
) {
    let servings = json(answer, "OpenRouter")
        .map(|body| openrouter::read_servings(&body));
    world
        .resource_mut::<Providers>()
        .0
        .insert(id, Some(servings));
}

/// The endpoint that runs the models of `source`: the endpoint of
/// that name, or for OpenRouter the first at its address.
pub(crate) fn endpoint_of(
    settings: &Settings,
    source: &Source,
) -> Option<String> {
    let endpoint = match source {
        Source::Endpoint(name) => {
            settings.endpoints.iter().find(|e| e.name == *name)
        }
        Source::OpenRouter => settings
            .endpoints
            .iter()
            .find(|endpoint| endpoint.url.contains(OPENROUTER)),
    };
    endpoint.map(|endpoint| endpoint.name.clone())
}

/// The name the ware `id` goes by in the catalogue, once there is an
/// endpoint to run it on.
pub(crate) fn name_of(world: &World, id: &WareId) -> Option<String> {
    let settings = world.resource::<Configured>().0.as_ref()?;
    let endpoint = endpoint_of(settings, &id.source)?;
    Some(format!("{endpoint}::{}", id.id))
}

/// True when the ware `id` is among the models the home allows.
pub(crate) fn added(world: &World, id: &WareId) -> bool {
    name_of(world, id).is_some_and(|name| {
        world
            .resource::<Catalogued>()
            .0
            .iter()
            .any(|model| model.name == name && model.allowed)
    })
}

/// True when the ware `id` is the default model.
pub(crate) fn is_default(world: &World, id: &WareId) -> bool {
    let settings = &world.resource::<Configured>().0;
    name_of(world, id).is_some_and(|name| {
        settings
            .as_ref()
            .is_some_and(|settings| settings.model == name)
    })
}

/// Says `words` at the Models stall.
fn say(world: &mut World, words: impl Into<String>, wrong: bool) {
    world.resource_mut::<Said<Ware>>().say(words, wrong);
}

/// Adds the ware `id` to the catalogue and does `then` with it. With
/// no OpenRouter endpoint yet, it waits for one.
pub(crate) fn acquire(world: &mut World, id: &WareId, then: Then) {
    if world.resource::<Buying>().adding.is_some() {
        return;
    }
    if then == Then::MakeDefault
        && added(world, id)
        && let Some(name) = name_of(world, id)
    {
        return world.ask(AgentsRequest::SetModel(name));
    }
    let Some(settings) = world.resource::<Configured>().0.clone()
    else {
        return say(
            world,
            "The agents have not said where models come from yet.",
            true,
        );
    };
    let Some(endpoint) = endpoint_of(&settings, &id.source) else {
        if id.source != Source::OpenRouter {
            let why = "That endpoint is no longer in Settings.";
            return say(world, why, true);
        }
        world.resource_mut::<Buying>().waiting =
            Some((id.clone(), then));
        return world.resource_mut::<Said<Ware>>().0 = None;
    };
    let Some(ware) = world.resource::<Stock<Ware>>().get(id).cloned()
    else {
        return say(
            world,
            "This model is no longer on the shelf.",
            true,
        );
    };
    world.resource_mut::<Buying>().adding = Some((id.clone(), then));
    say(world, format!("Adding {}...", ware.title), false);
    world.ask(AgentsRequest::AddModel {
        endpoint,
        model: ware.listed(),
    });
}

/// Begins the setup of a place for models, for the ware that waits.
pub(crate) fn set_up(world: &mut World) {
    let adding = world
        .resource::<Configured>()
        .0
        .as_ref()
        .is_some_and(|settings| settings.chosen);
    world.ask(AgentsRequest::BeginSetup { adding });
}

/// Takes in what the agents service says of the settings, the
/// catalogue and the models added.
pub(crate) fn heard(world: &mut World, told: &Heard) {
    match told {
        Heard::Reply(_, AgentsReply::Settings(settings))
        | Heard::Event(AgentsEvent::Settings(settings)) => {
            world.keep(Configured(Some(settings.clone())));
            settled(world);
        }
        Heard::Event(AgentsEvent::Listing(listing)) => {
            if let Some(settings) =
                &mut world.resource_mut::<Configured>().0
            {
                settings.listing = listing.clone();
            }
        }
        Heard::Reply(_, AgentsReply::Catalogue(models)) => {
            world.keep(Catalogued(models.clone()));
            shelve_served(world);
        }
        Heard::Event(AgentsEvent::Catalogue) => {
            world.ask(AgentsRequest::Catalogue);
        }
        Heard::Reply(
            Some(AgentsRequest::AddModel { .. }),
            AgentsReply::Added(name),
        ) => {
            let adding = world.resource_mut::<Buying>().adding.take();
            world.traded(Stall::Models);
            world.ask(AgentsRequest::Catalogue);
            match adding {
                Some((_, Then::MakeDefault)) => {
                    world.ask(AgentsRequest::SetModel(name.clone()));
                }
                Some((id, Then::Keep)) => {
                    let title = world
                        .resource::<Stock<Ware>>()
                        .get(&id)
                        .map_or(name.clone(), |ware| {
                            ware.title.clone()
                        });
                    say(world, format!("Added {title}."), false);
                }
                None => say(world, format!("Added {name}."), false),
            }
        }
        Heard::Failed(Some(AgentsRequest::AddModel { .. }), why) => {
            world.resource_mut::<Buying>().adding = None;
            say(
                world,
                format!("It could not be added: {}", why.plain()),
                true,
            );
        }
        Heard::Reply(
            Some(AgentsRequest::SetModel(name)),
            AgentsReply::Done,
        ) => {
            say(
                world,
                format!("{name} is your default model now."),
                false,
            );
        }
        Heard::Failed(Some(AgentsRequest::SetModel(_)), why) => {
            say(
                world,
                format!(
                    "It could not be made the default: {}",
                    why.plain()
                ),
                true,
            );
        }
        Heard::Failed(
            Some(AgentsRequest::BeginSetup { .. }),
            why,
        ) => {
            say(
                world,
                format!(
                    "Setting up could not begin: {}",
                    why.plain()
                ),
                true,
            );
        }
        _ => {}
    }
}

/// Follows the settings: a source no longer there is let go, and the
/// ware that waited is added once there is an OpenRouter endpoint.
fn settled(world: &mut World) {
    let Some(settings) = world.resource::<Configured>().0.clone()
    else {
        return;
    };
    let source =
        world.resource::<Shelf<Ware>>().filter.source.clone();
    if endpoint_of(&settings, &source).is_none()
        && source != Source::OpenRouter
    {
        world
            .resource_mut::<Shelf<Ware>>()
            .browse(|shelf| shelf.filter.source = Source::OpenRouter);
    }
    shelve_served(world);
    let ready =
        world.resource::<Buying>().waiting.as_ref().is_some_and(
            |(id, _)| endpoint_of(&settings, &id.source).is_some(),
        );
    if ready
        && let Some((id, then)) =
            world.resource_mut::<Buying>().waiting.take()
    {
        acquire(world, &id, then);
    }
}

/// Turns the stall to OpenRouter's models, the shelf shown.
pub(crate) fn browse(world: &mut World) {
    world.resource_mut::<Shelf<Ware>>().browse(|shelf| {
        shelf.filter.source = Source::OpenRouter;
        shelf.open = None;
    });
}
