//! Every plugin, by name, each with its switch, or with the button
//! that installs it.

use bevy::app::{App, Update};
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy::ui::AlignItems;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_sdk::capability::{AskToInstall, Asks, Uninstalled};
use hamlet_sdk::plugin::{Disable, Enable, PluginInfo, Status};

use super::{Open, moved, switch};
use crate::icons::Glyph;
use crate::looks::{Fills, action, bare, buttons, good_action, note};
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
    installed: bool,
    /// The capabilities its manifest asks for.
    asks: usize,
}

/// Every plugin, by name.
#[derive(Resource, Default, PartialEq)]
struct Listed(Vec<Line>);

fn list(
    plugins: Query<(
        &PluginInfo,
        &Status,
        Option<&Asks>,
        Has<Uninstalled>,
    )>,
    mut listed: ResMut<Listed>,
) {
    let mut lines = plugins
        .iter()
        .map(|(info, status, asks, uninstalled)| Line {
            id: info.id.clone(),
            name: info.name.clone(),
            version: info.version.clone(),
            builtin: info.builtin,
            status: *status,
            installed: !uninstalled,
            asks: asks.map_or(0, |asks| asks.0.len()),
        })
        .collect::<Vec<_>>();
    lines.sort_by(|a, b| a.name.cmp(&b.name));
    if listed.0 != lines {
        listed.0 = lines;
    }
}

fn line(line: &Line) -> AnyView {
    let status = match (line.installed, line.status) {
        (false, _) => "Not installed",
        (_, Status::Starting) => "Starting",
        (_, Status::Running) => "Running",
        (_, Status::Disabled) => "Disabled",
        (_, Status::Failed) => "Failed",
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
    let (id, rows) = (line.id.clone(), line.asks);
    let mut actions = Vec::new();
    if rows > 0 {
        let id = id.clone();
        actions.push(action(
            Glyph::Shield,
            "PERMISSIONS",
            move |world| {
                *world.resource_mut::<Open>() =
                    Open(Some(id.clone()));
            },
        ));
    }
    let control = if line.installed {
        let on =
            matches!(line.status, Status::Starting | Status::Running);
        switch(on, move |world, on| {
            if on {
                world.trigger(Enable(id.clone()));
            } else {
                world.trigger(Disable(id.clone()));
            }
            world.flush();
        })
        .boxed()
    } else {
        good_action(Glyph::Plus, "INSTALL", move |world| {
            world.trigger(AskToInstall { plugin: id.clone() });
            world.flush();
        })
    };
    row((
        column((words, buttons(actions))).gap(6.0).fills_width(),
        control,
    ))
    .gap(12.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The list of plugins.
pub(super) fn view() -> impl View {
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
