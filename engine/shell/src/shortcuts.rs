//! The Shortcuts page of Settings: every command by its menu, or by
//! what added it, with its keys, a way to bind it to another chord,
//! to clear it and to put it back.

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::{Mut, World};
use bevy::input::ButtonInput;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::ui::AlignItems;
use bevy::window::Window;
use bevy_fynix::shortcut::{Chord, Mods};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, label, row,
};
use bevy_fynix::{ModifierExt, ViewExt, keyed, resource};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{CommandId, PanelId};

use crate::command::ChordExt;
use crate::icons::{Glyph, glyph};
use crate::keymap::{Bindings, Keymap};
use crate::menu::MenuBar;
use crate::style::{
    ButtonStyle, action, buttons, content_button, description, field,
    list, note, section,
};
use crate::{AnyView, SettingsPage, View, themed};

/// The name of the Shortcuts page.
pub(crate) const SHORTCUTS: &str = "shortcuts";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Capturing>()
        .init_resource::<Search>()
        .init_resource::<Clash>()
        .init_resource::<Rows>()
        .add_systems(Startup, add)
        .add_systems(Update, follow)
        .add_observer(capture);
}

/// The command waiting for the chord it will be bound to.
#[derive(Resource, Default)]
pub(crate) struct Capturing {
    command: Option<CommandId>,
    /// The key that bound it is still down, so it runs nothing.
    held: bool,
}

impl Capturing {
    /// Whether a key press is the page's, and runs no command.
    pub(crate) fn active(&self) -> bool {
        self.command.is_some() || self.held
    }
}

/// What the commands are filtered by.
#[derive(Resource, Clone, PartialEq, Default)]
struct Search(String);

/// A chord pressed for `command` that `other` has, waiting for the
/// person to swap or replace.
#[derive(Resource, Clone, PartialEq, Default)]
struct Clash(Option<Clashing>);

#[derive(Clone, PartialEq)]
struct Clashing {
    command: CommandId,
    chord: Chord,
    other: CommandId,
}

/// The groups as the page lists them.
#[derive(Resource, Clone, PartialEq, Default)]
struct Rows {
    groups: Vec<Group>,
    /// Whether the person bound any command.
    rebound: bool,
}

#[derive(Clone, PartialEq)]
struct Group {
    title: String,
    rows: Vec<Line>,
}

/// A command as its row shows it.
#[derive(Clone, PartialEq)]
struct Line {
    command: CommandId,
    label: String,
    /// Its chords, as the platform writes them.
    keys: String,
    rebound: bool,
    capturing: bool,
    /// The label of another command bound to one of its chords.
    shared: Option<String>,
    /// A chord pressed for it that another has: the chord and the
    /// other's label.
    clash: Option<(String, String)>,
}

fn add(mut commands: Commands) {
    commands.spawn(SettingsPage {
        id: PanelId::new(ENGINE, SHORTCUTS).expect("a valid id"),
        title: "Shortcuts".into(),
        about: "The keys that run each command.".into(),
        icon: Glyph::Bolt,
        content: view,
    });
}

/// The search and Reset All over the groups, each a list of rows.
fn view(_: Entity) -> AnyView {
    let search = field(
        resource::<Search, _>(|search| search.0.clone()),
        "Search commands".into(),
        |world, typed| {
            let mut search = world.resource_mut::<Search>();
            if search.0 != typed {
                search.0 = typed;
            }
        },
    );
    // Only while the person bound any.
    let reset_all =
        keyed(resource::<Rows, _>(|rows| rows.rebound), |&rebound| {
            if !rebound {
                return column(()).boxed();
            }
            action(ButtonStyle::Plain, "RESET ALL")
                .on_activate(|world| {
                    world.resource_mut::<Clash>().0 = None;
                    world.resource_mut::<Keymap>().reset_all();
                })
                .boxed()
        })
        .within(row(()).gap(0.0));
    let groups = keyed(resource::<Rows, _>(Clone::clone), |rows| {
        if rows.groups.is_empty() {
            return note("No command matches.").boxed();
        }
        let groups = rows.groups.clone();
        themed(move |cx| {
            let gap = cx.theme().space.gap;
            let parts = groups
                .iter()
                .map(|group| {
                    column((
                        section(group.title.to_uppercase()),
                        list(
                            group
                                .rows
                                .iter()
                                .cloned()
                                .map(|one| line(one).boxed())
                                .collect(),
                        ),
                    ))
                    .gap(gap)
                    .boxed()
                })
                .collect::<Vec<_>>();
            column(parts).gap(cx.theme().space.panel)
        })
        .boxed()
    })
    .within(column(()).gap(0.0));
    themed(move |cx| {
        let theme = cx.theme();
        let (gap, panel) = (theme.space.gap, theme.space.panel);
        column((
            description(
                "Press a shortcut, then the keys to run its command \
                 with. Escape leaves it as it was.",
            ),
            row((column((search,)).gap(0.0).grow(1.0), reset_all))
                .gap(gap)
                .align(AlignItems::Center),
            groups,
        ))
        .gap(panel)
    })
    .boxed()
}

/// A command's row: its label, its keys to press to bind it, and
/// buttons to clear it and to put it back. Under it, a chord another
/// command has, with the ways out.
fn line(line: Line) -> impl View {
    let command = line.command.clone();
    let (keys, tone) = if line.capturing {
        ("Press keys".to_owned(), Tone::Accent)
    } else if line.keys.is_empty() {
        ("None".to_owned(), Tone::Faint)
    } else if line.shared.is_some() {
        (line.keys.clone(), Tone::Critical)
    } else {
        (line.keys.clone(), Tone::Body)
    };
    let bind = {
        let command = command.clone();
        content_button(
            ButtonStyle::Plain,
            label(keys).wrap(false).tone(tone),
        )
        .on_activate(move |world| {
            // Or the next key lands on the button just pressed.
            world.resource_mut::<InputFocus>().clear();
            world.resource_mut::<Clash>().0 = None;
            world.resource_mut::<Capturing>().command =
                Some(command.clone());
        })
    };
    let mut items = vec![
        label(line.label.clone()).grown(1.0).boxed(),
        bind.boxed(),
    ];
    if !line.keys.is_empty() {
        let command = command.clone();
        items.push(
            icon(Glyph::Cross, "Clear", move |world| {
                world.resource_mut::<Clash>().0 = None;
                world.resource_scope(
                    |world, mut keymap: Mut<Keymap>| {
                        keymap.bind(
                            world.resource::<Bindings>(),
                            &command,
                            Vec::new(),
                        );
                    },
                );
            })
            .boxed(),
        );
    }
    if line.rebound {
        let command = command.clone();
        items.push(
            icon(Glyph::Restore, "Reset", move |world| {
                world.resource_mut::<Clash>().0 = None;
                world.resource_mut::<Keymap>().reset(&command);
            })
            .boxed(),
        );
    }
    let shared = line.shared.clone();
    let clash = line.clash.clone();
    themed(move |cx| {
        let gap = cx.theme().space.gap;
        let mut parts = vec![
            row(items).gap(gap).align(AlignItems::Center).boxed(),
        ];
        if let Some(other) = &shared {
            parts.push(
                warning(format!("Also bound to {other}.")).boxed(),
            );
        }
        if let Some((chord, other)) = &clash {
            parts.push(
                warning(format!("{chord} already runs {other}."))
                    .boxed(),
            );
            parts.push(choices(other).boxed());
        }
        column(parts).gap(gap / 2.0)
    })
}

/// A quiet button showing only `which`, saying `tip` under the
/// pointer and running `press`.
fn icon(
    which: Glyph,
    tip: &'static str,
    press: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    let shown = themed(move |cx| {
        let size = cx.theme().space.icon;
        glyph(which, size).tone(Tone::Dim)
    });
    content_button(ButtonStyle::Quiet, shown)
        .on_activate(press)
        .tooltip(move || note(tip))
}

fn warning(text: String) -> impl View {
    themed(move |cx| {
        let size = cx.theme().text.small;
        label(text).size(size).tone(Tone::Critical)
    })
}

/// Swap, Replace and Cancel, for a chord `other` has.
fn choices(other: &str) -> impl View {
    let settle = |swap: bool| {
        move |world: &mut World| {
            let Some(clashing) =
                world.resource_mut::<Clash>().0.take()
            else {
                return;
            };
            world.resource_scope(|world, mut keymap: Mut<Keymap>| {
                keymap.take(
                    world.resource::<Bindings>(),
                    &clashing.command,
                    clashing.chord,
                    &clashing.other,
                    swap,
                );
            });
        }
    };
    let swap_tip = format!("{other} takes this one's keys");
    let replace_tip = format!("{other} loses these keys");
    buttons(vec![
        action(ButtonStyle::Plain, "SWAP")
            .on_activate(settle(true))
            .tooltip(move || note(swap_tip.clone()))
            .boxed(),
        action(ButtonStyle::Plain, "REPLACE")
            .on_activate(settle(false))
            .tooltip(move || note(replace_tip.clone()))
            .boxed(),
        action(ButtonStyle::Quiet, "CANCEL")
            .on_activate(|world| {
                world.resource_mut::<Clash>().0 = None
            })
            .boxed(),
    ])
}

/// Binds the command waiting to the next chord pressed, or asks to
/// swap or replace when another has it. Escape leaves it as it was.
fn capture(
    event: On<FocusedInput<KeyboardInput>>,
    windows: Query<(), With<Window>>,
    focus: Res<InputFocus>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut capturing: ResMut<Capturing>,
    mut commands: Commands,
) {
    let key = &event.input;
    // Seen once, where it starts.
    let first = match focus.get() {
        Some(held) => event.event_target() == held,
        None => windows.contains(event.event_target()),
    };
    if !first {
        return;
    }
    if !key.state.is_pressed() {
        if capturing.held {
            capturing.held = false;
        }
        return;
    }
    let Some(command) = capturing.command.clone() else {
        return;
    };
    if key.repeat {
        return;
    }
    let chord = Chord {
        key: key.key_code,
        mods: keys.as_deref().map_or(Mods::NONE, Mods::held),
    };
    if chord == Chord::key(KeyCode::Escape) {
        *capturing = Capturing {
            command: None,
            held: true,
        };
        return;
    }
    // A modifier on its way down, or a key a file cannot name.
    let modifier = matches!(
        chord.key,
        KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
    );
    if modifier || chord.keys().is_none() {
        return;
    }
    *capturing = Capturing {
        command: None,
        held: true,
    };
    commands.queue(move |world: &mut World| {
        let other = world
            .resource::<Bindings>()
            .holders(chord)
            .find(|holder| **holder != command)
            .cloned();
        match other {
            Some(other) => {
                world.resource_mut::<Clash>().0 = Some(Clashing {
                    command,
                    chord,
                    other,
                });
            }
            None => {
                world.resource_scope(
                    |world, mut keymap: Mut<Keymap>| {
                        keymap.bind(
                            world.resource::<Bindings>(),
                            &command,
                            vec![chord],
                        );
                    },
                );
            }
        }
    });
}

/// Lists the rows again when the keys, the menus or what the page
/// shows change.
fn follow(
    bindings: Res<Bindings>,
    bar: Res<MenuBar>,
    keymap: Res<Keymap>,
    capturing: Res<Capturing>,
    search: Res<Search>,
    clash: Res<Clash>,
    mut rows: ResMut<Rows>,
) {
    let changed = bindings.is_changed()
        || bar.is_changed()
        || keymap.is_changed()
        || capturing.is_changed()
        || search.is_changed()
        || clash.is_changed();
    if !changed {
        return;
    }
    let label_of = |command: &CommandId| {
        bindings.of(command).map_or_else(
            || command.to_string(),
            |bound| bound.label.clone(),
        )
    };
    let wanted = search.0.trim().to_lowercase();
    let mut groups = Vec::<Group>::new();
    let mut placed = Vec::<&CommandId>::new();
    // Each command under its first menu, then the rest by what added
    // it.
    let menus = bar.0.iter().flat_map(|menu| {
        menu.items
            .iter()
            .map(move |item| (menu.title.clone(), &item.command))
    });
    let rest = bindings.all().iter().map(|bound| {
        let (owner, name) = bound
            .command
            .as_str()
            .split_once('/')
            .unwrap_or_default();
        // The engine's own by what its name starts with, as
        // `workspace.new` is a workspace's.
        let title = match name.split_once('.') {
            _ if owner != ENGINE => owner.to_owned(),
            Some((kind, _)) => {
                let mut chars = kind.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            }
            None => "Engine".to_owned(),
        };
        (title, &bound.command)
    });
    for (title, command) in menus.chain(rest) {
        let Some(bound) = bindings.of(command) else {
            continue;
        };
        if placed.contains(&command) {
            continue;
        }
        placed.push(command);
        if !bound.label.to_lowercase().contains(&wanted) {
            continue;
        }
        let shared = bound.chords.iter().find_map(|&chord| {
            bindings
                .holders(chord)
                .find(|holder| **holder != bound.command)
                .map(label_of)
        });
        let clash = clash
            .0
            .as_ref()
            .filter(|clashing| clashing.command == bound.command)
            .map(|clashing| {
                (
                    clashing.chord.to_string(),
                    label_of(&clashing.other),
                )
            });
        let line = Line {
            command: bound.command.clone(),
            label: bound.label.clone(),
            keys: bound
                .chords
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            rebound: keymap.rebound(&bound.command).is_some(),
            capturing: capturing.command.as_ref()
                == Some(&bound.command),
            shared,
            clash,
        };
        match groups.iter_mut().find(|group| group.title == title) {
            Some(group) => group.rows.push(line),
            None => groups.push(Group {
                title,
                rows: vec![line],
            }),
        }
    }
    let now = Rows {
        groups,
        rebound: !keymap.is_empty(),
    };
    if *rows != now {
        *rows = now;
    }
}
