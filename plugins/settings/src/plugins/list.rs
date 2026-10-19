//! Every plugin, by name, each with its switch, or with the button
//! that installs it.

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Query, ResMut};
use bevy::ui::AlignItems;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_sdk::capability::{
    AskToInstall, AskToRemove, Asks, Granted, Uninstalled,
};
use hamlet_sdk::command::Commands;
use hamlet_sdk::context::ContextItems;
use hamlet_sdk::inspector::InspectorSections;
use hamlet_sdk::keys::Keys;
use hamlet_sdk::plugin::{
    Blocked, Disable, Enable, PluginInfo, Status,
};
use hamlet_sdk::reload::{Reload, Rollback};
use hamlet_sdk::settings::SettingsPages;

use super::adds::{self, Adds, Contributed};
use super::links::{self, Everyone, Links};
use super::{Open, moved, switch};
use crate::icons::Glyph;
use crate::looks::{Fills, action, bare, buttons, good_action, note};
use crate::platform::DisplayText;
use crate::{AnyView, View};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Listed>().add_systems(
        Update,
        list.run_if(
            moved
                .or_eager(resource_changed::<Commands>)
                .or_eager(resource_changed::<Keys>)
                .or_eager(resource_changed::<ContextItems>)
                .or_eager(resource_changed::<InspectorSections>)
                .or_eager(resource_changed::<SettingsPages>),
        ),
    );
}

/// One plugin, as its page shows it.
#[derive(Clone, PartialEq)]
struct Line {
    id: String,
    name: String,
    version: String,
    builtin: bool,
    essential: bool,
    status: Status,
    installed: bool,
    /// The capabilities its manifest asks for.
    asks: usize,
    /// Why the files on disk are not what runs, and why the plugin
    /// is held back for what it needs.
    notes: Vec<String>,
    /// Whether it has a last good version to go back to.
    rollback: bool,
    /// The plugins it needs and the plugins that need it.
    links: Links,
    /// What it adds to the window.
    adds: Adds,
}

/// Every plugin, by name.
#[derive(Resource, Default, PartialEq)]
struct Listed(Vec<Line>);

fn list(
    plugins: Query<(
        Entity,
        &PluginInfo,
        &Status,
        Option<&Asks>,
        Has<Uninstalled>,
        Option<&Reload>,
        Option<&Blocked>,
        Option<&Granted>,
    )>,
    everyone: Everyone,
    from: Contributed,
    mut listed: ResMut<Listed>,
) {
    let mut lines = plugins
        .iter()
        .map(
            |(
                plugin,
                info,
                status,
                asks,
                uninstalled,
                reload,
                blocked,
                granted,
            )| Line {
                id: info.id.clone(),
                name: info.name.clone(),
                version: info.version.clone(),
                builtin: info.builtin,
                essential: info.essential,
                status: *status,
                installed: !uninstalled,
                asks: asks.map_or(0, |asks| asks.0.len()),
                notes: reload
                    .and_then(|reload| reload.note.clone())
                    .into_iter()
                    .chain(blocked.map(|blocked| blocked.0.clone()))
                    .collect(),
                rollback: reload
                    .is_some_and(|reload| reload.rollback),
                links: links::of(&info.id, &everyone),
                adds: adds::of(plugin, info, granted, &from),
            },
        )
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
    let kind = match (line.essential, line.builtin) {
        (true, _) => "Essential",
        (_, true) => "Built in",
        _ => "Installed",
    };
    let mut words = vec![
        label(line.name.clone())
            .size(13.0)
            .bold(true)
            .tagged(DisplayText)
            .boxed(),
        note(format!("{} - {kind} - {status}", line.version)).boxed(),
    ];
    words.extend(links::lines(&line.links));
    words.extend(adds::lines(&line.adds));
    for why in &line.notes {
        words.push(note(why.clone()).boxed());
    }
    let words = column(words).gap(2.0);
    let (id, rows) = (line.id.clone(), line.asks);
    let mut actions = Vec::new();
    if line.rollback {
        let id = id.clone();
        actions.push(action(
            Glyph::Restore,
            "ROLL BACK",
            move |world| {
                world.trigger(Rollback { plugin: id.clone() });
                world.flush();
            },
        ));
    }
    if line.installed && line.status == Status::Failed {
        let id = id.clone();
        actions.push(action(
            Glyph::Restore,
            "RESTART",
            move |world| {
                world.trigger(Enable(id.clone()));
                world.flush();
            },
        ));
    }
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
    if !line.builtin {
        let id = id.clone();
        actions.push(action(
            Glyph::Delete,
            "UNINSTALL",
            move |world| {
                world.trigger(AskToRemove { plugin: id.clone() });
                world.flush();
            },
        ));
    }
    let mut cells = vec![
        column((words, buttons(actions)))
            .gap(6.0)
            .fills_width()
            .boxed(),
    ];
    if line.essential {
    } else if line.installed {
        let on =
            matches!(line.status, Status::Starting | Status::Running);
        cells.push(
            switch(on, move |world, on| {
                if on {
                    world.trigger(Enable(id.clone()));
                } else {
                    world.trigger(Disable(id.clone()));
                }
                world.flush();
            })
            .boxed(),
        );
    } else {
        cells.push(good_action(
            Glyph::Plus,
            "INSTALL",
            move |world| {
                world.trigger(AskToInstall { plugin: id.clone() });
                world.flush();
            },
        ));
    }
    row(cells).gap(12.0).align(AlignItems::Center).boxed()
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
