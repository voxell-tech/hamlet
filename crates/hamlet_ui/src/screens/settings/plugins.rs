//! The Plugins page: every plugin there is, each with a switch.

use bevy::app::{App, Update};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy::ui::AlignItems;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_sdk::plugin::{Disable, Enable, PluginInfo, Status};

use super::switch;
use crate::looks::{Fills, bare, note};
use crate::platform::DisplayText;
use crate::{AnyView, View};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Listed>()
        .add_systems(Update, list.run_if(moved));
}

/// One plugin, as its page shows it.
#[derive(Clone, PartialEq)]
struct Line {
    id: String,
    name: String,
    version: String,
    builtin: bool,
    status: Status,
}

/// Every plugin, by name.
#[derive(Resource, Default, PartialEq)]
struct Listed(Vec<Line>);

fn moved(
    changed: Query<(), Or<(Added<PluginInfo>, Changed<Status>)>>,
    mut gone: RemovedComponents<PluginInfo>,
) -> bool {
    !changed.is_empty() || gone.read().count() > 0
}

fn list(
    plugins: Query<(&PluginInfo, &Status)>,
    mut listed: ResMut<Listed>,
) {
    let mut lines = plugins
        .iter()
        .map(|(info, status)| Line {
            id: info.id.clone(),
            name: info.name.clone(),
            version: info.version.clone(),
            builtin: info.builtin,
            status: *status,
        })
        .collect::<Vec<_>>();
    lines.sort_by(|a, b| a.name.cmp(&b.name));
    if listed.0 != lines {
        listed.0 = lines;
    }
}

fn line(line: &Line) -> AnyView {
    let id = line.id.clone();
    let status = match line.status {
        Status::Starting => "Starting",
        Status::Running => "Running",
        Status::Disabled => "Disabled",
        Status::Failed => "Failed",
    };
    let kind = if line.builtin {
        "Built in"
    } else {
        "Installed"
    };
    let words = column((
        label(line.name.clone())
            .size(13.0)
            .bold(true)
            .tagged(DisplayText),
        note(format!("{} - {kind} - {status}", line.version)),
    ))
    .gap(2.0);
    row((
        row((words,)).fills_width(),
        switch(
            matches!(line.status, Status::Starting | Status::Running),
            move |world, on| {
                if on {
                    world.trigger(Enable(id.clone()));
                } else {
                    world.trigger(Disable(id.clone()));
                }
                world.flush();
            },
        ),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The page's contents.
pub(super) fn page() -> impl View {
    keyed(resource::<Listed, _>(|l| l.0.clone()), |lines| {
        if lines.is_empty() {
            return note("No plugin is added.").boxed();
        }
        column(lines.iter().map(line).collect::<Vec<_>>())
            .gap(12.0)
            .boxed()
    })
    .within(bare())
}
