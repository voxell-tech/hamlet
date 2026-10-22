//! The plugin list: every plugin, whether it is on, a switch beside
//! its name, and for an installed one a button to uninstall it; one
//! that comes with Hamlet says so. Under each, its limits fold out.
//! A button above asks for a plugin to be added.

mod limits;

use std::collections::BTreeSet;

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::{EntityEvent, Event};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Changed, Has, Or};
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{PanelId, PluginId};
use hamlet_engine_core::{LogLine, PluginBudgets};
use hamlet_engine_shell::dock::{PLUGIN_LIST, PanelIcon};
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::style::{
    ButtonStyle, Wraps, buttons, icon_button, list, note,
    scroll_area, toggle,
};
use hamlet_engine_shell::{AnyView, Cx, Panel, StyleExt};

use self::limits::{LimitLine, limits_view, lines};
use crate::limits::ChosenLimits;
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
struct Rows {
    plugins: Vec<Row>,
    /// The plugins whose limits are folded out.
    unfolded: BTreeSet<Entity>,
    /// A value typed for a limit of a plugin that was refused, and
    /// why.
    refused: Option<(Entity, String)>,
}

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
    limits: Vec<LimitLine>,
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
        if rows.plugins.is_empty() {
            return note("No plugins.");
        }
        list(
            rows.plugins
                .iter()
                .map(|row_of| row_view(row_of, rows))
                .collect::<Vec<_>>(),
        )
    })
    .within(column(()).gap(0.0));
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.section;
        cx.build(
            column((
                buttons(vec![add.boxed()]),
                scroll_area((rows,)),
            ))
            .gap(gap)
            .grow(1.0)
            .min_height(px(0.0)),
        )
    })
    .in_panel()
}

/// A plugin: its name over its id, version and state, with its
/// switch beside them, its buttons, and its limits while `rows` has
/// them folded out.
fn row_view(row_of: &Row, rows: &Rows) -> AnyView {
    let plugin = row_of.plugin;
    let id = row_of.id.clone();
    let unfolded = rows.unfolded.contains(&plugin);
    let shown_limits = unfolded.then(|| {
        let refused = rows
            .refused
            .as_ref()
            .filter(|(at, _)| *at == plugin)
            .map(|(_, why)| why.clone());
        limits_view(plugin, &row_of.id, &row_of.limits, refused)
    });
    let (state, tone) = match (&row_of.stopped, row_of.on) {
        (Some(why), _) => (format!("Stopped: {why}"), Tone::Critical),
        (None, true) => {
            (format!("On, adds {}", row_of.adds), Tone::Dim)
        }
        (None, false) => ("Off".into(), Tone::Dim),
    };
    let switch = toggle(row_of.on, move |world, on| {
        // Only the person's own switch is kept for the next start.
        let kept = world.resource_mut::<SwitchedOff>().set(&id, !on);
        if let Err(why) = kept {
            world.write_message(LogLine::error("plugins", why));
        }
        if on {
            world.trigger(Enable { entity: plugin });
        } else {
            world.trigger(Disable { entity: plugin });
        }
    });
    let mut actions = Vec::new();
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
    let fold = if unfolded {
        Glyph::ChevronUp
    } else {
        Glyph::ChevronRight
    };
    actions.push(
        icon_button(ButtonStyle::Quiet, fold, "LIMITS")
            .on_activate(move |world| {
                let mut rows = world.resource_mut::<Rows>();
                if !rows.unfolded.remove(&plugin) {
                    rows.unfolded.insert(plugin);
                }
            })
            .boxed(),
    );
    let (name, about) = (row_of.name.clone(), row_of.about.clone());
    let bundled = row_of.kind == PluginKind::Bundled;
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, size, section) =
            (theme.space.gap, theme.text.small, theme.space.section);
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
        let head = row((
            column((
                label(name).bold(true).wrap(false),
                row(facts).gap(gap).tagged(Wraps),
            ))
            .gap(gap / 2.0)
            .grow(1.0)
            .min_width(px(0.0)),
            switch,
        ))
        .gap(section)
        .align(AlignItems::Start);
        let mut parts = vec![head.boxed(), buttons(actions)];
        parts.extend(shown_limits);
        cx.build(column(parts).gap(gap / 2.0))
    })
}

/// Gathers the rows again when a plugin comes, goes, is switched or
/// is held to other limits.
fn follow(
    changed: Query<
        (),
        Or<(
            Changed<PluginInfo>,
            Changed<Enabled>,
            Changed<Contributions>,
            Changed<Stopped>,
            Changed<PluginBudgets>,
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
        Option<&PluginBudgets>,
    )>,
    chosen: Res<ChosenLimits>,
    mut rows: ResMut<Rows>,
) {
    let went = gone.read().count()
        + off.read().count()
        + empty.read().count()
        + restarted.read().count();
    if changed.is_empty() && went == 0 && !chosen.is_changed() {
        return;
    }
    let mut now = plugins
        .iter()
        .map(|(plugin, info, on, adds, stopped, budgets)| Row {
            plugin,
            id: info.id.clone(),
            name: info.name.clone(),
            about: format!("{}  {}", info.id, info.version),
            on,
            adds: adds.map_or(0, |adds| adds.len()),
            stopped: stopped.map(|stopped| stopped.0.clone()),
            kind: info.kind,
            limits: budgets.map_or_else(Vec::new, |budgets| {
                lines(budgets, &info.limits, chosen.get(&info.id))
            }),
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| a.name.cmp(&b.name));
    if rows.plugins != now {
        rows.plugins = now;
    }
    if rows
        .unfolded
        .iter()
        .any(|plugin| !plugins.contains(*plugin))
    {
        rows.unfolded.retain(|plugin| plugins.contains(*plugin));
    }
}
