//! The workspace switcher: a tab for each workspace in the bar, a "+"
//! that adds one, and on a tab's right-click, rename and delete.

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::Rect;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::shortcut::{Chord, Mods};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, column, label,
    menu_item, popup, row,
};
use bevy_fynix::{ViewExt, keyed, resource};

use crate::command::{CTRL, RunCommand, ShellCommandsExt, engine_id};
use crate::fonts::DisplayText;
use crate::icons::{Glyph, glyph};
use crate::style::{ButtonStyle, StyleExt, field, section};
use crate::workspace::Workspaces;
use crate::{AnyView, Cx, ShellWorldExt};

/// The menu the workspace commands are in.
const MENU: &str = "Workspace";

/// The keys that show the first nine workspaces, with the primary
/// key.
const DIGITS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Tabs>()
        .init_resource::<Renaming>()
        .add_systems(Startup, add_commands)
        .add_systems(
            Update,
            (
                follow.run_if(resource_changed::<Workspaces>),
                focus_field,
            ),
        );
}

/// What the switcher shows: the workspaces' names and the one shown.
#[derive(Resource, Clone, PartialEq, Default)]
struct Tabs {
    names: Vec<String>,
    shown: usize,
}

/// The workspace being renamed, where its tab is, and its name.
#[derive(Resource, Clone, PartialEq, Default)]
pub(crate) struct Renaming(pub(crate) Option<(usize, Rect, String)>);

/// On a workspace's tab: the workspace's index.
#[derive(Component, Clone, Copy)]
pub(crate) struct WorkspaceTab(pub(crate) usize);

/// On the field a workspace is renamed in.
#[derive(Component, Clone, Copy)]
struct RenameField;

/// The switcher, for the bar.
pub fn view() -> AnyView {
    keyed(resource::<Tabs, _>(Clone::clone), |tabs| {
        let many = tabs.names.len() > 1;
        let mut views = tabs
            .names
            .iter()
            .enumerate()
            .map(|(at, name)| tab(at, name, at == tabs.shown, many))
            .collect::<Vec<_>>();
        AnyView::new(move |cx: &mut Cx| {
            let space = &cx.theme().space;
            let (gap, side) = (space.gap, space.icon);
            views.push(
                glyph(Glyph::Plus, side)
                    .styled(ButtonStyle::Quiet)
                    .on_activate(|world| {
                        world.resource_mut::<Workspaces>().add();
                    })
                    .boxed(),
            );
            cx.build(
                row(views)
                    .gap(gap)
                    .height(percent(100.0))
                    .align(AlignItems::Stretch),
            )
        })
    })
    .boxed()
}

/// The field a workspace is renamed in, over everything, while one
/// is.
pub fn rename_view() -> AnyView {
    keyed(
        resource::<Renaming, _>(|renaming| renaming.0.clone()),
        |at| {
            let Some((at, anchor, name)) = at.clone() else {
                return column(()).boxed();
            };
            let field =
                field(name, String::new(), move |world, name| {
                    world
                        .resource_mut::<Workspaces>()
                        .rename(at, &name);
                    world.resource_mut::<Renaming>().0 = None;
                })
                .tagged(RenameField);
            AnyView::new(move |cx: &mut Cx| {
                let space = &cx.theme().space;
                let (gap, width) = (space.gap, space.menu_width);
                cx.build(
                    popup(
                        anchor,
                        (column((
                            section("RENAME WORKSPACE"),
                            field,
                        ))
                        .gap(gap)
                        .min_width(px(width))
                        .padding(UiRect::all(px(gap))),),
                    )
                    .on_dismiss(|world, _| {
                        world.resource_mut::<Renaming>().0 = None
                    }),
                )
            })
        },
    )
    .boxed()
}

fn tab(at: usize, name: &str, shown: bool, many: bool) -> AnyView {
    label(name.to_uppercase())
        .wrap(false)
        .tagged(DisplayText)
        .in_tab(shown)
        .tagged(WorkspaceTab(at))
        .on_activate(move |world| {
            world.resource_mut::<Workspaces>().show(at);
        })
        .context_menu(move || {
            let mut items = vec![
                menu_item(label("Rename"))
                    .on_activate(move |world| world.start_rename(at))
                    .boxed(),
            ];
            if many {
                items.push(
                    menu_item(label("Delete").tone(Tone::Critical))
                        .on_activate(move |world| {
                            world
                                .resource_mut::<Workspaces>()
                                .delete(at);
                        })
                        .boxed(),
                );
            }
            items
        })
        .boxed()
}

fn follow(workspaces: Res<Workspaces>, mut tabs: ResMut<Tabs>) {
    let now = Tabs {
        names: workspaces
            .list()
            .iter()
            .map(|workspace| workspace.name.clone())
            .collect(),
        shown: workspaces.shown(),
    };
    if *tabs != now {
        *tabs = now;
    }
}

/// Puts the keyboard in a new rename field.
fn focus_field(
    fields: Query<&Children, Added<RenameField>>,
    texts: Query<(), With<EditableText>>,
    mut focus: ResMut<InputFocus>,
) {
    for children in &fields {
        if let Some(text) =
            children.iter().copied().find(|&kid| texts.contains(kid))
        {
            focus.set(text, FocusCause::Navigated);
        }
    }
}

/// The workspace commands: new, rename, delete, next and previous in
/// the menu, Ctrl+Tab and Ctrl+Shift+Tab to go round, and the
/// primary key with 1 to 9 to pick one.
fn add_commands(mut commands: Commands) {
    let key = |key, mods| Chord { key, mods };
    commands
        .spawn_command(
            engine_id("workspace.new"),
            "New workspace",
            Some(MENU),
            &[],
        )
        .observe(
            |_: On<RunCommand>,
             mut workspaces: ResMut<Workspaces>| {
                workspaces.add();
            },
        );
    commands
        .spawn_command(
            engine_id("workspace.rename"),
            "Rename workspace",
            Some(MENU),
            &[],
        )
        .observe(|_: On<RunCommand>, mut commands: Commands| {
            commands.queue(|world: &mut World| {
                let shown = world.resource::<Workspaces>().shown();
                world.start_rename(shown);
            });
        });
    commands
        .spawn_command(
            engine_id("workspace.delete"),
            "Delete workspace",
            Some(MENU),
            &[],
        )
        .observe(
            |_: On<RunCommand>,
             mut workspaces: ResMut<Workspaces>| {
                let shown = workspaces.shown();
                workspaces.delete(shown);
            },
        );
    commands
        .spawn_command(
            engine_id("workspace.next"),
            "Next workspace",
            Some(MENU),
            &[key(KeyCode::Tab, CTRL)],
        )
        .observe(
            |_: On<RunCommand>,
             mut workspaces: ResMut<Workspaces>| {
                workspaces.cycle(false);
            },
        );
    commands
        .spawn_command(
            engine_id("workspace.previous"),
            "Previous workspace",
            Some(MENU),
            &[key(KeyCode::Tab, CTRL.with(Mods::SHIFT))],
        )
        .observe(
            |_: On<RunCommand>,
             mut workspaces: ResMut<Workspaces>| {
                workspaces.cycle(true);
            },
        );
    for (at, digit) in DIGITS.into_iter().enumerate() {
        commands.spawn_command(
            engine_id(&format!("workspace.show_{}", at + 1)),
            &format!("Workspace {}", at + 1),
            None,
            &[key(digit, Mods::PRIMARY)],
        )
        .observe(
            move |_: On<RunCommand>,
                  mut workspaces: ResMut<Workspaces>| {
                workspaces.show(at);
            },
        );
    }
}
