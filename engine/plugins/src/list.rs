//! The Plugins panel. Its list shows every plugin, whether it is on,
//! a switch beside its name, and that it comes with Hamlet if it
//! does; a button above asks for a plugin to be added, and a search
//! and a filter narrow the list. A plugin's row opens its page in the
//! panel, under a breadcrumb back to the list.

mod limits;
mod page;

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::{EntityEvent, Event};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Changed, Has, Or};
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{Signal, ViewExt, each, keyed, resource};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{PanelId, PluginId};
use hamlet_engine_core::{LogLine, PluginBudgets};
use hamlet_engine_shell::dock::{PLUGIN_LIST, PanelIcon};
use hamlet_engine_shell::icons::{Glyph, glyph};
use hamlet_engine_shell::style::{
    ButtonStyle, Wraps, buttons, divided_row, dropdown, field,
    icon_button, note, scroll_area, switch,
};
use hamlet_engine_shell::{AnyView, Cx, Panel, StyleExt};

use crate::limits::ChosenLimits;
use crate::list::limits::{MemoryLine, memory_line};
pub use crate::list::page::PluginPageSection;
use crate::list::page::{Added, Things, added, page_view};
use crate::switch::{
    Contributions, Disable, Enable, Enabled, Stopped,
};
use crate::switched_off::SwitchedOff;
use crate::{PluginInfo, PluginKind};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Rows>()
        .init_resource::<Shown>()
        .add_systems(Startup, open)
        .add_systems(Update, follow);
}

/// The plugins as the panel shows them, and what the person searched
/// for and picked to see, kept until the engine closes.
#[derive(Resource, Clone, PartialEq, Default)]
struct Rows {
    plugins: Vec<Row>,
    /// A value typed for the memory of a plugin that was refused,
    /// and why.
    refused: Option<(Entity, String)>,
    /// The plugin whose UNINSTALL was pressed, waiting to be
    /// confirmed.
    uninstalling: Option<Entity>,
    /// Words a plugin's name or id holds, of any case, to be shown.
    search: String,
    filter: Filter,
}

/// The page the Plugins panel shows.
#[derive(Resource, Clone, PartialEq, Eq, Default, Debug)]
enum Shown {
    #[default]
    List,
    /// The page of the plugin of this id.
    Plugin(PluginId),
}

/// Which plugins the list shows.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Filter {
    #[default]
    All,
    On,
    Off,
    Bundled,
    Installed,
}

impl Filter {
    /// Every filter, in the order the list offers them.
    const ALL: [Filter; 5] = [
        Filter::All,
        Filter::On,
        Filter::Off,
        Filter::Bundled,
        Filter::Installed,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::On => "On",
            Self::Off => "Off",
            Self::Bundled => "Comes with Hamlet",
            Self::Installed => "Installed",
        }
    }

    fn takes(self, row: &Row) -> bool {
        match self {
            Self::All => true,
            Self::On => row.on,
            Self::Off => !row.on,
            Self::Bundled => row.kind == PluginKind::Bundled,
            Self::Installed => row.kind == PluginKind::Installed,
        }
    }
}

impl Rows {
    /// The plugins the search and the filter let through.
    fn shown(&self) -> impl Iterator<Item = &Row> {
        let search = self.search.trim().to_lowercase();
        self.plugins.iter().filter(move |row| {
            self.filter.takes(row)
                && (row.name.to_lowercase().contains(&search)
                    || row.id.as_str().contains(&search))
        })
    }

    /// The plugin of the id `id`.
    fn find(&self, id: &PluginId) -> Option<&Row> {
        self.plugins.iter().find(|row| row.id == *id)
    }
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
    /// What it adds, as its page lists it.
    added: Vec<Added>,
    /// Why the engine stopped it.
    stopped: Option<String>,
    kind: PluginKind,
    memory: Option<MemoryLine>,
}

impl Row {
    /// Whether it is on and how much it adds, or why it was stopped,
    /// in its tone.
    fn state(&self) -> (String, Tone) {
        match (&self.stopped, self.on) {
            (Some(why), _) => {
                (format!("Stopped: {why}"), Tone::Critical)
            }
            (None, true) => {
                (format!("On, adds {}", self.adds), Tone::Dim)
            }
            (None, false) => ("Off".into(), Tone::Dim),
        }
    }
}

/// What `read` gives of the row of `plugin`, kept as the rows change.
fn bound<T: Default + 'static>(
    plugin: Entity,
    read: impl Fn(&Row) -> T + Send + Sync + 'static,
) -> Signal<T> {
    resource::<Rows, _>(move |rows| {
        rows.plugins
            .iter()
            .find(|row| row.plugin == plugin)
            .map(&read)
            .unwrap_or_default()
    })
}

/// The switch that turns `plugin`, of the id `id`, on or off.
fn plugin_switch(plugin: Entity, id: PluginId) -> AnyView {
    switch(bound(plugin, |row| row.on), move |world, on| {
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
    })
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

/// The list, or the page of the plugin shown.
fn view(_: Entity) -> AnyView {
    keyed(resource::<Shown, _>(Clone::clone), |shown| match shown {
        Shown::List => list_view(),
        Shown::Plugin(id) => page_view(id.clone()),
    })
    .within(column(()).gap(0.0).grow(1.0).min_height(px(0.0)))
    .in_panel()
}

/// ADD PLUGIN... and a search with a filter over the list of plugins.
fn list_view() -> AnyView {
    let add =
        icon_button(ButtonStyle::Plain, Glyph::Plus, "ADD PLUGIN...")
            .on_activate(|world| world.trigger(AddPlugin));
    let search = field(
        resource::<Rows, _>(|rows| rows.search.clone()),
        "Search plugins".into(),
        |world, typed| {
            let mut rows = world.resource_mut::<Rows>();
            if rows.search != typed {
                rows.search = typed;
            }
        },
    );
    let filter = dropdown(
        Filter::ALL.iter().map(|at| at.name().to_owned()).collect(),
        resource::<Rows, _>(|rows| {
            Filter::ALL
                .iter()
                .position(|at| *at == rows.filter)
                .unwrap_or(0)
        }),
        |world, at| {
            let picked =
                Filter::ALL.get(at).copied().unwrap_or_default();
            let mut rows = world.resource_mut::<Rows>();
            if rows.filter != picked {
                rows.filter = picked;
            }
        },
    );
    // Built again only as the list empties or fills; each row follows
    // its plugin in place.
    let rows = keyed(
        resource::<Rows, _>(|rows| {
            (rows.plugins.is_empty(), rows.shown().next().is_none())
        }),
        |&(none, unmatched)| {
            if none {
                return note("No plugins.");
            }
            if unmatched {
                return note("No plugin matches.");
            }
            each(
                resource::<Rows, _>(|rows| {
                    rows.shown()
                        .map(|row| {
                            (
                                row.plugin,
                                row.id.clone(),
                                row.kind == PluginKind::Bundled,
                            )
                        })
                        .collect::<Vec<_>>()
                }),
                |(plugin, ..)| *plugin,
                |(plugin, id, bundled)| {
                    row_view(*plugin, id.clone(), *bundled)
                },
            )
            .within(column(()).gap(0.0))
            .boxed()
        },
    )
    .within(column(()).gap(0.0));
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, section, side) =
            (theme.space.gap, theme.space.section, theme.space.row);
        let finding = row((
            column((search,)).gap(0.0).grow(1.0).min_width(px(0.0)),
            column((filter,)).gap(0.0).width(px(side * 6.0)),
        ))
        .gap(gap)
        .align(AlignItems::Center);
        cx.build(
            column((
                buttons(vec![add.boxed()]),
                finding,
                scroll_area((rows,)),
            ))
            .gap(section)
            .grow(1.0)
            .min_height(px(0.0)),
        )
    })
}

/// The row of `plugin`, of the id `id`: its name over its id, version
/// and state, which opens its page, with its switch beside them, all
/// but the switch dim while it is off. It follows the plugin in
/// place, under a divider unless it is the first shown.
fn row_view(plugin: Entity, id: PluginId, bundled: bool) -> AnyView {
    let switch = plugin_switch(plugin, id.clone());
    let divided = resource::<Rows, _>(move |rows| {
        rows.shown().next().is_some_and(|row| row.plugin != plugin)
    });
    let content = AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, size, icon, section) = (
            theme.space.gap,
            theme.text.small,
            theme.space.icon,
            theme.space.section,
        );
        let mut facts = vec![
            label(bound(plugin, |row| row.about.clone()))
                .size(size)
                .tone(Tone::Dim)
                .wrap(false)
                .boxed(),
            label(bound(plugin, |row| row.state().0))
                .size(size)
                .tone(bound(plugin, |row| row.state().1))
                .boxed(),
        ];
        if bundled {
            let tone = bound(plugin, |row| {
                if row.on { Tone::Accent } else { Tone::Dim }
            });
            facts.push(
                label("Comes with Hamlet")
                    .size(size)
                    .tone(tone)
                    .boxed(),
            );
        }
        let name = keyed(bound(plugin, |row| row.on), move |&on| {
            let name = label(bound(plugin, |row| row.name.clone()))
                .bold(true)
                .wrap(false);
            // Left in the row's tone, which lightens under the
            // pointer, while it is on.
            if on {
                name.boxed()
            } else {
                name.tone(Tone::Dim).boxed()
            }
        });
        // The whole row opens the page; the switch, a button of its
        // own, keeps its presses.
        let head = row((name, column(()).grow(1.0), switch))
            .gap(gap)
            .align(AlignItems::Center);
        cx.build(
            row((
                column((head, row(facts).gap(gap).tagged(Wraps)))
                    .gap(gap / 2.0)
                    .grow(1.0)
                    .min_width(px(0.0)),
                glyph(Glyph::ChevronRight, icon).tone(Tone::Dim),
            ))
            .gap(section)
            .grow(1.0)
            .align(AlignItems::Center)
            .styled(ButtonStyle::Row)
            .on_activate(move |world| {
                world.show(Shown::Plugin(id.clone()));
            }),
        )
    });
    divided_row(content, divided)
}

/// The Plugins panel's methods on [`World`].
trait ListWorldExt {
    /// Shows `page` in the Plugins panel, with no UNINSTALL waiting
    /// to be confirmed.
    fn show(&mut self, page: Shown);

    /// Sets the memory of the plugin `id` to `bytes`, or back for
    /// `None`, saying on its page why when it cannot be kept.
    fn choose(
        &mut self,
        plugin: Entity,
        id: &PluginId,
        bytes: Option<u64>,
    );

    /// Shows `why` under the memory of `plugin`, or nothing for
    /// `None`.
    fn refuse(&mut self, plugin: Entity, why: Option<String>);

    /// Asks to confirm uninstalling `plugin`, or asks no more for
    /// `None`.
    fn confirm_uninstall(&mut self, plugin: Option<Entity>);
}

impl ListWorldExt for World {
    fn show(&mut self, page: Shown) {
        self.confirm_uninstall(None);
        let mut shown = self.resource_mut::<Shown>();
        if *shown != page {
            *shown = page;
        }
    }

    fn choose(
        &mut self,
        plugin: Entity,
        id: &PluginId,
        bytes: Option<u64>,
    ) {
        let kept =
            self.resource_mut::<ChosenLimits>().set_memory(id, bytes);
        if let Err(why) = &kept {
            self.write_message(LogLine::error(
                "plugins",
                why.clone(),
            ));
        }
        self.refuse(plugin, kept.err());
    }

    fn refuse(&mut self, plugin: Entity, why: Option<String>) {
        let refused = why.map(|why| (plugin, why));
        let mut rows = self.resource_mut::<Rows>();
        if rows.refused != refused {
            rows.refused = refused;
        }
    }

    fn confirm_uninstall(&mut self, plugin: Option<Entity>) {
        let mut rows = self.resource_mut::<Rows>();
        if rows.uninstalling != plugin {
            rows.uninstalling = plugin;
        }
    }
}

/// Gathers the rows again when a plugin comes, goes, is switched or
/// is held to other limits, and shows the list again in place of the
/// page of a plugin that went.
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
    things: Query<Things>,
    chosen: Res<ChosenLimits>,
    mut rows: ResMut<Rows>,
    mut shown: ResMut<Shown>,
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
            added: adds.map_or_else(Vec::new, |adds| {
                added(adds.iter(), &things)
            }),
            stopped: stopped.map(|stopped| stopped.0.clone()),
            kind: info.kind,
            memory: budgets.map(|budgets| {
                memory_line(
                    budgets,
                    &info.limits,
                    chosen.get(&info.id),
                )
            }),
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| a.name.cmp(&b.name));
    if let Shown::Plugin(id) = &*shown
        && !now.iter().any(|row| row.id == *id)
    {
        *shown = Shown::List;
        rows.uninstalling = None;
    }
    if rows.plugins != now {
        rows.plugins = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_and_the_filter_combine() {
        let row = |id: &str, name: &str, on, kind| Row {
            plugin: Entity::PLACEHOLDER,
            id: PluginId::try_from(id).unwrap(),
            name: name.into(),
            about: String::new(),
            on,
            adds: 0,
            added: Vec::new(),
            stopped: None,
            kind,
            memory: None,
        };
        let mut rows = Rows {
            plugins: vec![
                row("hamlet.chat", "Chat", true, PluginKind::Bundled),
                row(
                    "hamlet.realm",
                    "Realm",
                    false,
                    PluginKind::Bundled,
                ),
                row(
                    "test.notes",
                    "Notes",
                    true,
                    PluginKind::Installed,
                ),
            ],
            ..Rows::default()
        };
        let shown = |rows: &Rows| {
            rows.shown()
                .map(|row| row.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(&rows), ["Chat", "Realm", "Notes"]);
        rows.search = " HAMLET ".into();
        assert_eq!(shown(&rows), ["Chat", "Realm"]);
        rows.filter = Filter::On;
        assert_eq!(shown(&rows), ["Chat"]);
        rows.search = "not".into();
        rows.filter = Filter::Bundled;
        assert!(shown(&rows).is_empty());
        rows.filter = Filter::Installed;
        assert_eq!(shown(&rows), ["Notes"]);
    }
}
