//! The window's focus: the conversation open, who it is with, and
//! the thing selected. Every panel follows it, and each change is
//! told to the watchers.

use bevy::app::{App, PostUpdate};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use hamlet_core::agents::{self, Mayor, Roster};
use hamlet_core::session;
use hamlet_engine_api::agents::{
    AgentsEvent, Focus, Recipient, Selected, Session,
};
use hamlet_rules::hamlet::HOME;

use crate::answer::AgentsWorldExt;
use crate::watch::{Gather, Watchers};
use crate::{Chief, views};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Told>().add_systems(
        PostUpdate,
        (reseat, tell_focus).chain().in_set(Gather),
    );
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
