//! First-run setup through the server's settings, models and logins.

mod pages;
mod providers;

use std::collections::BTreeMap;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::InputFocus;
use bevy::text::EditableText;
use hamlet_client::Connection;
use hamlet_client::connection::State;
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Settings;
use hamlet_protocol::client as protocol;

pub use self::pages::view;
pub use self::providers::Browser;

/// The empty local-model search, referring to the server's machine.
pub const NOTHING_LOCAL: &str = "No model server answered on the \
                                 server's machine. Install Ollama \
                                 from ollama.com there, download a \
                                 model, then choose Look Again.";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Welcome>()
        .init_resource::<Committing>()
        .init_resource::<Browser>()
        .init_resource::<providers::Login>()
        .add_observer(answered)
        .add_observer(|_: On<Reconnected>, mut commands: Commands| {
            commands.queue(providers::reconnected);
        })
        .add_systems(Update, (follow, run_committed, read_narrow));
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
    manual: bool,
    trying: bool,
    requests: BTreeMap<protocol::RequestId, Asked>,
}

#[derive(Clone)]
enum Asked {
    Models,
    Check(bool),
    Pick,
}

/// A pending field commit before a button's action.
#[derive(Resource, Default)]
pub struct Committing(Option<u8>);

impl Committing {
    pub fn waits(&self) -> bool {
        self.0.is_some()
    }
}

fn run_committed(world: &mut World) {
    let Some(frames) = world.resource::<Committing>().0 else {
        return;
    };
    if frames > 0 {
        world.resource_mut::<Committing>().0 = Some(frames - 1);
    } else {
        world.resource_mut::<Committing>().0 = None;
        providers::reply(world);
    }
}

fn commit(world: &mut World) {
    if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.clear();
    }
    world.resource_mut::<Committing>().0 = Some(3);
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
        if providers::receive(world, &response) {
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
pub fn follow(
    settings: Res<Settings>,
    connection: Res<Connection>,
    mut welcome: ResMut<Welcome>,
) {
    use bevy::ecs::change_detection::DetectChanges;
    let words = (connection.state != State::Connected)
        .then(|| connection.words.clone());
    if welcome.connection != words {
        welcome.connection = words;
    }
    if settings.is_changed() {
        if settings.0.unset {
            welcome.open = true;
        } else if !welcome.manual
            || !welcome.trying
                && settings.0.link.state
                    == protocol::LinkState::Connected
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

#[derive(Component)]
pub(crate) struct Narrowing;

pub(crate) fn read_narrow(
    field: Query<&Children, With<Narrowing>>,
    texts: Query<&EditableText>,
    mut welcome: ResMut<Welcome>,
) {
    let Some(text) = field
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != welcome.narrow.as_str() {
        welcome.narrow = text.value().to_string();
    }
}
#[cfg(test)]
pub(crate) mod server_tests {
    use bevy::color::Alpha;
    use bevy::ecs::entity::Entity;
    use bevy::ecs::hierarchy::ChildOf;
    use bevy::ui::widget::Text;
    use bevy::ui_widgets::{Activate, Button};
    use bevy_fynix::{FynixPlugin, mount};
    use hamlet_assets::Tuning;
    use hamlet_client::{ClientPlugin, receive};
    use hamlet_protocol::client as protocol;

    use super::*;
    use crate::theme::{self, HamletTheme};

    pub(crate) fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            bevy::ui_widgets::MenuPlugin,
            ClientPlugin { server: None },
            FynixPlugin::<HamletTheme>::default(),
            plugin,
            crate::secret::plugin,
        ));
        theme::insert_theme(app.world_mut(), HamletTheme::designed());
        app.insert_resource(Browser(|_, url| {
            panic!("unexpected browser URL: {url}")
        }));
        app
    }

    pub(crate) fn show(app: &mut App, unset: bool) {
        let mut state = hamlet_testing::realm::state();
        state.settings.unset = unset;
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.world_mut().resource_mut::<Connection>().state =
            State::Connected;
        for _ in 0..3 {
            app.update();
        }
    }

    pub(crate) fn words(app: &mut App) -> String {
        app.world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn press(app: &mut App, text: &str) {
        let world = app.world_mut();
        let entity = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .filter(|(_, shown)| shown.0 == text)
            .find_map(|(mut entity, _)| {
                loop {
                    if world.get::<Button>(entity).is_some()
                        || world
                            .get::<bevy::ui_widgets::MenuItem>(entity)
                            .is_some()
                    {
                        return Some(entity);
                    }
                    entity = world.get::<ChildOf>(entity)?.parent();
                }
            })
            .unwrap_or_else(|| panic!("no button {text}"));
        world.trigger(Activate { entity });
        for _ in 0..5 {
            app.update();
        }
    }

    #[test]
    fn received_settings_and_connection_choose_the_welcome_page() {
        let mut app = app();
        show(&mut app, true);
        mount::<HamletTheme>(app.world_mut(), view());
        assert!(words(&mut app).contains("WELCOME TO HAMLET"));
        show(&mut app, false);
        assert!(!words(&mut app).contains("WELCOME TO HAMLET"));
        app.world_mut().insert_resource(Connection {
            state: State::Refused,
            words: "The server refused this window: bad token."
                .into(),
        });
        for _ in 0..3 {
            app.update();
        }
        let shown = words(&mut app);
        assert!(shown.contains("bad token"));
        for command in
            ["hamlet server", "hamlet window", "hamlet dev"]
        {
            assert!(shown.contains(command), "{shown}");
        }
    }
    pub(crate) fn sent(app: &mut App) -> Vec<serde_json::Value> {
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .map(|message| serde_json::to_value(message).unwrap())
            .collect()
    }

    pub(crate) fn incoming(app: &mut App, value: serde_json::Value) {
        receive(
            app.world_mut(),
            serde_json::from_value(value).unwrap(),
        );
        for _ in 0..3 {
            app.update();
        }
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

    #[test]
    fn a_providers_key_uses_the_servers_secret_prompt_and_answers() {
        let mut app = app();
        show(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "I HAVE A KEY");
        press(&mut app, "OpenRouter");
        let requests = sent(&mut app);
        let start = &requests[0];
        assert_eq!(start["type"], "login");
        assert_eq!(
            start["start"],
            serde_json::json!({"provider":"openrouter", "method":"key"})
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"key-login", "step":{"prompt":"Paste your key", "secret":true}}),
        );
        assert!(words(&mut app).contains("Paste your key"));
        let (input, ink) = app
            .world_mut()
            .query::<(Entity, &EditableText, &bevy::text::TextColor)>(
            )
            .iter(app.world())
            .map(|(entity, _, color)| (entity, color.0))
            .next()
            .expect("secret field");
        assert_eq!(ink.alpha(), 0.0);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, bevy::input_focus::FocusCause::Pressed);
        app.update();
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text("fake-key");
        press(&mut app, "SEND");
        let requests = sent(&mut app);
        assert_eq!(
            requests[0]["reply"],
            serde_json::json!({"login":"key-login", "value":"fake-key"})
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"key-login", "step":{"failed":"That key was refused."}}),
        );
        assert!(words(&mut app).contains("That key was refused."));
        assert!(!words(&mut app).contains("fake-key"));
    }

    #[derive(Resource, Default)]
    struct Opened(Vec<String>);

    #[test]
    fn subscription_steps_open_the_browser_show_codes_and_cancel_on_the_server()
     {
        let mut app = app();
        app.init_resource::<Opened>();
        app.world_mut().insert_resource(Browser(|world, url| {
            world.resource_mut::<Opened>().0.push(url.into());
        }));
        show(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "CONTINUE WITH CHATGPT");
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
        let shown = words(&mut app);
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
        press(&mut app, "CANCEL SIGN-IN");
        assert_eq!(sent(&mut app)[0]["cancel"], "plan-login");
        press(&mut app, "SIGN IN WITH OPENROUTER");
        assert_eq!(
            sent(&mut app)[0]["start"],
            serde_json::json!({"provider":"openrouter", "method":"subscription"})
        );
    }

    #[test]
    fn login_urls_open_only_https_or_loopback_http_and_always_show_the_steps_words()
     {
        let mut app = app();
        app.init_resource::<Opened>().insert_resource(Browser(
            |world, url| {
                world.resource_mut::<Opened>().0.push(url.into());
            },
        ));
        show(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "CONTINUE WITH CHATGPT");
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
            let shown = words(&mut app);
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
        let mut app = app();
        show(&mut app, true);
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "I RUN MY OWN MODEL");
        let requests = sent(&mut app);
        assert_eq!(requests[0]["what"], "connection");
        assert_eq!(requests[1]["what"], "models");
        assert_eq!(requests[1]["refresh"], true);
        incoming(
            &mut app,
            serde_json::json!({"type":"read", "to":requests[1]["id"], "models":[]}),
        );
        assert!(words(&mut app).contains(
            "No model server answered on the server's machine."
        ));
        press(&mut app, "LOOK AGAIN");
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
                {"name":"local::small", "price":null, "priced":"unknown", "context":null, "allowed":false, "note":""},
                {"name":"unconfigured::far", "price":null, "priced":"unknown", "context":null, "allowed":false, "note":""}
            ]}),
        );
        assert!(!words(&mut app).contains("unconfigured::far"));
        press(&mut app, "local::small");
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
        for _ in 0..3 {
            app.update();
        }
        assert!(words(&mut app).contains("PICK A MODEL"));
        app.world_mut().resource_mut::<Settings>().0.link.state =
            protocol::LinkState::Connected;
        for _ in 0..3 {
            app.update();
        }
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":check["id"], "kind":"notice", "text":"Hello"}),
        );
        assert!(!words(&mut app).contains("PICK A MODEL"));
    }

    #[test]
    fn completed_login_moves_to_its_own_models_and_a_refused_edit_stays_open()
     {
        let mut app = app();
        show(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "I HAVE A KEY");
        press(&mut app, "OpenRouter");
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
                {"name":"openrouter::small", "price":null, "priced":"unknown", "context":null, "allowed":false, "note":""},
                {"name":"other::large", "price":null, "priced":"unknown", "context":null, "allowed":false, "note":""}
            ]}),
        );
        assert!(!words(&mut app).contains("other::large"));
        press(&mut app, "openrouter::small");
        let edit = sent(&mut app).remove(0);
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":edit["id"], "kind":"error", "text":"That model cannot be chosen."}),
        );
        assert!(
            words(&mut app).contains("That model cannot be chosen.")
        );
        assert!(sent(&mut app).is_empty());
    }
    #[test]
    fn a_previous_connection_check_does_not_finish_a_model_trial() {
        let mut app = app();
        show(&mut app, true);
        app.world_mut().resource_mut::<Settings>().0.link.state =
            protocol::LinkState::Connected;
        app.world_mut().resource_mut::<Welcome>().step = Step::Models;
        app.world_mut().resource_mut::<Welcome>().models =
            vec!["local::small".into()];
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "local::small");
        let edit = sent(&mut app).remove(0);
        app.world_mut().resource_mut::<Settings>().0.unset = false;
        for _ in 0..3 {
            app.update();
        }
        assert!(
            words(&mut app).contains("PICK A MODEL"),
            "a saved choice is not a model answer"
        );
        incoming(
            &mut app,
            serde_json::json!({"type":"reply", "to":edit["id"], "kind":"notice", "text":"Saved."}),
        );
        assert_eq!(sent(&mut app)[0]["what"], "model");
        assert!(words(&mut app).contains("PICK A MODEL"));
    }

    pub(crate) fn type_field(app: &mut App, was: &str, text: &str) {
        let input = app
            .world_mut()
            .query::<(Entity, &EditableText)>()
            .iter(app.world())
            .find(|(_, value)| value.value() == was)
            .map(|(entity, _)| entity)
            .expect("editable field");
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(input, bevy::input_focus::FocusCause::Pressed);
        app.update();
        app.world_mut()
            .get_mut::<EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text(text);
        app.world_mut().resource_mut::<InputFocus>().clear();
        for _ in 0..5 {
            app.update();
        }
    }
    #[test]
    fn a_model_failure_received_after_its_check_reply_stays_on_the_page()
     {
        let mut app = app();
        show(&mut app, true);
        app.world_mut().resource_mut::<Welcome>().step = Step::Models;
        app.world_mut().resource_mut::<Welcome>().models =
            vec!["local::small".into()];
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "local::small");
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
        for _ in 0..3 {
            app.update();
        }
        assert!(
            words(&mut app).contains("That model did not answer.")
        );
    }

    #[test]
    fn a_cancel_before_the_first_login_step_cancels_it_when_its_id_arrives()
     {
        let mut app = app();
        show(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "CONTINUE WITH CHATGPT");
        let start = sent(&mut app).remove(0);
        press(&mut app, "CANCEL SIGN-IN");
        incoming(
            &mut app,
            serde_json::json!({"type":"login", "to":start["id"], "login":"late-login", "step":{"url":"https://must-not-open.example"}}),
        );
        assert_eq!(sent(&mut app)[0]["cancel"], "late-login");
        assert!(words(&mut app).contains("WELCOME TO HAMLET"));
    }

    #[test]
    fn reconnecting_returns_a_cancelled_login_to_the_first_page() {
        let mut app = app();
        show(&mut app, true);
        providers(&mut app);
        mount::<HamletTheme>(app.world_mut(), view());
        press(&mut app, "CONTINUE WITH CHATGPT");
        sent(&mut app);
        app.world_mut().trigger(Reconnected);
        for _ in 0..3 {
            app.update();
        }
        assert!(words(&mut app).contains("WELCOME TO HAMLET"));
    }

    #[test]
    fn the_gallery_can_mount_a_key_page_without_the_setup_plugin() {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            ClientPlugin { server: None },
            FynixPlugin::<HamletTheme>::default(),
        ));
        theme::insert_theme(app.world_mut(), HamletTheme::designed());
        app.insert_resource(Welcome {
            open: true,
            step: Step::Key,
            ..Default::default()
        });
        mount::<HamletTheme>(app.world_mut(), view());
        assert!(words(&mut app).contains("USE A KEY"));
    }
}
