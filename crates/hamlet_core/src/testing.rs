//! The support the tests of this crate share: a home that goes when
//! its test ends, replies for the replay provider, and an app to put
//! lines to.

use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_home::Home;
use hamlet_model::config::Config;
use hamlet_model::log;
use hamlet_rules::hamlet::Hamlet;
use hamlet_sandbox::Sandbox;
use hamlet_testing::Scratch;
use serde_json::{Value, json};
use tokio::runtime::Runtime;

use crate::core::{
    self, HomeDir, Model, Sandboxing, Settings, Tokio,
};
use crate::session::{self, SessionKey, Status, UserMessage};
use crate::{agents, catalogue};

/// The chief every test speaks as.
pub(crate) const ALICE: &str = "alice";

/// A home in a folder that goes when the test ends.
pub(crate) struct Place {
    home: Home,
    _dir: Scratch,
}

impl Deref for Place {
    type Target = Home;

    fn deref(&self) -> &Home {
        &self.home
    }
}

/// A fresh home. `tag` must differ between tests.
pub(crate) fn place(tag: &str) -> Place {
    let dir = Scratch::new(tag);
    Place {
        home: Home::open(dir.to_path_buf(), None).unwrap(),
        _dir: dir,
    }
}

/// The first hamlet of `home`, with a librarian and a scout.
pub(crate) fn hamlet_of_two(home: &Home) -> Hamlet {
    let hamlet = Hamlet::new(home.root.clone());
    hamlet
        .create("librarian", "Keeps the reading list.", "test")
        .unwrap();
    hamlet.create("scout", "Looks things up.", "test").unwrap();
    hamlet
}

/// Writes the chief's limits for the first hamlet of `home`.
pub(crate) fn limit(home: &Home, text: &str) {
    std::fs::write(hamlet_home::limits::path(&home.root), text)
        .unwrap();
}

/// A reply in words.
pub(crate) fn says(text: &str) -> Value {
    json!({"role": "assistant", "content": text})
}

/// A reply that calls the tool `name`.
pub(crate) fn call(name: &str, args: Value) -> Value {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let id = format!("c{}", CALLS.fetch_add(1, Ordering::Relaxed));
    json!({"role": "assistant", "content": "", "tool_calls": [{
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": args.to_string()},
    }]})
}

/// A reply that runs `command` in the shell.
pub(crate) fn bash(command: &str) -> Value {
    call("bash", json!({"command": command}))
}

/// The tool results in the log at `path`.
pub(crate) fn told(path: &Path) -> Vec<String> {
    log::read_events(path)
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "tool")
        .map(|e| e["output"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// An app with sessions in `home` whose model answers with
/// `replies`, and which never reflects unasked.
pub(crate) fn app(
    home: &Place,
    rt: &Runtime,
    sandbox: &'static dyn Sandbox,
    replies: Vec<Value>,
) -> App {
    let config = Config {
        reflect_every: 0,
        ..Config::default()
    };
    let model = Model::new(&config, &home.root, Some(replies));
    let mut app = App::new();
    app.insert_resource(Tokio(rt.handle().clone()))
        .insert_resource(HomeDir(home.home.clone()))
        .insert_resource(Sandboxing(sandbox))
        .insert_resource(model)
        .insert_resource(Settings {
            config,
            error: None,
        })
        .add_plugins((
            core::plugin,
            agents::plugin,
            catalogue::plugin,
            session::plugin,
        ));
    app
}

/// The session a test's chief keeps with the first mayor.
pub(crate) fn with_mayor() -> SessionKey {
    SessionKey {
        channel: "test".into(),
        chat: ALICE.into(),
        thread: "main".into(),
    }
}

/// The session a test's chief keeps with the agent `name`.
pub(crate) fn with_agent(name: &str) -> SessionKey {
    SessionKey::with_agent("test", ALICE, name)
}

/// Types `text` to the session `key`.
pub(crate) fn say(app: &mut App, key: &SessionKey, text: &str) {
    app.world_mut().trigger(UserMessage {
        key: key.clone(),
        user: ALICE.into(),
        text: text.into(),
    });
    app.world_mut().flush();
}

/// Ticks until `done`.
pub(crate) fn until(
    app: &mut App,
    what: &str,
    mut done: impl FnMut(&mut App) -> bool,
) {
    for _ in 0..800 {
        app.update();
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("never happened: {what}");
}

/// The session `key`, if it is loaded.
pub(crate) fn session(
    app: &mut App,
    key: &SessionKey,
) -> Option<Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &SessionKey)>()
        .iter(world)
        .find(|(_, k)| *k == key)
        .map(|(entity, _)| entity)
}

/// True when the session `key` has a turn queued or running.
pub(crate) fn working(app: &mut App, key: &SessionKey) -> bool {
    session(app, key).is_some_and(|session| {
        app.world().get::<Status>(session).unwrap().working()
    })
}

/// Ticks until the session `key` has nothing queued or running, and
/// returns it.
pub(crate) fn settle(app: &mut App, key: &SessionKey) -> Entity {
    until(app, "the turn to end", |app| !working(app, key));
    session(app, key).expect("the session is loaded")
}
