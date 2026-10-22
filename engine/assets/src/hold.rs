//! References to assets. A plugin that holds an asset is told when it
//! is at a new version or taken away, so what it shows of it stays
//! live. What a plugin holds is let go when it is switched off.

use std::collections::BTreeSet;

use bevy::app::App;
use bevy::ecs::change_detection::Mut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::system::Query;
use hamlet_engine_api::{AssetId, Message};
use hamlet_engine_plugins::{Deliver, Disable, PluginInfo};

pub(crate) fn plugin(app: &mut App) {
    app.register_required_components::<PluginInfo, Holding>()
        .add_observer(let_go);
}

/// On a plugin: the assets it holds.
#[derive(Component, Default, Debug)]
pub struct Holding(pub BTreeSet<AssetId>);

/// A change to an asset: its new version, or `None` when it is gone,
/// and the depth of the request that made it.
pub(crate) type Change = (AssetId, Option<u64>, u32);

fn let_go(event: On<Disable>, mut holding: Query<&mut Holding>) {
    if let Ok(mut holding) = holding.get_mut(event.entity) {
        holding.0.clear();
    }
}

/// Tells each plugin that holds an asset of `changes` of it. One that
/// went is no longer held.
pub(crate) fn tell<'a>(
    changes: &[Change],
    holders: impl Iterator<Item = (Entity, Mut<'a, Holding>)>,
    deliver: &mut MessageWriter<Deliver>,
) {
    for (plugin, mut holding) in holders {
        for &(asset, version, depth) in changes {
            if !holding.0.contains(&asset) {
                continue;
            }
            let message = match version {
                Some(version) => Message::Changed { asset, version },
                None => {
                    holding.0.remove(&asset);
                    Message::Gone(asset)
                }
            };
            deliver.write(Deliver {
                plugin,
                message,
                depth,
            });
        }
    }
}
