//! Events between plugins. A plugin subscribes by name to an event
//! its manifest hears, and sends only the events its manifest sends,
//! `[budgets] events_sent_per_second` at most. The events of a frame
//! are passed on together, to each subscriber that names the
//! payload's type as the sender does, and handed over in one batch on
//! the next frame.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use bevy::app::{App, PostUpdate};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::observer::On;
use bevy::ecs::system::{Local, Query, Res};
use bevy::time::{Real, Time};
use hamlet_engine_api::{EventId, Failure, Message, Reply, Request};
use hamlet_engine_core::Budgets;
use hamlet_engine_plugins::{Asked, Deliver, Disable, PluginInfo};

pub(crate) fn plugin(app: &mut App) {
    app.register_required_components::<PluginInfo, Subscriptions>()
        .add_observer(forget)
        .add_systems(PostUpdate, pass);
}

/// On a plugin: the events it subscribed to.
#[derive(Component, Default, Debug)]
pub struct Subscriptions(pub BTreeSet<EventId>);

fn forget(
    event: On<Disable>,
    mut plugins: Query<&mut Subscriptions>,
) {
    if let Ok(mut subscriptions) = plugins.get_mut(event.entity) {
        subscriptions.0.clear();
    }
}

/// The bus's methods on [`PluginInfo`].
trait BusPluginInfoExt {
    /// The event `event` the plugin subscribes to, if its manifest
    /// hears it.
    fn subscribe(&self, event: &str) -> Result<EventId, Failure>;

    /// Whether the plugin hears the event `id` of `from` with the
    /// payload's type `from` sends it with.
    fn hears(&self, from: &PluginInfo, id: &EventId) -> bool;
}

impl BusPluginInfoExt for PluginInfo {
    fn subscribe(&self, event: &str) -> Result<EventId, Failure> {
        let id = EventId::try_from(event)
            .map_err(|why| Failure::Refused(why.to_string()))?;
        if self.events.hears.iter().any(|heard| heard.event == id) {
            Ok(id)
        } else {
            Err(Failure::Refused(format!(
                "{} does not hear {id}",
                self.id
            )))
        }
    }

    fn hears(&self, from: &PluginInfo, id: &EventId) -> bool {
        let sent = from
            .events
            .sends
            .iter()
            .find(|sent| sent.name == id.name());
        let heard =
            self.events.hears.iter().find(|heard| heard.event == *id);
        matches!((sent, heard), (Some(sent), Some(heard)) if sent.payload == heard.payload)
    }
}

/// Takes subscriptions, and passes on the events sent this frame.
/// `sent` holds, for each sender, when its second began and how many
/// events it sent in it.
fn pass(
    mut asked: MessageReader<Asked>,
    mut plugins: Query<(Entity, &PluginInfo, &mut Subscriptions)>,
    budgets: Res<Budgets>,
    time: Res<Time<Real>>,
    mut sent: Local<HashMap<Entity, (Duration, u32)>>,
    mut deliver: MessageWriter<Deliver>,
) {
    let now = time.elapsed();
    sent.retain(|plugin, _| plugins.contains(*plugin));
    let mut frame = Vec::new();
    for request in asked.read() {
        let Ok((_, plugin, mut subscriptions)) =
            plugins.get_mut(request.plugin)
        else {
            continue;
        };
        match &request.request {
            Request::Subscribe(event) => {
                let reply = plugin.subscribe(event).map(|id| {
                    subscriptions.0.insert(id);
                    Reply::Subscribed
                });
                deliver.write(request.reply(reply));
            }
            Request::Emit { event, payload } => {
                let refused = |why: String| {
                    request.reply(Err(Failure::Refused(why)))
                };
                let id = match EventId::new(plugin.id.as_str(), event)
                {
                    Ok(id)
                        if plugin
                            .events
                            .sends
                            .iter()
                            .any(|sent| sent.name == *event) =>
                    {
                        id
                    }
                    _ => {
                        deliver.write(refused(format!(
                            "{} does not send {event}",
                            plugin.id
                        )));
                        continue;
                    }
                };
                if request.depth > budgets.chain_depth {
                    deliver.write(request.reply(Err(
                        Failure::OutOfBudget(format!(
                            "the chain crosses more than {} plugins",
                            budgets.chain_depth
                        )),
                    )));
                    continue;
                }
                let (since, count) = sent
                    .get(&request.plugin)
                    .copied()
                    .filter(|(since, _)| {
                        now.saturating_sub(*since)
                            < Duration::from_secs(1)
                    })
                    .unwrap_or((now, 0));
                if count >= budgets.events_sent_per_second {
                    deliver.write(request.reply(Err(
                        Failure::OutOfBudget(format!(
                            "{} events a second, the most",
                            budgets.events_sent_per_second
                        )),
                    )));
                    continue;
                }
                sent.insert(request.plugin, (since, count + 1));
                frame.push((
                    request.plugin,
                    id,
                    payload.clone(),
                    request.depth,
                ));
            }
            _ => {}
        }
    }
    for (sender, id, payload, depth) in frame {
        let Ok((_, from, _)) = plugins.get(sender) else {
            continue;
        };
        for (plugin, to, subscriptions) in &plugins {
            if subscriptions.0.contains(&id) && to.hears(from, &id) {
                deliver.write(Deliver {
                    plugin,
                    message: Message::Event {
                        from: from.id.clone(),
                        event: id.clone(),
                        payload: payload.clone(),
                    },
                    depth,
                });
            }
        }
    }
}
