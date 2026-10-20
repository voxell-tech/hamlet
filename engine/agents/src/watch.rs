//! The plugins that watch the agents, and the changes sent to them,
//! each as a reply to the ticket of their `Watch`.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use bevy::app::{App, PostUpdate};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::ecs::system::{Local, Query, Res, ResMut, SystemParam};
use hamlet_core::agents::{Charter as Limits, Settlement};
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{HomeDir, Link, Listing, Settings, Trial};
use hamlet_core::oversight::grants::Grants as Held;
use hamlet_core::session::{
    SessionKey, SessionOutput, Status as Work,
};
use hamlet_engine_api::agents::{
    AgentsEvent, AgentsReply, Output, Status,
};
use hamlet_engine_api::{Message, Reply, Ticket};
use hamlet_engine_plugins::{Deliver, PluginInfo};
use hamlet_home::charter;
use hamlet_rules::hamlet::HOME;

use crate::answer::take;
use crate::setup::SetupState;
use crate::views::{self, StatusFacts};

/// The time between two looks at the charter's file.
const LOOK: Duration = Duration::from_secs(2);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Watchers>()
        .add_observer(heard)
        .configure_sets(PostUpdate, Gather.after(take))
        .add_systems(PostUpdate, send_events.after(Gather));
}

/// The systems that keep changes for the watchers, before they are
/// sent.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Gather;

/// The plugins that watch, each by the ticket of its `Watch`, and
/// the changes not yet sent to them.
#[derive(Resource, Default)]
pub(crate) struct Watchers {
    watching: Vec<(Entity, Ticket)>,
    pending: Vec<AgentsEvent>,
}

impl Watchers {
    /// Sends the changes to `plugin` from now on, with `ticket`.
    pub(crate) fn watch(&mut self, plugin: Entity, ticket: Ticket) {
        self.watching.retain(|(watcher, _)| *watcher != plugin);
        self.watching.push((plugin, ticket));
    }

    /// Some plugin watches.
    pub(crate) fn watched(&self) -> bool {
        !self.watching.is_empty()
    }

    /// Keeps `event` to send, joined to the one before when both are
    /// words of one kind of one session, so a reply streamed word by
    /// word goes in one event a frame.
    pub(crate) fn tell(&mut self, event: AgentsEvent) {
        if let AgentsEvent::Output { session, output } = &event
            && let Some(AgentsEvent::Output {
                session: before,
                output: earlier,
            }) = self.pending.last_mut()
            && before == session
            && let (Output::Text(said), Output::Text(more))
            | (Output::Reasoning(said), Output::Reasoning(more)) =
                (earlier, output)
        {
            said.push_str(more);
            return;
        }
        self.pending.push(event);
    }
}

/// What the watchers were last told, and when the charter's file is
/// next looked at.
#[derive(Default)]
struct Told {
    working: HashMap<Entity, bool>,
    status: Option<Status>,
    charter_due: Option<Instant>,
    charter_stamp: Option<SystemTime>,
}

/// The state of the agent core that the watchers are told of.
#[derive(SystemParam)]
struct Watched<'w, 's> {
    home: Res<'w, HomeDir>,
    settings: Res<'w, Settings>,
    link: Res<'w, Link>,
    listing: Res<'w, Listing>,
    models: Res<'w, ModelCatalogue>,
    trial: Res<'w, Trial>,
    setup: Res<'w, SetupState>,
    held: Res<'w, Held>,
    facts: StatusFacts<'w, 's>,
    hamlets:
        Query<'w, 's, (&'static Settlement, Ref<'static, Limits>)>,
    sessions: Query<
        'w,
        's,
        (Entity, &'static SessionKey, Ref<'static, Work>),
    >,
    added: Query<'w, 's, (), Added<SessionKey>>,
}

impl Watched<'_, '_> {
    /// What changed since the last frame, against what was `told`.
    /// `gone` when a session went.
    fn changes(
        &self,
        told: &mut Told,
        gone: bool,
    ) -> Vec<AgentsEvent> {
        let root = &self.home.0.root;
        let mut events = Vec::new();
        if self.settings.is_changed()
            || self.link.is_changed()
            || self.listing.is_changed()
        {
            events.push(AgentsEvent::Settings(views::settings_of(
                &self.settings,
                &self.link,
                &self.listing,
                root,
            )));
        }
        if self.link.is_changed() {
            events.push(AgentsEvent::Link(views::link(&self.link)));
        }
        if self.listing.is_changed() {
            events.push(AgentsEvent::Listing(views::listing(
                &self.listing,
            )));
        }
        if self.models.is_changed() {
            events.push(AgentsEvent::Catalogue);
        }
        if self.trial.is_changed() {
            events
                .push(AgentsEvent::Trial(views::trial(&self.trial)));
        }
        if self.setup.is_changed() {
            events.push(AgentsEvent::Setup(self.setup.view()));
        }
        if self.held.is_changed() {
            events
                .push(AgentsEvent::Grants(views::grants(&self.held)));
        }
        let limits_moved =
            self.hamlets.iter().any(|(hamlet, limits)| {
                hamlet.name == HOME && limits.is_changed()
            });
        if limits_moved {
            events.push(AgentsEvent::Limits(views::limits(root)));
        }
        let now = Instant::now();
        if told.charter_due.is_none_or(|due| now >= due) {
            told.charter_due = Some(now + LOOK);
            let stamp = std::fs::metadata(charter::file(root))
                .and_then(|file| file.modified())
                .ok();
            if told.charter_stamp != stamp {
                told.charter_stamp = stamp;
                events
                    .push(AgentsEvent::Charter(views::charter(root)));
            }
        }
        if gone || !self.added.is_empty() {
            events.push(AgentsEvent::Sessions);
        }
        told.working
            .retain(|session, _| self.sessions.contains(*session));
        for (session, key, work) in &self.sessions {
            let working = work.working();
            if work.is_changed()
                && told.working.insert(session, working)
                    != Some(working)
            {
                events.push(AgentsEvent::Working {
                    session: views::session_name(key),
                    working,
                });
            }
        }
        let status = self.facts.status();
        if told.status.as_ref() != Some(&status) {
            events.push(AgentsEvent::Status(status.clone()));
            told.status = Some(status);
        }
        events
    }
}

fn heard(out: On<SessionOutput>, mut watchers: ResMut<Watchers>) {
    if watchers.watching.is_empty() {
        return;
    }
    let Some(output) = views::output(&out.reply) else {
        return;
    };
    watchers.tell(AgentsEvent::Output {
        session: views::session_name(&out.key),
        output,
    });
}

/// Sends each watcher what changed since the last frame.
fn send_events(
    watched: Watched,
    mut removed: RemovedComponents<SessionKey>,
    plugins: Query<(), With<PluginInfo>>,
    mut watchers: ResMut<Watchers>,
    mut told: Local<Told>,
    mut deliver: MessageWriter<Deliver>,
) {
    let gone = removed.read().count() > 0;
    watchers
        .watching
        .retain(|(plugin, _)| plugins.contains(*plugin));
    if watchers.watching.is_empty() {
        watchers.pending.clear();
        return;
    }
    let mut events = watched.changes(&mut told, gone);
    events.append(&mut watchers.pending);
    for (plugin, ticket) in &watchers.watching {
        for event in &events {
            deliver.write(Deliver {
                plugin: *plugin,
                message: Message::Reply {
                    ticket: *ticket,
                    reply: Ok(Reply::Agents(AgentsReply::Event(
                        event.clone(),
                    ))),
                },
                depth: 1,
            });
        }
    }
}
