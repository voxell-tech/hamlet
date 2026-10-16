//! The Agent Bazaar: every model OpenRouter offers and those the
//! other endpoints serve, for the chief to find, compare and add to
//! the home, for an agent or as the default.

mod shelf;
mod ware;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::px;
use bevy_fynix::dock::{DockTree, DockWindowKind};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, scroll};
use bevy_fynix::{ViewExt, derived, keyed, resource};
use hamlet_core::bazaar::Bazaar;
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{
    HomeDir, SaveSettings, Settings, SignIn, Signing, TestConnection,
};
use hamlet_model::bazaar::{Filter, Sort, Ware, is_openrouter};
use hamlet_model::catalogue::{self, Catalogue};
use hamlet_model::config::providers::{OPENROUTER, PROVIDERS};
use hamlet_model::config::{Config, Endpoint};

use crate::dock::{PanelAppExt, panel, show_tab};
use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, action, back, bare, buttons, good_action, note,
    title,
};
use crate::roll::{either, moved};
use crate::{AnyView, Cx, View};

/// The panel's id in the dock.
const PANEL: &str = "bazaar";

/// The rows shown at first, and the more each press of Show More
/// adds.
const SHOWN: usize = 40;

pub(crate) fn plugin(app: &mut App) {
    app.add_panel(
        PANEL,
        DockWindowKind::new("AGENT BAZAAR", || panel(screen()))
            .icon(Glyph::Coins.image(ICON)),
    )
    .init_resource::<Browsing>()
    .add_systems(Update, (signed_in, shelf::read_words));
}

/// What the chief is looking at in the bazaar.
#[derive(Resource)]
pub struct Browsing {
    pub filter: Filter,
    pub sort: Sort,
    /// The endpoint whose models are shown; `None` for OpenRouter.
    pub source: Option<String>,
    /// The most rows shown.
    pub shown: usize,
    /// The ware open on a page of its own, by its id.
    pub open: Option<String>,
    /// The agent the chief came to choose a model for.
    pub for_agent: Option<Entity>,
    /// A ware that waits for the chief to sign in to OpenRouter,
    /// and what is then done with it.
    waiting: Option<(String, Then)>,
    /// What came of the last thing done, and whether it went wrong.
    pub said: Option<(String, bool)>,
}

impl Default for Browsing {
    fn default() -> Self {
        Browsing {
            filter: Filter::default(),
            sort: Sort::default(),
            source: None,
            shown: SHOWN,
            open: None,
            for_agent: None,
            waiting: None,
            said: None,
        }
    }
}

impl Browsing {
    /// The wares of the source shown.
    pub fn wares<'a>(&self, bazaar: &'a Bazaar) -> &'a [Ware] {
        match &self.source {
            None => &bazaar.wares.openrouter,
            Some(endpoint) => bazaar
                .wares
                .served
                .get(endpoint)
                .map_or(&[], Vec::as_slice),
        }
    }

    fn say(&mut self, words: impl Into<String>, wrong: bool) {
        self.said = Some((words.into(), wrong));
    }
}

/// What is done with a ware once it is added.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Then {
    Keep,
    MakeDefault,
    RunFor(Entity),
}

/// Opens the bazaar, to choose a model for `for_agent` where it is
/// some.
pub fn open(world: &mut World, for_agent: Option<Entity>) {
    let mut browsing = world.resource_mut::<Browsing>();
    browsing.for_agent = for_agent;
    browsing.open = None;
    browsing.said = None;
    let mut tree = world.resource_mut::<DockTree>();
    if tree.find_leaf_with_window(PANEL).is_none() {
        // Over the world, which has the most room.
        let leaf = tree
            .find_leaf_with_window("world")
            .or_else(|| tree.leaves().next().map(|(leaf, _)| leaf));
        if let Some(leaf) = leaf {
            tree.add_tab(leaf, PANEL);
        }
    }
    show_tab(world, PANEL);
}

/// Opens the bazaar at the models of the endpoint `source`, or of
/// OpenRouter for `None`.
pub fn open_at(world: &mut World, source: Option<String>) {
    open(world, None);
    let mut browsing = world.resource_mut::<Browsing>();
    browsing.source = source;
    browsing.shown = SHOWN;
}

/// The endpoint the models of `source` are run on: the endpoint of
/// that name, or for `None` the first at OpenRouter.
fn endpoint_of<'a>(
    config: &'a Config,
    source: Option<&str>,
) -> Option<&'a Endpoint> {
    match source {
        Some(name) => config.endpoint(name),
        None => config
            .endpoints
            .iter()
            .find(|endpoint| is_openrouter(&endpoint.base_url)),
    }
}

/// The name the ware `id` of `source` goes by in the catalogue, once
/// there is an endpoint to run it on.
pub(crate) fn name_of(
    config: &Config,
    source: Option<&str>,
    id: &str,
) -> Option<String> {
    endpoint_of(config, source)
        .map(|endpoint| catalogue::named(&endpoint.name, id))
}

/// True when the ware `id` of `source` is among the home's models.
pub(crate) fn added(
    world: &World,
    source: Option<&str>,
    id: &str,
) -> bool {
    let config = &world.resource::<Settings>().config;
    name_of(config, source, id).is_some_and(|name| {
        world
            .resource::<ModelCatalogue>()
            .catalogue
            .get(&name)
            .is_some_and(|model| model.allowed)
    })
}

/// Adds the ware `id` of the source shown to the home's models and
/// does `then` with it. With no OpenRouter endpoint yet, the chief
/// signs in first.
fn acquire(world: &mut World, id: &str, then: Then) {
    let source = world.resource::<Browsing>().source.clone();
    let mut config = world.resource::<Settings>().config.clone();
    let Some(endpoint) =
        endpoint_of(&config, source.as_deref()).cloned()
    else {
        let mut browsing = world.resource_mut::<Browsing>();
        if source.is_some() {
            browsing
                .say("That endpoint is no longer in Settings.", true);
            return;
        }
        browsing.waiting = Some((id.to_string(), then));
        browsing.say(
            "Approve Hamlet in your browser to connect OpenRouter. \
             The model is added once you have.",
            false,
        );
        world.trigger(SignIn::OpenRouter);
        return;
    };
    let ware = world
        .resource::<Bazaar>()
        .wares
        .get(source.as_deref(), id)
        .cloned();
    let Some(ware) = ware else {
        return;
    };
    let home = world.resource::<HomeDir>().0.root.clone();
    let mut all = Catalogue::load(&home);
    let local = catalogue::is_local(&endpoint.base_url);
    let model = all.add(&endpoint.name, &ware.listed(), local);
    if source.is_none() {
        ware.describe(model);
    }
    let name = model.name();
    let saved = all.save(&home);
    world.resource_mut::<ModelCatalogue>().reread();
    if let Err(e) = saved {
        world
            .resource_mut::<Browsing>()
            .say(format!("It could not be added. ({e:#})"), true);
        return;
    }
    // The merchant in the world cheers the sale.
    world.trigger(hamlet_world::bazaar::Traded);
    let done = match then {
        Then::Keep => format!("Added {}.", ware.title),
        Then::MakeDefault => {
            config.model.clone_from(&name);
            world.trigger(SaveSettings(config.clone()));
            world.trigger(TestConnection(config));
            format!("{} is the default model now.", ware.title)
        }
        Then::RunFor(agent) => {
            crate::models::assign(world, agent, &name);
            let who = hamlet_core::agents::name_of(world, agent);
            format!("{who} runs {} now.", ware.title)
        }
    };
    let failed = world.resource::<Settings>().error.clone();
    let mut browsing = world.resource_mut::<Browsing>();
    match failed {
        Some(why) if then == Then::MakeDefault => browsing
            .say(format!("It could not be saved. ({why})"), true),
        _ => browsing.say(done, false),
    }
}

/// Adds the ware that waited once the chief has signed in to
/// OpenRouter, with the key it gave.
fn signed_in(
    signing: Res<Signing>,
    mut browsing: ResMut<Browsing>,
    mut commands: Commands,
) {
    if !signing.is_changed() || browsing.waiting.is_none() {
        return;
    }
    match &*signing {
        Signing::Idle | Signing::Waiting => {}
        Signing::Done(key) => {
            let (id, then) =
                browsing.waiting.take().expect("it waits");
            let key = key.clone();
            commands.queue(move |world: &mut World| {
                let at = &PROVIDERS[OPENROUTER];
                let mut config =
                    world.resource::<Settings>().config.clone();
                config.settle(Endpoint {
                    name: at.endpoint.into(),
                    base_url: at.base_url.into(),
                    chatgpt: false,
                    api_key: Some(key),
                });
                world.trigger(SaveSettings(config));
                if let Some(why) =
                    world.resource::<Settings>().error.clone()
                {
                    world.resource_mut::<Browsing>().say(
                        format!(
                            "OpenRouter could not be saved. ({why})"
                        ),
                        true,
                    );
                    return;
                }
                acquire(world, &id, then);
            });
        }
        Signing::Failed(why) => {
            browsing.waiting = None;
            let why = format!(
                "Signing in to OpenRouter did not finish ({why}). \
                 Try again, or add a key under Endpoints in \
                 Settings."
            );
            browsing.say(why, true);
        }
    }
}

/// The bazaar as the Inspector shows it while it is selected in the
/// world: the models the home bought there, and the way in.
pub(crate) fn stall() -> impl View {
    let bought = derived(|world: &World| {
        let mut bought = world
            .resource::<ModelCatalogue>()
            .catalogue
            .allowed()
            .map(|model| {
                let name = match model.title.is_empty() {
                    true => model.name(),
                    false => model.title.clone(),
                };
                (name, model.card())
            })
            .collect::<Vec<_>>();
        bought.sort();
        let offered =
            world.resource::<Bazaar>().wares.openrouter.len();
        (bought, offered)
    })
    .when(either(moved::<ModelCatalogue>(), moved::<Bazaar>()));
    let list = keyed(bought, |(bought, offered)| {
        let mut rows = vec![
            note(match offered {
                0 => "Nothing from OpenRouter on the shelves yet."
                    .to_string(),
                offered => format!(
                    "{offered} models from OpenRouter on the \
                     shelves. The home has bought {}.",
                    bought.len()
                ),
            })
            .boxed(),
        ];
        rows.extend(bought.iter().map(|(name, card)| {
            crate::lists::item(name.clone(), card.clone(), Tone::Body)
        }));
        column(rows).gap(8.0).boxed()
    })
    .within(bare());
    column((
        title("THE AGENT BAZAAR"),
        buttons(vec![
            back("ALL HAMLETS", |world| {
                hamlet_world::pick::select(world, None)
            })
            .boxed(),
            good_action(Glyph::Coins, "OPEN THE BAZAAR", |world| {
                open(world, None)
            }),
            action(Glyph::Move, "MOVE", |world| {
                hamlet_world::moving::pick_up(
                    world,
                    hamlet_world::generator::society::Place::Bazaar,
                )
            }),
        ]),
        note(
            "Every model the home may run is bought here. The \
             merchant shows you what OpenRouter and your endpoints \
             offer, and cheers each one you add.",
        ),
        scroll((list,)).fills_height(),
    ))
    .fills_height()
    .gap(10.0)
}

/// The bazaar panel's contents: the shelf, or one ware on a page of
/// its own.
pub fn screen() -> AnyView {
    let page = keyed(
        resource::<Browsing, _>(|browsing| browsing.open.clone()),
        |open| match open {
            Some(id) => ware::page(id.clone()),
            None => shelf::page(),
        },
    )
    .within(crate::looks::bare().max_width(px(820.0)));
    AnyView::new(move |cx: &mut Cx| {
        cx.build(scroll((page,)).fills_height())
    })
}

#[cfg(test)]
mod tests {
    use bevy::ecs::observer::On;
    use hamlet_model::bazaar::Wares;
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_ware_is_added_once_there_is_an_openrouter_endpoint_to_run_it()
     {
        let home = Scratch::new("ui-bazaar");
        let mut world = World::new();
        world.insert_resource(HomeDir(hamlet_home::Home {
            root: home.to_path_buf(),
            workspace: home.join("workspace"),
        }));
        world.init_resource::<Browsing>();
        world.init_resource::<ModelCatalogue>();
        world.init_resource::<Signing>();
        world.insert_resource(Settings {
            config: Config::default(),
            error: None,
        });
        world.insert_resource(Bazaar::of(Wares {
            openrouter: vec![Ware {
                id: "maker/large".into(),
                title: "Maker: Large".into(),
                input: Some(3.0),
                output: Some(15.0),
                inputs: vec!["text".into(), "image".into()],
                tools: Some(true),
                ..Ware::default()
            }],
            ..Wares::default()
        }));
        // The core's: here they only keep the settings and wait.
        world.add_observer(
            |save: On<SaveSettings>,
             mut settings: ResMut<Settings>| {
                settings.config = save.event().0.clone();
            },
        );
        world.add_observer(
            |_: On<SignIn>, mut signing: ResMut<Signing>| {
                *signing = Signing::Waiting;
            },
        );

        // With no OpenRouter endpoint, the chief signs in first.
        acquire(&mut world, "maker/large", Then::Keep);
        assert!(Catalogue::load(&home).models.is_empty());
        assert_eq!(*world.resource::<Signing>(), Signing::Waiting);
        *world.resource_mut::<Signing>() =
            Signing::Done("sk-or".into());
        world.run_system_cached(signed_in).unwrap();

        let config = world.resource::<Settings>().config.clone();
        let endpoint = config.endpoint("openrouter").unwrap();
        assert_eq!(endpoint.api_key.as_deref(), Some("sk-or"));
        let all = Catalogue::load(&home);
        let model = all.get("openrouter::maker/large").unwrap();
        assert!(model.allowed);
        assert_eq!(model.title, "Maker: Large");
        assert!(model.card().contains("reads images"));
        assert!(
            !world.resource::<Browsing>().said.as_ref().unwrap().1
        );

        acquire(&mut world, "maker/large", Then::MakeDefault);
        assert_eq!(
            world.resource::<Settings>().config.model,
            "openrouter::maker/large"
        );
        assert_eq!(Catalogue::load(&home).models.len(), 1);
    }
}
