//! The bridge in agents' pages: the acts a page's own document
//! carries out for the agents' web tools, through the script in
//! `bridge.js`.

use std::collections::BTreeMap;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::time::{Real, Time};
use bevy_cef::prelude::{HostEmitEvent, JsEmitEventPlugin, Receive};
use hamlet_protocol::client::RequestId;
use serde::Deserialize;
use serde_json::{Value, json};

use super::agents::answer;

/// The script, run in each agent's page before the page's own.
pub(super) const SCRIPT: &str = include_str!("bridge.js");

/// How long a page is given to carry out an act.
const ACT_WITHIN: Duration = Duration::from_secs(25);

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(JsEmitEventPlugin::<Reply>::default())
        .init_resource::<Bridged>()
        .add_observer(replied)
        .add_systems(Update, give_up);
}

/// A page's reply to an act, as the script emits it.
#[derive(Deserialize, Debug)]
struct Reply {
    hamlet: String,
    failed: bool,
    text: String,
}

/// The acts asked of pages and not yet replied to, by nonce.
#[derive(Resource, Default)]
struct Bridged(BTreeMap<String, Act>);

struct Act {
    id: RequestId,
    page: Entity,
    until: Duration,
}

/// Asks `page` to carry out `act` with `arguments`, for the ask `id`.
pub(super) fn ask(
    world: &mut World,
    page: Entity,
    id: RequestId,
    act: &str,
    arguments: Value,
) {
    let nonce = RequestId::new().as_str().to_owned();
    let mut asked = json!({"nonce": nonce, "act": act});
    if let (Some(asked), Value::Object(arguments)) =
        (asked.as_object_mut(), arguments)
    {
        asked.extend(arguments);
    }
    world.trigger(HostEmitEvent::new(page, "hamlet", &asked));
    let until = world.resource::<Time<Real>>().elapsed() + ACT_WITHIN;
    world
        .resource_mut::<Bridged>()
        .0
        .insert(nonce, Act { id, page, until });
}

/// Answers the act a page replied to.
fn replied(reply: On<Receive<Reply>>, mut commands: Commands) {
    let (page, nonce, failed, text) = (
        reply.webview,
        reply.hamlet.clone(),
        reply.failed,
        reply.text.clone(),
    );
    commands.queue(move |world: &mut World| {
        let mine = world
            .resource::<Bridged>()
            .0
            .get(&nonce)
            .is_some_and(|act| act.page == page);
        if !mine {
            return;
        }
        if let Some(act) =
            world.resource_mut::<Bridged>().0.remove(&nonce)
        {
            answer(world, act.id, text, failed);
        }
    });
}

/// Fails the acts a page did not reply to in time, as when it left
/// for another page before it replied.
fn give_up(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed();
    let late = world
        .resource::<Bridged>()
        .0
        .iter()
        .filter(|(_, act)| act.until <= now)
        .map(|(nonce, _)| nonce.clone())
        .collect::<Vec<_>>();
    for nonce in late {
        if let Some(act) =
            world.resource_mut::<Bridged>().0.remove(&nonce)
        {
            answer(
                world,
                act.id,
                format!(
                    "The page did not answer within {} s; it may \
                     have moved to another page.",
                    ACT_WITHIN.as_secs()
                ),
                true,
            );
        }
    }
}
