//! The Models stall: every model OpenRouter offers and those each
//! other endpoint serves, as the catalogue has them; and a model
//! allowed in the catalogue, or made the default.

pub(crate) mod openrouter;

use std::cmp::Ordering;
use std::collections::BTreeMap;

use bevy::app::{App, PreUpdate};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Providers, Settings};
use hamlet_protocol::client as protocol;

use crate::PanelSystems;
use crate::bazaar::errand::{self, Errand, Fetched, json};
use crate::bazaar::models::openrouter::Serving;
use crate::bazaar::shelf::{self, Listing, Said, Shelf, Stock, now};
use crate::models::Shelf as Catalogue;

/// The most pages of OpenRouter's models asked for in one go.
const PAGES_MOST: usize = 20;

/// The host in the address of an OpenRouter endpoint.
const OPENROUTER: &str = "openrouter.ai";

/// The provider OpenRouter's sign-in gives the server.
const SIGNED_IN: &str = "openrouter";

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(shelf::plugin::<Ware>)
        .init_resource::<Gathered>()
        .init_resource::<Servings>()
        .init_resource::<Buying>()
        .add_observer(answered)
        .add_systems(
            PreUpdate,
            settled
                .run_if(
                    resource_changed::<Settings>
                        .or_eager(resource_changed::<Providers>)
                        .or_eager(resource_changed::<Catalogue>),
                )
                .in_set(PanelSystems::Follow),
        );
}

/// The place a ware is served.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub(crate) enum Source {
    #[default]
    OpenRouter,
    /// The endpoint of the settings of this name.
    Endpoint(String),
}

/// A ware by where it is served and its id there.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
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

    /// Its note in the catalogue: the first sentence told of it.
    fn note(&self) -> String {
        self.description
            .split(['\n', '.'])
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// A model of the catalogue that the endpoint `endpoint` serves
    /// as `id`.
    fn served(
        endpoint: &str,
        id: &str,
        model: &protocol::Listed,
    ) -> Ware {
        Ware {
            source: Source::Endpoint(endpoint.to_string()),
            id: id.to_string(),
            title: id.to_string(),
            description: model.note.clone(),
            input: model.price.as_ref().map(|price| price.input),
            output: model.price.as_ref().map(|price| price.output),
            context: model
                .context
                .and_then(|c| u64::try_from(c).ok()),
            ..Ware::default()
        }
    }
}

/// The order wares are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Sort {
    #[default]
    Newest,
    Cheapest,
    Roomiest,
    Smartest,
    Name,
}

/// The filter of the wares besides words.
#[derive(Clone, Debug, PartialEq)]
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
pub(crate) struct Servings(
    pub BTreeMap<String, Option<Result<Vec<Serving>, String>>>,
);

/// The step after a ware is allowed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Then {
    Keep,
    MakeDefault,
}

/// What a ware being had waits on the server for.
#[derive(Clone, PartialEq, Debug)]
enum Step {
    /// The catalogue listed again, to find it there.
    Listing(protocol::RequestId),
    /// It allowed in the catalogue.
    Allowing(protocol::RequestId),
    /// It made the default model.
    Defaulting(protocol::RequestId),
}

/// A ware being had, and one that waits for OpenRouter to be set up.
#[derive(Resource, Default)]
pub(crate) struct Buying {
    having: Option<(WareId, Then, Step)>,
    pub waiting: Option<(WareId, Then)>,
}

impl Buying {
    /// True while a ware is on its way to the catalogue.
    pub fn is_busy(&self) -> bool {
        self.having.is_some()
    }
}

/// Asks OpenRouter for its wares again, from the first page.
pub(crate) fn fetch_wares(world: &mut World) {
    if world.resource::<Stock<Ware>>().asking {
        return;
    }
    world.resource_mut::<Gathered>().0.clear();
    if errand::send(world, openrouter::page(0), Errand::Models(0)) {
        world.resource_mut::<Stock<Ware>>().ask();
    }
}

/// Takes in the page of OpenRouter's wares from `from` on, and asks
/// for the next while the pages are full.
pub(crate) fn came(world: &mut World, from: usize, answer: Fetched) {
    let page = match json(answer, "OpenRouter") {
        Ok(body) => openrouter::read(&body),
        Err(why) => {
            return world
                .resource_mut::<Stock<Ware>>()
                .came(Err(why), now());
        }
    };
    let mut gathered = world.resource_mut::<Gathered>();
    // A page that begins with what came already is the first again:
    // OpenRouter did not take the offset asked for.
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
        && errand::send(
            world,
            openrouter::page(next),
            Errand::Models(next),
        )
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
    let others = world
        .resource::<Settings>()
        .0
        .endpoints
        .iter()
        .filter(|endpoint| !endpoint.base_url.contains(OPENROUTER))
        .map(|endpoint| endpoint.name.clone())
        .collect::<Vec<_>>();
    let served = world
        .resource::<Catalogue>()
        .models
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
    let mut wares = stock
        .items
        .iter()
        .filter(|ware| ware.source == Source::OpenRouter)
        .cloned()
        .collect::<Vec<_>>();
    wares.extend(served);
    if wares != stock.items {
        stock.stock(wares);
    }
}

/// Asks OpenRouter which providers serve the ware `id`, unless that
/// is known already.
pub(crate) fn look(world: &mut World, id: &str) {
    if matches!(
        world.resource::<Servings>().0.get(id),
        Some(Some(Ok(_)) | None)
    ) {
        return;
    }
    if errand::send(
        world,
        openrouter::servings(id),
        Errand::Servings(id.into()),
    ) {
        world.resource_mut::<Servings>().0.insert(id.into(), None);
    }
}

/// Takes in who serves the ware `id`.
pub(crate) fn served(world: &mut World, id: String, answer: Fetched) {
    let servings = json(answer, "OpenRouter")
        .map(|body| openrouter::read_servings(&body));
    world
        .resource_mut::<Servings>()
        .0
        .insert(id, Some(servings));
}

/// The name the models of `source` go by in the catalogue before
/// `::`: the endpoint of that name, or for OpenRouter its sign-in or
/// else the first endpoint at its address.
pub(crate) fn endpoint_of(
    world: &World,
    source: &Source,
) -> Option<String> {
    let endpoints = &world.resource::<Settings>().0.endpoints;
    match source {
        Source::Endpoint(name) => endpoints
            .iter()
            .find(|endpoint| endpoint.name == *name)
            .map(|endpoint| endpoint.name.clone()),
        Source::OpenRouter => {
            let signed_in = world
                .resource::<Providers>()
                .0
                .iter()
                .any(|provider| {
                    provider.id == SIGNED_IN && provider.configured
                });
            match signed_in {
                true => Some(SIGNED_IN.to_string()),
                false => endpoints
                    .iter()
                    .find(|endpoint| {
                        endpoint.base_url.contains(OPENROUTER)
                    })
                    .map(|endpoint| endpoint.name.clone()),
            }
        }
    }
}

/// The name the ware `id` goes by in the catalogue, once there is an
/// endpoint to run it on.
pub(crate) fn name_of(world: &World, id: &WareId) -> Option<String> {
    let endpoint = endpoint_of(world, &id.source)?;
    Some(format!("{endpoint}::{}", id.id))
}

/// True when the ware `id` is among the models the home allows.
pub(crate) fn added(world: &World, id: &WareId) -> bool {
    name_of(world, id).is_some_and(|name| {
        world
            .resource::<Catalogue>()
            .models
            .iter()
            .any(|model| model.name == name && model.allowed)
    })
}

/// True when the ware `id` is the default model.
pub(crate) fn is_default(world: &World, id: &WareId) -> bool {
    name_of(world, id).is_some_and(|name| {
        world.resource::<Settings>().0.model == name
    })
}

/// Says `words` at the Models stall.
fn say(world: &mut World, words: impl Into<String>, wrong: bool) {
    world.resource_mut::<Said<Ware>>().say(words, wrong);
}

/// The name a ware is shown by in what is said of it.
fn title_of(world: &World, id: &WareId) -> String {
    world
        .resource::<Stock<Ware>>()
        .get(id)
        .map_or_else(|| id.id.clone(), |ware| ware.title.clone())
}

/// Allows the ware `id` in the catalogue and does `then` with it.
/// With no OpenRouter sign-in or endpoint yet, it waits for one.
pub(crate) fn acquire(world: &mut World, id: &WareId, then: Then) {
    if world.resource::<Buying>().is_busy() {
        return;
    }
    let Some(name) = name_of(world, id) else {
        if id.source != Source::OpenRouter {
            let why = "That endpoint is no longer in Settings.";
            return say(world, why, true);
        }
        world.resource_mut::<Buying>().waiting =
            Some((id.clone(), then));
        world.resource_mut::<Said<Ware>>().0 = None;
        return;
    };
    if then == Then::MakeDefault && added(world, id) {
        return make_default(world, id, &name);
    }
    if world.resource::<Stock<Ware>>().get(id).is_none() {
        return say(
            world,
            "This model is no longer on the shelf.",
            true,
        );
    }
    let title = title_of(world, id);
    say(world, format!("Adding {title}..."), false);
    let listed = world
        .resource::<Catalogue>()
        .models
        .iter()
        .any(|model| model.name == name);
    let step = match listed {
        true => Step::Allowing(allow(world, id, &name)),
        // A model the server has not listed yet is listed first.
        false => {
            Step::Listing(world.resource_mut::<Outbox>().send(|id| {
                protocol::ClientMessage::Read(
                    protocol::Read::Models {
                        id,
                        refresh: Some(true),
                    },
                )
            }))
        }
    };
    world.resource_mut::<Buying>().having =
        Some((id.clone(), then, step));
}

/// Asks the server to allow `name`, the ware `id`, with its note.
fn allow(
    world: &mut World,
    id: &WareId,
    name: &str,
) -> protocol::RequestId {
    let note = world
        .resource::<Stock<Ware>>()
        .get(id)
        .map(Ware::note)
        .filter(|note| !note.is_empty());
    world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Catalogue {
            id,
            model: name.to_string(),
            set: protocol::EditCatalogueSet {
                allowed: Some(true),
                note,
                price: None,
            },
        })
    })
}

/// Makes `name`, the ware `id`, the default model.
fn make_default(world: &mut World, id: &WareId, name: &str) {
    let request = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Settings {
            id,
            set: protocol::EditSettingsSet {
                model: Some(name.to_string()),
                endpoints: None,
                reflect_every: None,
                allow_net: None,
                workspace: None,
            },
        })
    });
    world.resource_mut::<Buying>().having = Some((
        id.clone(),
        Then::MakeDefault,
        Step::Defaulting(request),
    ));
}

/// Begins the setup of a place for models, for the ware that waits.
pub(crate) fn set_up(world: &mut World) {
    crate::welcome::add(world);
}

/// Takes in the server's answers to what the stall asked.
fn answered(response: On<Response>, mut commands: Commands) {
    let response = response.event().clone();
    commands.queue(move |world: &mut World| {
        let Some((id, then, step)) =
            world.resource::<Buying>().having.clone()
        else {
            return;
        };
        let (Step::Listing(request)
        | Step::Allowing(request)
        | Step::Defaulting(request)) = &step;
        if *request != response.to {
            return;
        }
        let refused = match &response.message {
            protocol::ServerMessage::Fault(fault) => {
                Some(fault.text.clone())
            }
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                Some(reply.text.clone())
            }
            _ => None,
        };
        world.resource_mut::<Buying>().having = None;
        let title = title_of(world, &id);
        if let Some(why) = refused {
            let words = match step {
                Step::Defaulting(_) => {
                    format!("It could not be made the default: {why}")
                }
                _ => format!("It could not be added: {why}"),
            };
            return say(world, words, true);
        }
        let Some(name) = name_of(world, &id) else {
            return say(
                world,
                "That endpoint is no longer in Settings.",
                true,
            );
        };
        match step {
            Step::Listing(_) => {
                if let protocol::ServerMessage::Read(
                    protocol::ReadResult::Models { models, .. },
                ) = response.message
                {
                    world.resource_mut::<Catalogue>().models = models;
                }
                let listed = world
                    .resource::<Catalogue>()
                    .models
                    .iter()
                    .any(|model| model.name == name);
                if !listed {
                    let why = format!(
                        "The server does not list {title} as \
                         `{name}`. Check the sign-in or the \
                         endpoint in Settings."
                    );
                    return say(world, why, true);
                }
                let request = allow(world, &id, &name);
                world.resource_mut::<Buying>().having =
                    Some((id, then, Step::Allowing(request)));
            }
            Step::Allowing(_) => {
                crate::bazaar::traded(world);
                crate::models::read_models(world, false);
                match then {
                    Then::MakeDefault => {
                        make_default(world, &id, &name);
                    }
                    Then::Keep => {
                        say(world, format!("Added {title}."), false);
                    }
                }
            }
            Step::Defaulting(_) => say(
                world,
                format!("{title} is your default model now."),
                false,
            ),
        }
    });
}

/// Follows the settings and the catalogue: a source no longer there
/// is let go, the endpoints' wares are shelved, and the ware that
/// waited is added once there is somewhere to run it.
fn settled(world: &mut World) {
    let source =
        world.resource::<Shelf<Ware>>().filter.source.clone();
    if source != Source::OpenRouter
        && endpoint_of(world, &source).is_none()
    {
        world
            .resource_mut::<Shelf<Ware>>()
            .browse(|shelf| shelf.filter.source = Source::OpenRouter);
    }
    shelve_served(world);
    let ready =
        world.resource::<Buying>().waiting.as_ref().is_some_and(
            |(id, _)| endpoint_of(world, &id.source).is_some(),
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
