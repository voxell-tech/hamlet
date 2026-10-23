//! The window's focus: the conversation open, who it is with, and
//! the thing selected. Every panel follows it, and each change is
//! told to the watchers.

use bevy::app::{App, PostUpdate, Startup};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::name::Name;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Res, ResMut};
use bevy::ecs::world::World;
use hamlet_core::agents::{self, Citizen, Mayor, Roster};
use hamlet_core::core::HomeDir;
use hamlet_core::session;
use hamlet_engine_api::agents::{
    AgentsEvent, Focus, Recipient, Selected, Session,
};
use hamlet_rules::hamlet::HOME;
use serde::{Deserialize, Serialize};

use crate::answer::AgentsWorldExt;
use crate::roster::RosterWorldExt;
use crate::watch::{Gather, Watchers};
use crate::{Chief, views};

/// The file under the home the selection is kept in between
/// launches.
const KEPT: &str = "selection.toml";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Told>()
        .add_systems(Startup, reselect)
        .add_systems(
            PostUpdate,
            ((reseat, tell_focus).chain().in_set(Gather), keep),
        );
}

/// The selection as its file keeps it.
#[derive(Serialize, Deserialize)]
struct Kept {
    selected: Selected,
}

/// The focus's methods on [`World`].
pub(crate) trait FocusWorldExt {
    /// The focus as it stands.
    fn focus(&self) -> Option<Focus>;

    /// Opens the chief's session with `to` as the open conversation,
    /// and selects its agent.
    fn open(&mut self, to: Recipient) -> Option<Session>;

    /// Selects `thing`, talking to it when it is a living agent.
    fn select(&mut self, thing: Selected);
}

impl FocusWorldExt for World {
    fn focus(&self) -> Option<Focus> {
        let chief = self.resource::<Chief>();
        Some(Focus {
            chief: chief.name.clone(),
            with: chief.with.clone(),
            open: views::session(self, chief.session)?,
            selected: chief.selected.clone(),
            offset: chrono::Local::now().offset().local_minus_utc(),
        })
    }

    fn open(&mut self, to: Recipient) -> Option<Session> {
        let key = self.chief_key(&to);
        let session = session::ensure(self, &key);
        let agent = match &to {
            Recipient::Agent(name) => agents::named(self, name),
            Recipient::Mayor(hamlet) => {
                agents::mayor_of(self, hamlet)
            }
        };
        // Its letters to and from other agents are part of its
        // session.
        if let Some(agent) = agent {
            session::gather(self, agent);
        }
        // A mayor is talked to by its office.
        let office = agent
            .filter(|agent| self.get::<Mayor>(*agent).is_some())
            .map(|mayor| {
                agents::hamlet_of(self, mayor)
                    .unwrap_or(HOME)
                    .to_string()
            });
        let named = agent.map(|agent| agents::name_of(self, agent));
        let mut chief = self.resource_mut::<Chief>();
        chief.session = session;
        chief.with = office.map_or(to, Recipient::Mayor);
        if let Some(name) = named {
            chief.selected = Selected::Agent(name);
        }
        views::session(self, session)
    }

    fn select(&mut self, thing: Selected) {
        if let Selected::Agent(name) = &thing
            && let Some(agent) = agents::named(self, name)
        {
            let open = self.resource::<Chief>().session;
            let with =
                self.get::<session::TalksTo>(open).map(|with| with.0);
            if with != Some(agent) {
                let name = agents::name_of(self, agent);
                self.open(Recipient::Agent(name));
            }
        }
        self.resource_mut::<Chief>().selected = thing;
    }
}

/// Falls back to the first mayor when the agent the open
/// conversation is with is gone.
fn reseat(chief: Res<Chief>, roster: Roster, mut commands: Commands) {
    let Recipient::Agent(name) = &chief.with else {
        return;
    };
    if roster.named(name).is_none() {
        commands.queue(|world: &mut World| {
            world.open(Recipient::Mayor(HOME.into()));
        });
    }
}

/// Selects again what was selected when the engine last stopped, if
/// it is still there.
fn reselect(world: &mut World) {
    let file = world.resource::<HomeDir>().0.root.join(KEPT);
    let Some(kept) = std::fs::read_to_string(file)
        .ok()
        .and_then(|text| toml::from_str::<Kept>(&text).ok())
    else {
        return;
    };
    let there = match &kept.selected {
        // Living or gone.
        Selected::Agent(name) => world
            .query_filtered::<&Name, With<Citizen>>()
            .iter(world)
            .any(|called| called.as_str() == name),
        Selected::Hamlet(name) => world.hamlet_root(name).is_ok(),
        Selected::Hearth => true,
        Selected::Library | Selected::Nothing => false,
    };
    if there {
        world.select(kept.selected);
    }
}

/// Writes the selection when it changes. The library is not kept.
fn keep(
    chief: Res<Chief>,
    home: Res<HomeDir>,
    mut written: Local<Option<Selected>>,
) {
    if !chief.is_changed() {
        return;
    }
    let selected = match &chief.selected {
        Selected::Library => Selected::Nothing,
        selected => selected.clone(),
    };
    if written.as_ref() == Some(&selected) {
        return;
    }
    let kept = Kept {
        selected: selected.clone(),
    };
    if let Ok(text) = toml::to_string(&kept) {
        let _ = std::fs::write(home.0.root.join(KEPT), text);
    }
    *written = Some(selected);
}

/// The focus the watchers were last told of.
#[derive(Resource, Default)]
struct Told(Option<Focus>);

/// Tells the watchers of a change of the focus.
fn tell_focus(
    watchers: Res<Watchers>,
    mut told: ResMut<Told>,
    mut commands: Commands,
) {
    if !watchers.watched() {
        told.0 = None;
        return;
    }
    commands.queue(|world: &mut World| {
        let now = world.focus();
        let mut told = world.resource_mut::<Told>();
        if told.0 == now {
            return;
        }
        told.0.clone_from(&now);
        if let Some(focus) = now {
            world
                .resource_mut::<Watchers>()
                .tell(AgentsEvent::Focus(focus));
        }
    });
}
