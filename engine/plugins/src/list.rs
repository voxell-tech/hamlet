//! The plugin list: every plugin, whether it is on, a button to
//! switch it, and for an installed one a button to uninstall it; one
//! that comes with Hamlet says so. A button above asks for a plugin
//! to be added.

use bevy::app::{App, Startup, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::event::{EntityEvent, Event};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Changed, Has, Or};
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{PanelId, PluginId};
use hamlet_engine_core::LogLine;
use hamlet_engine_shell::dock::{PLUGIN_LIST, PanelIcon};
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::style::{
    ButtonStyle, buttons, icon_button, list, note, scroll_area,
};
use hamlet_engine_shell::{AnyView, Cx, Panel, StyleExt};

use crate::switch::{
    Contributions, Disable, Enable, Enabled, Stopped,
};
use crate::switched_off::SwitchedOff;
use crate::{PluginInfo, PluginKind};

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
    id: PluginId,
    name: String,
    /// Its id and version.
    about: String,
    on: bool,
    /// How many things it adds.
    adds: usize,
    /// Why the engine stopped it.
    stopped: Option<String>,
    kind: PluginKind,
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
    commands.spawn((
        Panel {
            id: PanelId::new(ENGINE, PLUGIN_LIST)
                .expect("a valid id"),
            title: "Plugins".into(),
            content: view,
        },
        PanelIcon(Glyph::Chip),
    ));
}

/// ADD PLUGIN... over the list of plugins.
fn view(_: Entity) -> AnyView {
    let add =
        icon_button(ButtonStyle::Plain, Glyph::Plus, "ADD PLUGIN...")
            .on_activate(|world| world.trigger(AddPlugin));
    let rows = keyed(resource::<Rows, _>(Clone::clone), |rows| {
        if rows.0.is_empty() {
            return note("No plugins.");
        }
        list(rows.0.iter().map(row_view).collect::<Vec<_>>())
    })
    .within(column(()).gap(0.0));
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.section;
        cx.build(
            column((
                buttons(vec![add.boxed()]),
                scroll_area((rows,)),
            ))
            .gap(gap),
        )
    })
    .in_panel()
}

/// A plugin: its name, its id, version and state, and its buttons.
fn row_view(row_of: &Row) -> AnyView {
    let plugin = row_of.plugin;
    let id = row_of.id.clone();
    let on = row_of.on;
    let (state, tone) = match (&row_of.stopped, on) {
        (Some(why), _) => (format!("Stopped: {why}"), Tone::Critical),
        (None, true) => {
            (format!("On, adds {}", row_of.adds), Tone::Dim)
        }
        (None, false) => ("Off".into(), Tone::Dim),
    };
    let switch = if on {
        icon_button(ButtonStyle::Plain, Glyph::Power, "SWITCH OFF")
    } else {
        icon_button(ButtonStyle::Plain, Glyph::Power, "SWITCH ON")
    }
    .on_activate(move |world| {
        // Only the person's own switch is kept for the next start.
        let kept = world.resource_mut::<SwitchedOff>().set(&id, on);
        if let Err(why) = kept {
            world.write_message(LogLine::error("plugins", why));
        }
        if on {
            world.trigger(Disable { entity: plugin });
        } else {
            world.trigger(Enable { entity: plugin });
        }
    })
    .boxed();
    let mut actions = vec![switch];
    if row_of.kind == PluginKind::Installed {
        actions.push(
            icon_button(
                ButtonStyle::Critical,
                Glyph::Bin,
                "UNINSTALL",
            )
            .on_activate(move |world| {
                world.trigger(Uninstall { entity: plugin });
            })
            .boxed(),
        );
    }
    let (name, about) = (row_of.name.clone(), row_of.about.clone());
    let bundled = row_of.kind == PluginKind::Bundled;
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, size) = (theme.space.gap, theme.text.small);
        let mut facts = vec![
            label(about)
                .size(size)
                .tone(Tone::Dim)
                .wrap(false)
                .boxed(),
            label(state).size(size).tone(tone).boxed(),
        ];
        if bundled {
            facts.push(
                label("Comes with Hamlet")
                    .size(size)
                    .tone(Tone::Accent)
                    .boxed(),
            );
        }
        cx.build(
            column((
                label(name).bold(true).wrap(false),
                row(facts).gap(gap),
                buttons(actions),
            ))
            .gap(gap / 2.0),
        )
    })
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
            id: info.id.clone(),
            name: info.name.clone(),
            about: format!("{}  {}", info.id, info.version),
            on,
            adds: adds.map_or(0, |adds| adds.len()),
            stopped: stopped.map(|stopped| stopped.0.clone()),
            kind: info.kind,
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| a.name.cmp(&b.name));
    if rows.0 != now {
        rows.0 = now;
    }
}
