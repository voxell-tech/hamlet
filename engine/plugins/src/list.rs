//! The plugin list: every plugin, whether it is on, a button to
//! switch it, and for an installed one a button to uninstall it. A
//! button above asks for a plugin to be added.

use bevy::app::{App, Startup, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::event::{EntityEvent, Event};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Changed, Has, Or};
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, ghost, label, row,
};
use bevy_fynix::{ModifierExt, ScopedExt, ViewExt, keyed, resource};
use hamlet_engine_api::PanelId;
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_shell::dock::PLUGIN_LIST;
use hamlet_engine_shell::{AnyView, Panel, panel};

use crate::PluginInfo;
use crate::switch::{
    Contributions, Disable, Enable, Enabled, Stopped,
};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Rows>()
        .add_systems(Startup, open)
        .add_systems(Update, follow);
}

/// The plugins as the list shows them.
#[derive(Resource, Clone, PartialEq, Default)]
struct Rows(Vec<Row>);

#[derive(Clone, PartialEq)]
struct Row {
    plugin: Entity,
    name: String,
    /// Its id and version.
    about: String,
    on: bool,
    /// How many things it adds.
    adds: usize,
    /// Why the engine stopped it.
    stopped: Option<String>,
    builtin: bool,
}

/// Asks for a plugin to be added: a host offers the person a folder
/// to add one from.
#[derive(Event, Clone, Copy, Debug)]
pub struct AddPlugin;

/// Asks for the installed plugin to be taken away, with its files and
/// what the person answered for it.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct Uninstall {
    pub entity: Entity,
}

fn open(mut commands: Commands) {
    commands.spawn(Panel {
        id: PanelId::new(ENGINE, PLUGIN_LIST).expect("a valid id"),
        title: "Plugins".into(),
        content: view,
    });
}

fn view(_: Entity) -> AnyView {
    let add = button(label("ADD PLUGIN..."))
        .rules(ghost)
        .on_activate(|world| world.trigger(AddPlugin));
    let rows = keyed(resource::<Rows, _>(Clone::clone), |rows| {
        if rows.0.is_empty() {
            return label("No plugins.").tone(Tone::Dim).boxed();
        }
        column(rows.0.iter().map(row_view).collect::<Vec<_>>())
            .gap(12.0)
            .boxed()
    });
    panel(column((row((add,)), rows)).gap(16.0))
}

fn row_view(row_of: &Row) -> AnyView {
    let plugin = row_of.plugin;
    let on = row_of.on;
    let state = match (&row_of.stopped, on) {
        (Some(why), _) => format!("Stopped: {why}"),
        (None, true) => format!("On, adds {}", row_of.adds),
        (None, false) => "Off".into(),
    };
    let switch =
        button(label(if on { "Switch off" } else { "Switch on" }))
            .rules(ghost)
            .on_activate(move |world| {
                if on {
                    world.trigger(Disable { entity: plugin });
                } else {
                    world.trigger(Enable { entity: plugin });
                }
            })
            .boxed();
    let mut buttons = vec![switch];
    if !row_of.builtin {
        buttons.push(
            button(label("Uninstall"))
                .rules(ghost)
                .on_activate(move |world| {
                    world.trigger(Uninstall { entity: plugin });
                })
                .boxed(),
        );
    }
    row((
        column((
            label(row_of.name.clone()),
            label(format!("{}  {state}", row_of.about))
                .tone(Tone::Dim)
                .size(12.0),
        ))
        .gap(2.0)
        .grown(1.0),
        row(buttons).gap(4.0),
    ))
    .align(AlignItems::Center)
    .min_height(px(0.0))
    .boxed()
}

/// Gathers the rows again when a plugin comes, goes, or is switched.
fn follow(
    changed: Query<
        (),
        Or<(
            Changed<PluginInfo>,
            Changed<Enabled>,
            Changed<Contributions>,
            Changed<Stopped>,
        )>,
    >,
    mut gone: RemovedComponents<PluginInfo>,
    mut off: RemovedComponents<Enabled>,
    mut empty: RemovedComponents<Contributions>,
    mut restarted: RemovedComponents<Stopped>,
    plugins: Query<(
        Entity,
        &PluginInfo,
        Has<Enabled>,
        Option<&Contributions>,
        Option<&Stopped>,
    )>,
    mut rows: ResMut<Rows>,
) {
    let went = gone.read().count()
        + off.read().count()
        + empty.read().count()
        + restarted.read().count();
    if changed.is_empty() && went == 0 {
        return;
    }
    let mut now = plugins
        .iter()
        .map(|(plugin, info, on, adds, stopped)| Row {
            plugin,
            name: info.name.clone(),
            about: format!("{}  {}", info.id, info.version),
            on,
            adds: adds.map_or(0, |adds| adds.len()),
            stopped: stopped.map(|stopped| stopped.0.clone()),
            builtin: info.builtin,
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| a.name.cmp(&b.name));
    if rows.0 != now {
        rows.0 = now;
    }
}
