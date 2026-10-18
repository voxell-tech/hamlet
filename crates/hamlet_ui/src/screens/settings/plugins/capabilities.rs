//! Each capability, with the plugins that ask for it and what the
//! chief chose for each.

use bevy::app::{App, Update};
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_sdk::capability::{
    Asks, Capability, Choice, Choices, Uninstalled,
};
use hamlet_sdk::plugin::PluginInfo;

use super::control::control;
use super::moved;
use crate::looks::{Fills, bare, hairline, note};
use crate::platform::DisplayText;
use crate::{AnyView, View, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Held>()
        .add_systems(Update, keep.run_if(moved));
}

/// A plugin that asks for a capability.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Holder {
    pub id: String,
    pub name: String,
    pub choice: Option<Choice>,
    pub installed: bool,
}

/// A capability and the plugins that ask for it.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Group {
    pub capability: Capability,
    pub holders: Vec<Holder>,
}

/// The capabilities some plugin asks for, in the order they are
/// named.
#[derive(Resource, Default, PartialEq, Debug)]
pub(super) struct Held(pub Vec<Group>);

fn keep(
    plugins: Query<(&PluginInfo, &Asks, &Choices, Has<Uninstalled>)>,
    mut held: ResMut<Held>,
) {
    let mut now = Capability::ALL
        .into_iter()
        .map(|capability| Group {
            capability,
            holders: plugins
                .iter()
                .filter(|(_, asks, ..)| asks.names(capability))
                .map(|(info, _, choices, uninstalled)| Holder {
                    id: info.id.clone(),
                    name: info.name.clone(),
                    choice: choices.choice(capability),
                    installed: !uninstalled,
                })
                .collect(),
        })
        .filter(|group| !group.holders.is_empty())
        .collect::<Vec<_>>();
    for group in &mut now {
        group.holders.sort_by(|a, b| a.name.cmp(&b.name));
    }
    if held.0 != now {
        held.0 = now;
    }
}

fn group(group: &Group) -> AnyView {
    let holders = group
        .holders
        .iter()
        .map(|holder| {
            let name = label(holder.name.clone())
                .size(12.0)
                .bold(true)
                .tagged(DisplayText);
            let state = if holder.installed {
                control(&holder.id, group.capability, holder.choice)
            } else {
                note("Not installed").boxed()
            };
            column((name, state)).gap(4.0).boxed()
        })
        .collect::<Vec<_>>();
    column((
        row((label(group.capability.name())
            .size(13.0)
            .bold(true)
            .tagged(DisplayText),))
        .fills_width(),
        note(group.capability.words()),
        column(holders).gap(8.0),
    ))
    .gap(4.0)
    .boxed()
}

/// The capabilities, each over the plugins that ask for it.
pub(super) fn view() -> impl View {
    keyed(resource::<Held, _>(|held| held.0.clone()), |groups| {
        if groups.is_empty() {
            return note("No plugin asks for anything.").boxed();
        }
        let mut parts = Vec::new();
        for group in groups {
            if !parts.is_empty() {
                parts.push(with_cx(|cx| {
                    hairline(cx.theme().palette.brass_6)
                }));
            }
            parts.push(self::group(group));
        }
        column(parts).gap(12.0).boxed()
    })
    .within(bare())
}
