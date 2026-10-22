//! A plugin's page in the Plugins panel: a breadcrumb back to the
//! list, and under it, as it scrolls, the plugin's head with its
//! switch, its memory, what it adds, the sections other parts of the
//! engine add, and UNINSTALL for an installed one.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ModifierExt, ViewExt, keyed, resource};
use hamlet_engine_api::{CommandId, PluginId};
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::style::{
    ButtonStyle, Wraps, action, breadcrumb, buttons, heading,
    icon_button, list, note, scroll_area, section,
};
use hamlet_engine_shell::{
    AnyView, Command, Cx, MenuEntry, Overlay, Panel, PanelSection,
    SettingsFooter, SettingsPage, Shortcut, StatusItem,
    SuggestedWorkspace,
};

use crate::PluginKind;
use crate::list::limits::memory_view;
use crate::list::{
    ListWorldExt, Question, Row, Rows, Shown, Uninstall, and_list,
    bound, plugin_switch,
};

/// A section of every plugin's page, under what it adds, as another
/// part of the engine adds one.
#[derive(Component, Clone, Copy, Debug)]
#[component(immutable)]
pub struct PluginPageSection {
    /// Builds its content, given the plugin's entity.
    pub content: fn(Entity) -> AnyView,
}

/// A thing a plugin adds, of any kind its page lists.
#[derive(QueryData)]
pub(super) struct Things {
    panel: Option<&'static Panel>,
    page: Option<&'static SettingsPage>,
    footer: Option<&'static SettingsFooter>,
    command: Option<&'static Command>,
    menu_entry: Option<&'static MenuEntry>,
    shortcut: Option<&'static Shortcut>,
    status_item: Option<&'static StatusItem>,
    overlay: Option<&'static Overlay>,
    section: Option<&'static PanelSection>,
    workspace: Option<&'static SuggestedWorkspace>,
}

/// One thing a plugin adds, as its page lists it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) struct Added {
    kind: Kind,
    name: String,
}

/// What kind of thing a plugin adds, in the order its page lists
/// them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Panel,
    SettingsPage,
    SettingsFooter,
    Command,
    MenuEntry,
    Shortcut,
    StatusItem,
    Overlay,
    PanelSection,
    Workspace,
}

/// A plugin's page as it is shown.
#[derive(Clone, PartialEq)]
struct Page {
    row: Row,
    /// Why a value typed for its memory was refused.
    refused: Option<String>,
    /// Its UNINSTALL was pressed, and waits to be confirmed.
    uninstalling: bool,
    /// Its switch was pressed, and waits to be confirmed.
    asked: Option<Question>,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Panel => "Panel",
            Self::SettingsPage => "Settings page",
            Self::SettingsFooter => "Settings footer",
            Self::Command => "Command",
            Self::MenuEntry => "Menu entry",
            Self::Shortcut => "Shortcut",
            Self::StatusItem => "Status item",
            Self::Overlay => "Overlay",
            Self::PanelSection => "Section",
            Self::Workspace => "Workspace",
        }
    }
}

impl Added {
    /// Its name, with what kind of thing it is beside it.
    fn view(&self) -> AnyView {
        let (kind, name) = (self.kind.name(), self.name.clone());
        AnyView::new(move |cx: &mut Cx| {
            let theme = cx.theme();
            let (gap, size) = (theme.space.gap, theme.text.small);
            cx.build(
                row((
                    label(name).grown(1.0),
                    label(kind)
                        .size(size)
                        .tone(Tone::Dim)
                        .wrap(false),
                ))
                .gap(gap)
                .align(AlignItems::Center),
            )
        })
    }
}

impl Page {
    /// The page of the plugin `id` in `rows`.
    fn of(rows: &Rows, id: &PluginId) -> Option<Self> {
        let row = rows.find(id)?.clone();
        Some(Self {
            refused: rows
                .refused
                .as_ref()
                .filter(|(at, _)| *at == row.plugin)
                .map(|(_, why)| why.clone()),
            uninstalling: rows.uninstalling == Some(row.plugin),
            asked: rows.asked(row.plugin),
            row,
        })
    }
}

/// What the things in `of` are, by kind and then by name.
pub(super) fn added(
    of: impl Iterator<Item = Entity>,
    things: &Query<Things>,
) -> Vec<Added> {
    let found = of
        .filter_map(|thing| things.get(thing).ok())
        .collect::<Vec<_>>();
    let label_of = |id: &CommandId| {
        found
            .iter()
            .filter_map(|thing| thing.command)
            .find(|command| command.id == *id)
            .map_or_else(
                || id.to_string(),
                |command| command.label.clone(),
            )
    };
    let mut all = Vec::new();
    for thing in &found {
        let named = [
            thing.panel.map(|it| (Kind::Panel, it.title.clone())),
            thing
                .page
                .map(|it| (Kind::SettingsPage, it.title.clone())),
            thing.footer.map(|it| {
                (Kind::SettingsFooter, it.id.name().to_owned())
            }),
            thing.command.map(|it| (Kind::Command, it.label.clone())),
            thing.menu_entry.map(|it| {
                let label = label_of(&it.command);
                (Kind::MenuEntry, format!("{} > {label}", it.menu))
            }),
            thing.shortcut.map(|it| {
                let label = label_of(&it.command);
                (Kind::Shortcut, format!("{}: {label}", it.chord))
            }),
            thing.status_item.map(|it| {
                (Kind::StatusItem, it.id.name().to_owned())
            }),
            thing
                .overlay
                .map(|it| (Kind::Overlay, it.id.name().to_owned())),
            thing.section.map(|it| {
                (
                    Kind::PanelSection,
                    format!("{} in {}", it.title, it.panel),
                )
            }),
            thing
                .workspace
                .map(|it| (Kind::Workspace, it.name.clone())),
        ];
        all.extend(
            named
                .into_iter()
                .flatten()
                .map(|(kind, name)| Added { kind, name }),
        );
    }
    all.sort();
    all
}

/// The page of the plugin `id`: the breadcrumb, and under it what
/// scrolls.
pub(super) fn page_view(id: PluginId) -> AnyView {
    let named = id.clone();
    let trail = breadcrumb(
        vec!["PLUGINS".into()],
        resource::<Rows, _>(move |rows| {
            rows.find(&named)
                .map(|row| row.name.to_uppercase())
                .unwrap_or_default()
        }),
        |world, _| world.show(Shown::List),
    );
    let found = id.clone();
    // The head, with its switch, is built once a plugin; the rest
    // again as the plugin changes.
    let content = keyed(
        resource::<Rows, _>(move |rows| {
            rows.find(&found).map(|row| (row.plugin, row.kind))
        }),
        move |found| {
            let Some((plugin, kind)) = *found else {
                return note("No such plugin.");
            };
            let head = head(plugin, id.clone(), kind);
            let id = id.clone();
            let rest = keyed(
                resource::<Rows, _>(move |rows| Page::of(rows, &id)),
                |page| match page {
                    Some(page) => body(page),
                    None => note("No such plugin."),
                },
            )
            .within(column(()).gap(0.0));
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.panel;
                cx.build(column((head, rest)).gap(gap))
            })
        },
    )
    .within(column(()).gap(0.0));
    AnyView::new(move |cx: &mut Cx| {
        let section = cx.theme().space.section;
        cx.build(
            column((trail, scroll_area((content,))))
                .gap(section)
                .grow(1.0)
                .min_height(px(0.0)),
        )
    })
}

/// The memory, what it adds, the other sections and UNINSTALL, a part
/// of a panel apart.
fn body(page: &Page) -> AnyView {
    let row_of = &page.row;
    let plugin = row_of.plugin;
    let memory = row_of.memory.map(|line| {
        memory_view(
            plugin,
            &row_of.id,
            line,
            page.refused.clone(),
            row_of.on,
        )
    });
    let asked =
        page.asked.map(|question| confirmation(row_of, question));
    let adds = adds(row_of);
    let needs = needs(row_of);
    let needed_by = needed_by(row_of);
    let uninstall = (row_of.kind == PluginKind::Installed)
        .then(|| uninstall(row_of, page.uninstalling));
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.panel;
        let mut parts = asked.into_iter().collect::<Vec<_>>();
        parts.extend(memory);
        parts.extend([adds, needs, needed_by]);
        parts.extend(sections(cx.world, plugin));
        parts.extend(uninstall);
        cx.build(column(parts).gap(gap))
    })
}

/// The name of `plugin`, of the id `id` and the kind `kind`, over its
/// id and version, where it comes from and its state, with its switch
/// beside them. It follows the plugin in place.
fn head(plugin: Entity, id: PluginId, kind: PluginKind) -> AnyView {
    let switch = plugin_switch(plugin, id);
    let from = match kind {
        PluginKind::BuiltIn => "Built into the engine",
        PluginKind::Bundled => "Comes with Hamlet",
        PluginKind::Installed => "Installed",
    };
    let from_tone = bound(plugin, move |row| {
        if kind == PluginKind::Bundled && row.on {
            Tone::Accent
        } else {
            Tone::Dim
        }
    });
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, size, section) =
            (theme.space.gap, theme.text.small, theme.space.section);
        let facts = row((
            label(bound(plugin, |row| row.about.clone()))
                .size(size)
                .tone(Tone::Dim)
                .wrap(false),
            label(from).size(size).tone(from_tone).wrap(false),
            label(bound(plugin, |row| row.state().0))
                .size(size)
                .tone(bound(plugin, |row| row.state().1)),
        ))
        .gap(gap)
        .tagged(Wraps);
        let name = heading(bound(plugin, |row| row.name.clone()));
        cx.build(
            row((
                column((name, facts))
                    .gap(gap / 2.0)
                    .grow(1.0)
                    .min_width(px(0.0)),
                switch,
            ))
            .gap(section)
            .align(AlignItems::Start),
        )
    })
}

/// WHAT IT ADDS over a list of it, which is empty while it is off.
fn adds(row_of: &Row) -> AnyView {
    let listed = if !row_of.on {
        note("Nothing while it is off.")
    } else if row_of.added.is_empty() {
        note("Nothing.")
    } else {
        list(row_of.added.iter().map(Added::view).collect())
    };
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap / 2.0;
        cx.build(column((section("WHAT IT ADDS"), listed)).gap(gap))
    })
}

/// NEEDS over each plugin it needs with the versions that fit, and
/// whether it is there.
fn needs(row_of: &Row) -> AnyView {
    let listed = if row_of.needs.is_empty() {
        note("Nothing.")
    } else {
        list(
            row_of
                .needs
                .iter()
                .map(|(needed, unmet)| {
                    let (state, tone) = match unmet {
                        Some(unmet) => (unmet.clone(), Tone::Accent),
                        None => ("Met".into(), Tone::Dim),
                    };
                    fact_row(needed.clone(), state, tone)
                })
                .collect(),
        )
    };
    titled("NEEDS", listed)
}

/// NEEDED BY over the plugins that need it.
fn needed_by(row_of: &Row) -> AnyView {
    let listed = if row_of.needed_by.is_empty() {
        note("Nothing.")
    } else {
        list(
            row_of
                .needed_by
                .iter()
                .map(|name| label(name.clone()).boxed())
                .collect(),
        )
    };
    titled("NEEDED BY", listed)
}

/// `name` with `state` at its end in `tone`.
fn fact_row(name: String, state: String, tone: Tone) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, size) = (theme.space.gap, theme.text.small);
        cx.build(
            row((
                label(name).grown(1.0),
                label(state).size(size).tone(tone).wrap(false),
            ))
            .gap(gap)
            .align(AlignItems::Center),
        )
    })
}

/// `title` as a section's head over `content`.
fn titled(title: &'static str, content: AnyView) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap / 2.0;
        cx.build(column((section(title), content)).gap(gap))
    })
}

/// What a switch of the plugin of `row_of` moves besides it, with
/// the buttons that go on or not.
pub(super) fn confirmation(
    row_of: &Row,
    question: Question,
) -> AnyView {
    let (plugin, id) = (row_of.plugin, row_of.id.clone());
    let (said, mut sure) = match question {
        Question::Off => (
            format!(
                "Switching off {} also stops {}.",
                row_of.name,
                and_list(&row_of.stops)
            ),
            vec![
                action(ButtonStyle::Critical, "SWITCH OFF")
                    .on_activate(move |world| {
                        world.switch(plugin, &id, false);
                    })
                    .boxed(),
            ],
        ),
        Question::On => {
            let names = row_of
                .wakes
                .iter()
                .map(|(.., name)| name.clone())
                .collect::<Vec<_>>();
            let (which, them) = if names.len() == 1 {
                ("is", "it")
            } else {
                ("are", "them")
            };
            let wakes = row_of.wakes.clone();
            let alone = id.clone();
            (
                format!(
                    "{} needs {}, which {which} off. Switch {them} \
                     on too?",
                    row_of.name,
                    and_list(&names)
                ),
                vec![
                    action(ButtonStyle::Primary, "SWITCH ON ALL")
                        .on_activate(move |world| {
                            for (at, id, _) in &wakes {
                                world.switch(*at, id, true);
                            }
                            world.switch(plugin, &id, true);
                        })
                        .boxed(),
                    action(ButtonStyle::Plain, "ONLY THIS ONE")
                        .on_activate(move |world| {
                            world.switch(plugin, &alone, true);
                        })
                        .boxed(),
                ],
            )
        }
    };
    sure.push(
        action(ButtonStyle::Quiet, "CANCEL")
            .on_activate(|world| world.confirm_switch(None))
            .boxed(),
    );
    let (said, sure) = (note(said), buttons(sure));
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(column((said, sure)).gap(gap))
    })
}

/// The [`PluginPageSection`]s for `plugin`, in the order they came.
fn sections(world: &World, plugin: Entity) -> Vec<AnyView> {
    let mut found = world
        .try_query::<(Entity, &PluginPageSection)>()
        .map(|mut all| {
            all.iter(world)
                .map(|(at, section)| (at, section.content))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    found.sort_by_key(|(at, _)| *at);
    found
        .into_iter()
        .map(|(_, content)| content(plugin))
        .collect()
}

/// UNINSTALL..., or once it is pressed, what uninstalling takes away
/// with UNINSTALL and CANCEL.
fn uninstall(row_of: &Row, asked: bool) -> AnyView {
    let plugin = row_of.plugin;
    if !asked {
        let style = if row_of.on {
            ButtonStyle::Critical
        } else {
            ButtonStyle::Quiet
        };
        return buttons(vec![
            icon_button(style, Glyph::Bin, "UNINSTALL...")
                .on_activate(move |world| {
                    world.confirm_uninstall(Some(plugin));
                })
                .boxed(),
        ]);
    }
    let mut said = format!(
        "Uninstall {}? Its files go, and what you answered for it.",
        row_of.name
    );
    if !row_of.stops.is_empty() {
        said.push_str(&format!(
            " Uninstalling it also stops {}.",
            and_list(&row_of.stops)
        ));
    }
    let said = note(said);
    let sure = buttons(vec![
        icon_button(ButtonStyle::Critical, Glyph::Bin, "UNINSTALL")
            .on_activate(move |world| {
                world.trigger(Uninstall { entity: plugin });
                world.show(Shown::List);
            })
            .boxed(),
        action(ButtonStyle::Quiet, "CANCEL")
            .on_activate(|world| world.confirm_uninstall(None))
            .boxed(),
    ]);
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(column((said, sure)).gap(gap))
    })
}
