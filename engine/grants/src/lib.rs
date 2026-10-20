//! What the person lets plugins do. An installed plugin is switched
//! on only once the person let it in, and each capability it asks for
//! is answered Allow, Ask next time or Don't allow. The answers are
//! kept in the grants file, and changed on the Permissions page.

mod file;
mod page;
mod petition;
mod waiting;

use bevy::app::{App, Startup};
use bevy::ecs::component::Component;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Query, Res, ResMut};
use hamlet_engine_api::Capability;
use hamlet_engine_core::{Home, LogLine};
use hamlet_engine_plugins::{PluginInfo, Uninstall};

pub use self::file::{Answer, Answered, FILE, Grants, Record};
pub use self::petition::{
    Decided, FirstUse, NEEDS_YOU, Petition, Question, answer_install,
    answer_use,
};
pub use self::waiting::Waiting;

pub fn plugin(app: &mut App) {
    let home = app.world().resource::<Home>().clone();
    app.insert_resource(Grants::open(&home))
        .add_observer(forget)
        .add_systems(Startup, tell_held)
        .add_plugins((petition::plugin, page::plugin));
}

/// On an installed plugin the person let in.
#[derive(Component, Clone, Copy, Debug)]
pub struct Admitted;

/// Powers no plugin is ever granted, by the first word of a
/// capability's name, with what they are.
const NEVER: [(&str, &str); 6] = [
    ("keys", "reading keys"),
    ("secrets", "reading secrets"),
    ("answer", "answering for the person"),
    ("home", "the raw home"),
    ("process", "running processes"),
    ("sandbox", "turning the sandbox off"),
];

/// What the capability `name` is, when it is a power no plugin is
/// ever granted.
pub fn never_granted(name: &str) -> Option<&'static str> {
    let first = name.split('.').next().unwrap_or(name);
    NEVER
        .iter()
        .find(|(word, _)| first.eq_ignore_ascii_case(word))
        .map(|(_, power)| *power)
}

/// Whether a plugin may use a capability now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Allowed,
    Refused,
    /// The person is to be asked first: send a [`FirstUse`].
    Asking,
}

impl Grants {
    /// Whether the plugin `info` may use `capability` on `target`
    /// now. One it did not ask for is refused.
    pub fn verdict(
        &self,
        info: &PluginInfo,
        capability: Capability,
        target: &str,
    ) -> Verdict {
        let asked = info.asks.iter().any(|ask| {
            ask.capability == capability && ask.target == target
        });
        let record =
            self.record(&info.id).filter(|record| record.allowed);
        match (
            asked,
            record.and_then(|r| r.answer(capability, target)),
        ) {
            (true, Some(Answer::Allow)) => Verdict::Allowed,
            (true, Some(Answer::Ask)) => Verdict::Asking,
            _ => Verdict::Refused,
        }
    }
}

/// Forgets what the person answered for a plugin being uninstalled.
fn forget(
    event: On<Uninstall>,
    plugins: Query<&PluginInfo>,
    mut grants: ResMut<Grants>,
    mut log: MessageWriter<LogLine>,
) {
    let Ok(info) = plugins.get(event.entity) else {
        return;
    };
    if let Err(why) = grants.forget(&info.id) {
        log.write(LogLine::error(
            "grants",
            format!("{FILE} cannot be written: {why}"),
        ));
    }
}

fn tell_held(grants: Res<Grants>, mut log: MessageWriter<LogLine>) {
    if let Some(why) = grants.held() {
        log.write(LogLine::error(
            "grants",
            format!(
                "{FILE} cannot be read ({why}). It is left as it \
                 is, and answers are kept only until the engine \
                 closes."
            ),
        ));
    }
}
