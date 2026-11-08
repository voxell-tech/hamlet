//! First-run setup through the server's settings, models and logins.

mod pages;
mod providers;

use std::collections::BTreeMap;

use bevy::app::{App, PostUpdate, PreUpdate, Update};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::{InputFocus, InputFocusSystems};
use hamlet_client::Connection;
use hamlet_client::connection::Contact;
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Seen, Settings};
use hamlet_protocol::client as protocol;

pub use self::pages::view;
pub use self::providers::{Browser, search};
use crate::PanelSystems;

/// What the local-model search says when nothing answered.
pub const NOTHING_LOCAL: &str = "Nothing answered yet. Install \
                                 Ollama from ollama.com, download a \
                                 model, then choose Look again.";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Welcome>()
        .init_resource::<Committing>()
        .init_resource::<Browser>()
        .init_resource::<providers::Login>()
        .add_observer(answered)
        .add_observer(|_: On<Reconnected>, mut commands: Commands| {
            commands.queue(providers::reconnected);
        })
        .add_systems(PreUpdate, follow.in_set(PanelSystems::Follow))
        .add_plugins(key_plugin)
        // After Bevy's text input, in `PreUpdate`, has edited the
        // field.
        // After Bevy tells the field commit() unfocused that it lost
        // focus, and fynix's observer hands its text to the page, so
        // a commit replies in the frame it was asked for.
        .add_systems(
            PostUpdate,
            run_committed
                .run_if(|pending: Res<Committing>| {
                    pending.is_pending()
                })
                .after(InputFocusSystems::FocusChangeEvents),
        );
}

/// The key page's keys, its focus and the provider it scrolls to,
/// and the hold on sending a key while its field is empty or holds
/// the key already refused.
pub fn key_plugin(app: &mut App) {
    app.add_observer(providers::search_keys).add_systems(
        Update,
        (
            providers::hold_send,
            providers::focus_search,
            providers::reveal_chosen,
        ),
    );
}

/// A page of first-run setup.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Step {
    #[default]
    Choice,
    Key,
    SignIn,
    Local,
    Models,
}

/// The first-run page and its received answers.
#[derive(Resource, Default)]
pub struct Welcome {
    pub open: bool,
    pub adding: bool,
    pub step: Step,
    pub asking: bool,
    pub from: String,
    pub models: Vec<String>,
    pub problem: Option<String>,
    connection: Option<String>,
    narrow: String,
    /// The model picked in the list, to be tried on Continue.
    chosen: Option<String>,
    manual: bool,
    trying: bool,
    requests: BTreeMap<protocol::RequestId, Asked>,
}

impl Welcome {
    /// Whether first-run setup, or the screen that asks for a server,
    /// covers the window.
    pub fn is_shown(&self) -> bool {
        self.open || self.connection.is_some()
    }
}

#[derive(Clone)]
enum Asked {
    Models,
    Check(bool),
    Pick,
}

/// A pending field commit before a button's action.
#[derive(Resource, Default)]
pub(crate) struct Committing(bool);

impl Committing {
    pub(crate) fn is_pending(&self) -> bool {
        self.0
    }
}

fn run_committed(world: &mut World) {
    world.resource_mut::<Committing>().0 = false;
    providers::reply(world);
}

fn commit(world: &mut World) {
    if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.clear();
    }
    world.resource_mut::<Committing>().0 = true;
}

/// The setup screen from its first page.
pub fn again(world: &mut World) {
    close(world);
    *world.resource_mut::<Welcome>() = Welcome {
        open: true,
        manual: true,
        ..Default::default()
    };
}

/// `key` typed for `provider` on the key page, as the chief types
/// it.
pub fn type_key(world: &mut World, provider: &str, key: &str) {
    let mut login = world.get_resource_or_init::<providers::Login>();
    login.provider = Some(provider.into());
    login.answer = key.into();
}

/// Setup for another provider, keeping the current model.
pub fn add(world: &mut World) {
    again(world);
    world.resource_mut::<Welcome>().adding = true;
}

fn close(world: &mut World) {
    providers::cancel(world);
    *world.resource_mut::<Welcome>() = Welcome::default();
}

fn go(world: &mut World, step: Step) {
    providers::cancel(world);
    let mut welcome = world.resource_mut::<Welcome>();
    welcome.step = step;
    welcome.asking = false;
    welcome.problem = None;
    welcome.narrow.clear();
    welcome.chosen = None;
    welcome.requests.clear();
}

fn read_models(world: &mut World, refresh: bool) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Models {
            id,
            refresh: Some(refresh),
        })
    });
    let mut welcome = world.resource_mut::<Welcome>();
    welcome.requests.insert(id, Asked::Models);
    welcome.asking = true;
}

fn check(world: &mut World, model: bool) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Check(if model {
            protocol::Check::Model { id, model: None }
        } else {
            protocol::Check::Connection { id }
        })
    });
    world
        .resource_mut::<Welcome>()
        .requests
        .insert(id, Asked::Check(model));
}

fn look(world: &mut World) {
    go(world, Step::Local);
    world.resource_mut::<Welcome>().from =
        "The server's endpoints".into();
    check(world, false);
    read_models(world, true);
}

fn pick(world: &mut World, model: &str) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Settings {
            id,
            set: protocol::EditSettingsSet {
                model: Some(model.into()),
                allow_net: None,
                character: None,
                effort: None,
                newcomers: None,
                reflect_every: None,
                endpoints: None,
                workspace: None,
            },
        })
    });
    let mut welcome = world.resource_mut::<Welcome>();
    welcome.requests.insert(id, Asked::Pick);
    welcome.manual = true;
    welcome.trying = true;
    welcome.asking = true;
    welcome.problem = None;
}

fn answered(response: On<Response>, mut commands: Commands) {
    let response = response.event().clone();
    commands.queue(move |world: &mut World| {
        if providers::receive(world, &response)
            == providers::Received::Login
        {
            return;
        }
        let asked = world
            .resource_mut::<Welcome>()
            .requests
            .remove(&response.to);
        let Some(asked) = asked else { return };
        let failure = match &response.message {
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
        if let Some(problem) = failure {
            let mut welcome = world.resource_mut::<Welcome>();
            welcome.problem = Some(problem);
            welcome.asking = false;
            welcome.trying = false;
            return;
        }
        match (asked, response.message) {
            (
                Asked::Models,
                protocol::ServerMessage::Read(
                    protocol::ReadResult::Models { models, .. },
                ),
            ) => {
                let filter = world
                    .resource::<providers::Login>()
                    .provider
                    .clone();
                let local =
                    world.resource::<Welcome>().step == Step::Local;
                // The rows read the models' prices from the
                // catalogue, which this answer is the newest of.
                if let Some(mut shelf) =
                    world.get_resource_mut::<crate::models::Shelf>()
                {
                    shelf.models.clone_from(&models);
                }
                let endpoints =
                    &world.resource::<Settings>().0.endpoints;
                let names = models
                    .into_iter()
                    .map(|model| model.name)
                    .filter(|name| {
                        if local {
                            endpoints.iter().any(|at| {
                                name.starts_with(&format!(
                                    "{}::",
                                    at.name
                                ))
                            })
                        } else {
                            filter.as_ref().is_none_or(|id| {
                                name.starts_with(&format!("{id}::"))
                            })
                        }
                    })
                    .collect::<Vec<_>>();
                let mut welcome = world.resource_mut::<Welcome>();
                welcome.asking = false;
                welcome.models = names;
                if welcome.models.is_empty() {
                    welcome.problem = Some(if local {
                        NOTHING_LOCAL.into()
                    } else {
                        "No models were listed for this provider."
                            .into()
                    });
                } else if welcome.adding {
                    if let Some(mut pane) = world
                        .get_resource_mut::<crate::settings::Pane>()
                    {
                        *pane = crate::settings::Pane::Models;
                    }
                    close(world);
                } else {
                    welcome.step = Step::Models;
                }
            }
            (Asked::Pick, protocol::ServerMessage::Reply(_)) => {
                check(world, true);
            }
            (
                Asked::Check(model),
                protocol::ServerMessage::Reply(_),
            ) => {
                let settings = &world.resource::<Settings>().0;
                let finished = model
                    && !settings.unset
                    && settings.link.state
                        == protocol::LinkState::Connected;
                let link = &settings.link;
                let problem = (link.state
                    == protocol::LinkState::Failed)
                    .then(|| link.words.clone())
                    .flatten();
                let mut welcome = world.resource_mut::<Welcome>();
                welcome.asking = false;
                if model {
                    welcome.trying = false;
                }
                if finished {
                    welcome.open = false;
                }
                if problem.is_some() {
                    welcome.problem = problem;
                }
            }
            _ => {}
        }
    });
}

/// Setup visibility and the server's connection words.
fn follow(
    settings: Res<Settings>,
    seen: Option<Res<Seen>>,
    connection: Res<Connection>,
    mut welcome: ResMut<Welcome>,
    mut decided: Local<bool>,
) {
    use bevy::ecs::change_detection::DetectChanges;
    let words = (connection.state != Contact::Connected)
        .then(|| connection.words.clone());
    if welcome.connection != words {
        welcome.connection = words;
    }
    // Only the first state opens setup; after it, only the chief
    // does.
    if seen.is_some() && !*decided {
        *decided = true;
        welcome.open |= settings.0.unset;
    }
    if settings.is_changed() {
        if !settings.0.unset
            && (!welcome.manual
                || !welcome.trying
                    && settings.0.link.state
                        == protocol::LinkState::Connected)
        {
            welcome.open = false;
        }
        if settings.0.link.state == protocol::LinkState::Failed
            && (welcome.asking
                || welcome.manual && welcome.step == Step::Models)
        {
            welcome.problem.clone_from(&settings.0.link.words);
            welcome.asking = false;
            welcome.trying = false;
            if welcome.step == Step::Models {
                welcome.open = true;
            }
        }
    }
}

#[cfg(test)]
mod server_tests {
    use bevy::color::Alpha;
    use bevy::ecs::entity::Entity;
    use bevy::ecs::hierarchy::{ChildOf, Children};
    use bevy::ecs::query::With;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
    use bevy::text::EditableText;
    use bevy::ui::widget::Text;
    use bevy::window::{PrimaryWindow, Window};
    use bevy_fynix::{FynixPlugin, mount};
    use hamlet_client::ClientPlugin;
    use hamlet_protocol::client as protocol;
    use hamlet_ui::theme::{self, HamletTheme};

    use super::*;
    use crate::testing::{
        all_words, connect, incoming, press, press_and_update, sent,
        welcome_app, words_and_spans,
    };

    #[test]
    fn received_settings_and_connection_choose_the_welcome_page() {
        let mut app = welcome_app();
        connect(&mut app, true);
        mount::<HamletTheme>(app.world_mut(), view());
        assert!(all_words(&mut app).contains("Welcome to Hamlet"));
        connect(&mut app, false);
        assert!(!all_words(&mut app).contains("Welcome to Hamlet"));
        app.world_mut().insert_resource(Connection {
            state: Contact::Refused,
            words: "The server refused this window: bad token."
                .into(),
        });
        app.update();
        let shown = all_words(&mut app);
        assert!(shown.contains("bad token"));
        for command in
            ["hamlet server", "hamlet window", "hamlet dev"]
        {
            assert!(shown.contains(command), "{shown}");
        }
    }
    #[test]
    fn a_later_state_does_not_reopen_the_welcome_page() {
        let mut app = welcome_app();
        connect(&mut app, true);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "I'll set this up later");
        let mut state = hamlet_testing::realm::state();
        state.settings.unset = true;
        // Differs from the held settings, so they read as changed.
        state.settings.link.words = Some("Changed".into());
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert!(!all_words(&mut app).contains("Welcome to Hamlet"));
    }
    fn providers(app: &mut App) {
        incoming(
            app,
            serde_json::json!({"type":"change", "epoch":1, "revision":1, "changes":[{"type":"providers", "providers":[
                {"id":"openrouter", "name":"OpenRouter", "label":"Sign in with OpenRouter", "configured":false, "methods":["key","subscription"]},
                {"id":"chatgpt", "name":"ChatGPT", "label":"Continue with ChatGPT", "configured":false, "methods":["subscription"]}
            ]}]}),
        );
    }

    /// `root` and everything under it.
    fn under(world: &World, root: Entity) -> Vec<Entity> {
        let mut all = vec![root];
        let mut at = 0;
        while let Some(&node) = all.get(at) {
            if let Some(children) = world.get::<Children>(node) {
                all.extend(children.iter());
            }
            at += 1;
        }
        all
    }

    /// The text of the key page's field.
    fn key_field(app: &mut App) -> Entity {
        let world = app.world_mut();
        let field = world
            .query_filtered::<Entity, With<providers::KeyField>>()
            .single(world)
            .expect("one key field");
        under(world, field)
            .into_iter()
            .find(|part| world.get::<EditableText>(*part).is_some())
            .expect("the key field's text")
    }

    /// Types `key` in the key page's field and presses Continue.
    fn send_key(app: &mut App, key: &str) {
        let input = key_field(app);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, bevy::input_focus::FocusCause::Pressed);
        app.update();
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text(key);
        // Continue takes a press once the field has text, and the
        // text is committed and sent in the same frame.
        app.update();
        press(app, "Continue");
        app.update();
    }

    #[test]
    fn continue_waits_for_a_key_to_be_typed() {
        let mut app = welcome_app();
        connect(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "I have a key");
        app.update();
        let held = |app: &mut App| {
            app.world_mut()
                .query_filtered::<(), (
                    With<providers::KeySend>,
                    With<bevy::ui::InteractionDisabled>,
                )>()
                .iter(app.world())
                .count()
                == 1
        };
        assert!(held(&mut app), "held while the field is empty");
        let input = key_field(&mut app);
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text("fake-key");
        app.update();
        assert!(!held(&mut app), "free once a key is typed");
    }

    #[test]
    fn a_providers_key_uses_the_servers_secret_prompt_and_answers() {
        let mut app = welcome_app();
        connect(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "I have a key");
        // The field, its provider and the button are there before
        // anything is sent.
        assert!(sent(&mut app).is_empty());
        press_and_update(&mut app, "OpenRouter");
        let input = key_field(&mut app);
        let ink = app
            .world()
            .get::<bevy::text::TextColor>(input)
            .expect("secret field")
            .0;
        assert_eq!(ink.alpha(), 0.0);
        send_key(&mut app, "fake-key");
        let requests = sent(&mut app);
        let start = &requests[0];
        assert_eq!(start["type"], "login");
        assert_eq!(
            start["start"],
            serde_json::json!({"provider":"openrouter", "method":"key"})
        );
        // The key typed answers the server's secret prompt.
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"key-login", "step":{"prompt":"Paste your key", "secret":true}}),
        );
        let requests = sent(&mut app);
        assert_eq!(
            requests[0]["reply"],
            serde_json::json!({"login":"key-login", "value":"fake-key"})
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"key-login", "step":{"failed":"That key was refused."}}),
        );
        let shown = all_words(&mut app);
        assert!(shown.contains("That key was refused."));
        assert!(!shown.contains("fake-key"));
        // Tried again, with the same provider.
        send_key(&mut app, "other-key");
        let requests = sent(&mut app);
        assert_eq!(
            requests.last().unwrap()["start"],
            serde_json::json!({"provider":"openrouter", "method":"key"})
        );
    }

    /// The name on the provider lit in the key page's list.
    fn lit(app: &mut App) -> String {
        let world = app.world_mut();
        let row = world
            .query_filtered::<Entity, With<providers::ChosenProvider>>()
            .single(world)
            .expect("one provider lit");
        under(world, row)
            .into_iter()
            .find_map(|part| world.get::<Text>(part))
            .map(|text| text.0.clone())
            .expect("its name")
    }

    fn key(app: &mut App, window: Entity, code: KeyCode, key: Key) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: code,
                logical_key: key.clone(),
                state,
                text: None,
                repeat: false,
                window,
            });
            app.update();
        }
    }

    #[test]
    fn the_providers_search_has_the_focus_and_its_keys_choose_from_the_popular_first()
     {
        let mut app = welcome_app();
        app.add_plugins(bevy::input_focus::InputDispatchPlugin);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        connect(&mut app, true);
        incoming(
            &mut app,
            serde_json::json!({"type":"change", "epoch":1, "revision":1, "changes":[{"type":"providers", "providers":[
                {"id":"amazon-bedrock", "name":"Amazon Bedrock", "label":"Amazon Bedrock", "configured":false, "methods":["key"]},
                {"id":"anthropic", "name":"Anthropic", "label":"Anthropic", "configured":false, "methods":["key"]},
                {"id":"chatgpt", "name":"ChatGPT", "label":"Continue with ChatGPT", "configured":false, "methods":["subscription"]},
                {"id":"deepseek", "name":"DeepSeek", "label":"DeepSeek", "configured":false, "methods":["key"]},
                {"id":"openrouter", "name":"OpenRouter", "label":"Sign in with OpenRouter", "configured":false, "methods":["key","subscription"]}
            ]}]}),
        );
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "I have a key");
        app.update();
        let focused = app.world().resource::<InputFocus>().get();
        let in_search = focused.is_some_and(|at| {
            let world = app.world();
            std::iter::successors(Some(at), |at| {
                world.get::<ChildOf>(*at).map(ChildOf::parent)
            })
            .any(|up| {
                world.get::<providers::ProviderSearch>(up).is_some()
            })
        });
        assert!(in_search, "the search has the focus on opening");
        assert_eq!(lit(&mut app), "OpenRouter");
        let mut walked = Vec::new();
        for _ in 0..4 {
            key(&mut app, window, KeyCode::ArrowDown, Key::ArrowDown);
            walked.push(lit(&mut app));
        }
        assert_eq!(
            walked,
            ["Anthropic", "Amazon Bedrock", "DeepSeek", "DeepSeek"]
        );
        key(&mut app, window, KeyCode::ArrowUp, Key::ArrowUp);
        assert_eq!(lit(&mut app), "Amazon Bedrock");
        // A search that hides the provider chosen chooses what it
        // finds, and one that finds nothing keeps it.
        search(app.world_mut(), "deep");
        app.update();
        assert_eq!(lit(&mut app), "DeepSeek");
        search(app.world_mut(), "nothing like it");
        app.update();
        assert!(all_words(&mut app).contains("No providers found"));
        search(app.world_mut(), "");
        app.update();
        assert_eq!(lit(&mut app), "DeepSeek");
        key(&mut app, window, KeyCode::Enter, Key::Enter);
        let field = key_field(&mut app);
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(field),
            "Enter moves to the key's field"
        );
    }

    #[derive(Resource, Default)]
    struct Opened(Vec<String>);

    #[test]
    fn subscription_steps_open_the_browser_show_codes_and_cancel_on_the_server()
     {
        let mut app = welcome_app();
        app.init_resource::<Opened>();
        app.world_mut().insert_resource(Browser(|world, url| {
            world.resource_mut::<Opened>().0.push(url.into());
        }));
        connect(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "Continue with ChatGPT");
        let start = sent(&mut app).remove(0);
        assert_eq!(
            start["start"],
            serde_json::json!({"provider":"chatgpt", "method":"subscription"})
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"plan-login", "step":{"url":"https://login.example/authorize", "instructions":"Allow Hamlet"}}),
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"plan-login", "step":{"code":"ABCD-EFGH"}}),
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"plan-login", "step":{"progress":"Waiting for approval"}}),
        );
        let shown = all_words(&mut app);
        for text in [
            "https://login.example/authorize",
            "ABCD-EFGH",
            "Allow Hamlet",
            "Waiting for approval",
        ] {
            assert!(shown.contains(text), "{shown}");
        }
        assert_eq!(
            app.world().resource::<Opened>().0,
            ["https://login.example/authorize"]
        );
        press_and_update(&mut app, "Cancel sign-in");
        assert_eq!(sent(&mut app)[0]["cancel"], "plan-login");
        press_and_update(&mut app, "Sign in with OpenRouter");
        assert_eq!(
            sent(&mut app)[0]["start"],
            serde_json::json!({"provider":"openrouter", "method":"subscription"})
        );
    }

    #[test]
    fn login_urls_open_only_https_or_loopback_http_and_always_show_the_steps_words()
     {
        let mut app = welcome_app();
        app.init_resource::<Opened>().insert_resource(Browser(
            |world, url| {
                world.resource_mut::<Opened>().0.push(url.into());
            },
        ));
        connect(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "Continue with ChatGPT");
        let start = sent(&mut app).remove(0);
        for url in [
            "file:///Applications/Calculator.app",
            "vscode://file/x",
            "mailto:chief@example.com",
            "http://remote.example/login",
            "http://localhost.evil/login",
            "http://localhost@remote.example/login",
            "https://login.example/authorize",
            "http://localhost:1455/login",
            "http://127.0.0.1:1455/login",
            "http://[::1]:1455/login",
        ] {
            incoming(
                &mut app,
                serde_json::json!({"type":"login", "to":start["id"], "login":"plan-login", "step":{"url":url, "instructions":"Follow these steps yourself."}}),
            );
            let shown = all_words(&mut app);
            assert!(shown.contains(url), "{shown}");
            assert!(shown.contains("Follow these steps yourself."));
        }
        assert_eq!(
            app.world().resource::<Opened>().0,
            [
                "https://login.example/authorize",
                "http://localhost:1455/login",
                "http://127.0.0.1:1455/login",
                "http://[::1]:1455/login"
            ]
        );
    }

    #[test]
    fn local_search_lists_the_servers_models_and_checks_a_saved_choice()
     {
        let mut app = welcome_app();
        connect(&mut app, true);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "I run my own model");
        let requests = sent(&mut app);
        assert_eq!(requests[0]["what"], "connection");
        assert_eq!(requests[1]["what"], "models");
        assert_eq!(requests[1]["refresh"], true);
        incoming(
            &mut app,
            serde_json::json!({"type":"read", "to":requests[1]["id"], "models":[]}),
        );
        // Set as a paragraph of spans, for its link.
        assert!(words_and_spans(&mut app).iter().any(|words| {
            words.contains("Nothing answered yet. Install Ollama")
        }));
        press_and_update(&mut app, "Look again");
        let requests = sent(&mut app);
        app.world_mut().resource_mut::<Settings>().0.endpoints.push(
            protocol::SettingsViewEndpoints {
                name: "local".into(),
                base_url: "http://localhost:11434/v1".into(),
                key: false,
            },
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"read", "to":requests[1]["id"], "models":[
                {"name":"local::small", "price":null, "priced":"unknown", "context":null, "defaultEffort":null, "efforts":[], "fast":false, "note":""},
                {"name":"unconfigured::far", "price":null, "priced":"unknown", "context":null, "defaultEffort":null, "efforts":[], "fast":false, "note":""}
            ]}),
        );
        assert!(!all_words(&mut app).contains("unconfigured::far"));
        press_and_update(&mut app, "small");
        press_and_update(&mut app, "Continue");
        let edit = sent(&mut app).remove(0);
        assert_eq!(edit["doc"], "settings");
        assert_eq!(
            edit["set"],
            serde_json::json!({"model":"local::small"})
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":edit["id"], "kind":"notice", "text":"Saved."}),
        );
        let check = sent(&mut app).remove(0);
        assert_eq!(check["what"], "model");
        app.world_mut().resource_mut::<Settings>().0.unset = false;
        app.update();
        assert!(all_words(&mut app).contains("Pick a model"));
        app.world_mut().resource_mut::<Settings>().0.link.state =
            protocol::LinkState::Connected;
        app.update();
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":check["id"], "kind":"notice", "text":"Hello"}),
        );
        assert!(!all_words(&mut app).contains("Pick a model"));
    }

    #[test]
    fn completed_login_moves_to_its_own_models_and_a_refused_edit_stays_open()
     {
        let mut app = welcome_app();
        connect(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "I have a key");
        press_and_update(&mut app, "OpenRouter");
        send_key(&mut app, "fake-key");
        let start = sent(&mut app).remove(0);
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"login", "step":{"done":"openrouter"}}),
        );
        let read = sent(&mut app).remove(0);
        assert_eq!(read["what"], "models");
        incoming(
            &mut app,
            serde_json::json!({"type":"read", "to":read["id"], "models":[
                {"name":"openrouter::small", "price":null, "priced":"unknown", "context":null, "defaultEffort":null, "efforts":[], "fast":false, "note":""},
                {"name":"other::large", "price":null, "priced":"unknown", "context":null, "defaultEffort":null, "efforts":[], "fast":false, "note":""}
            ]}),
        );
        assert!(!all_words(&mut app).contains("other::large"));
        press_and_update(&mut app, "small");
        press_and_update(&mut app, "Continue");
        let edit = sent(&mut app).remove(0);
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":edit["id"], "kind":"error", "text":"That model cannot be chosen."}),
        );
        assert!(
            all_words(&mut app)
                .contains("That model cannot be chosen.")
        );
        assert!(sent(&mut app).is_empty());
    }
    #[test]
    fn a_previous_connection_check_does_not_finish_a_model_trial() {
        let mut app = welcome_app();
        connect(&mut app, true);
        app.world_mut().resource_mut::<Settings>().0.link.state =
            protocol::LinkState::Connected;
        app.world_mut().resource_mut::<Welcome>().step = Step::Models;
        app.world_mut().resource_mut::<Welcome>().models =
            vec!["local::small".into()];
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "small");
        press_and_update(&mut app, "Continue");
        let edit = sent(&mut app).remove(0);
        app.world_mut().resource_mut::<Settings>().0.unset = false;
        app.update();
        assert!(
            all_words(&mut app).contains("Pick a model"),
            "a saved choice is not a model answer"
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":edit["id"], "kind":"notice", "text":"Saved."}),
        );
        assert_eq!(sent(&mut app)[0]["what"], "model");
        assert!(all_words(&mut app).contains("Pick a model"));
    }

    #[test]
    fn a_model_failure_received_after_its_check_reply_stays_on_the_page()
     {
        let mut app = welcome_app();
        connect(&mut app, true);
        app.world_mut().resource_mut::<Welcome>().step = Step::Models;
        app.world_mut().resource_mut::<Welcome>().models =
            vec!["local::small".into()];
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "small");
        press_and_update(&mut app, "Continue");
        let edit = sent(&mut app).remove(0);
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":edit["id"], "kind":"notice", "text":"Saved."}),
        );
        let check = sent(&mut app).remove(0);
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":check["id"], "kind":"notice", "text":"That model did not answer."}),
        );
        app.world_mut().resource_mut::<Settings>().0.link =
            protocol::Link {
                state: protocol::LinkState::Failed,
                models: None,
                words: Some("That model did not answer.".into()),
            };
        app.update();
        assert!(
            all_words(&mut app)
                .contains("That model did not answer.")
        );
    }

    #[test]
    fn a_cancel_before_the_first_login_step_cancels_it_when_its_id_arrives()
     {
        let mut app = welcome_app();
        connect(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "Continue with ChatGPT");
        let start = sent(&mut app).remove(0);
        press_and_update(&mut app, "Cancel sign-in");
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"late-login", "step":{"url":"https://must-not-open.example"}}),
        );
        assert_eq!(sent(&mut app)[0]["cancel"], "late-login");
        assert!(all_words(&mut app).contains("Welcome to Hamlet"));
    }

    #[test]
    fn reconnecting_returns_a_cancelled_login_to_the_first_page() {
        let mut app = welcome_app();
        connect(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press_and_update(&mut app, "Continue with ChatGPT");
        sent(&mut app);
        app.world_mut().trigger(Reconnected);
        app.update();
        assert!(all_words(&mut app).contains("Welcome to Hamlet"));
    }

    #[test]
    fn the_gallery_can_mount_a_key_page_without_the_setup_plugin() {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            ClientPlugin { server: None },
            FynixPlugin::<HamletTheme>::default(),
        ));
        theme::show(app.world_mut(), HamletTheme::designed());
        app.insert_resource(Welcome {
            open: true,
            step: Step::Key,
            ..Default::default()
        });
        mount::<HamletTheme>(app.world_mut(), view());
        assert!(all_words(&mut app).contains("Use a key"));
    }
}
