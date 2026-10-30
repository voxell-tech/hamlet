//! The terminals beside the screen: by hamlet, each group folded
//! away or open, each terminal with its name, what it is, the last
//! command it ran, and its close. A double click names one.

use std::collections::BTreeSet;

use bevy::app::{App, PostUpdate, PreUpdate};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::events::PointerClick;
use bevy::picking::pointer::PointerButton;
use bevy::text::{EditableText, TextEdit};
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm;
use hamlet_client::terminals::{self, Terminal, TerminalKind};
use hamlet_ui::styles::{
    Fills, ICON, caption, close_button, hairline, link_row,
    scroll_area, text_input,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, View, with_cx};

use super::{Active, Adding, Handing, activate};
use crate::PanelSystems;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Listed>()
        .init_resource::<Folded>()
        .init_resource::<Renaming>()
        .add_observer(start_rename)
        .add_systems(PreUpdate, list.in_set(PanelSystems::Bind))
        // After the views mount and after the screen takes the keys
        // a click gave it, so the new field keeps them.
        .add_systems(PostUpdate, (focus_field, end_rename).chain());
}

/// The terminals as the list shows them.
#[derive(Resource, Default, Clone, PartialEq)]
pub(super) struct Listed(pub(super) Vec<Group>);

/// The groups folded away, by [`Group::key`].
#[derive(Resource, Default, Clone, PartialEq)]
struct Folded(BTreeSet<String>);

/// The terminal whose name is being typed, and whether its field has
/// had the keys yet.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
struct Renaming(Option<(Entity, bool)>);

/// A terminal's row in the list, which a double click names.
#[derive(Component)]
struct Entry(Entity);

/// The field a terminal's name is typed in.
#[derive(Component)]
struct RenameField;

/// The terminals of one hamlet, or the unbounded ones.
#[derive(Clone, PartialEq)]
pub(super) struct Group {
    /// The hamlet's id, or empty for the unbounded ones.
    key: String,
    pub(super) title: String,
    pub(super) items: Vec<Item>,
}

#[derive(Clone, PartialEq)]
pub(super) struct Item {
    pub(super) terminal: Entity,
    pub(super) name: String,
    /// The agent's name, or "chief".
    pub(super) owner: String,
    pub(super) sandboxed: bool,
    pub(super) exit: Option<i64>,
    pub(super) runs: bool,
    pub(super) active: bool,
    /// The last command it ran.
    pub(super) last: Option<String>,
}

impl Listed {
    pub(super) fn find(&self, terminal: Entity) -> Option<&Item> {
        self.items().find(|item| item.terminal == terminal)
    }

    pub(super) fn first(&self) -> Option<Entity> {
        self.items().map(|item| item.terminal).next()
    }

    pub(super) fn items(&self) -> impl Iterator<Item = &Item> {
        self.0.iter().flat_map(|group| &group.items)
    }
}

/// Lists the terminals by hamlet, the unbounded ones last. In a
/// hamlet the agents' come first, by agent and each agent's runs
/// before its shells, then the chief's.
pub(super) fn list(world: &mut World) {
    let active = world.resource::<Active>().0;
    let Some(views) =
        world.try_query::<(Entity, &Terminal)>().map(|mut query| {
            query
                .iter(world)
                .map(|(entity, view)| (entity, view.0.clone()))
                .collect::<Vec<_>>()
        })
    else {
        return;
    };
    let mut keyed = views
        .into_iter()
        .map(|(entity, view)| {
            let hamlet = view.hamlet.clone();
            let title = hamlet.as_deref().map_or_else(
                || "Unbounded".to_owned(),
                |id| {
                    realm::hamlet(world, id)
                        .map(|h| realm::name_of(world, h))
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| id.to_owned())
                },
            );
            let owner = view.agent.as_ref().map(|agent| {
                realm::agent(world, agent)
                    .map(|a| realm::name_of(world, a))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| {
                        format!("agent {}", agent.number)
                    })
            });
            let runs = view.kind == TerminalKind::Runs;
            // A name the chief gave shows over what programs set.
            let name = if view.named || view.title.is_empty() {
                view.name.clone()
            } else {
                view.title.clone()
            };
            let order = (
                hamlet.is_none(),
                title.clone(),
                owner.is_none(),
                owner.clone(),
                !runs,
                view.name.clone(),
                view.id.clone(),
            );
            let item = Item {
                terminal: entity,
                name,
                owner: owner.unwrap_or_else(|| "chief".into()),
                sandboxed: view.sandboxed,
                exit: view.exit,
                runs,
                active: active == Some(entity),
                last: view.last.clone(),
            };
            (order, (hamlet.unwrap_or_default(), title), item)
        })
        .collect::<Vec<_>>();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut groups = Vec::<Group>::new();
    for (_, (key, title), item) in keyed {
        match groups.last_mut() {
            Some(group) if group.key == key => group.items.push(item),
            _ => groups.push(Group {
                key,
                title,
                items: vec![item],
            }),
        }
    }
    let listed = Listed(groups);
    if *world.resource::<Listed>() != listed {
        world.insert_resource(listed);
    }
}

/// The list of terminals and the press that opens another.
pub(super) fn side() -> impl View {
    with_cx(|cx| {
        let space = &cx.theme().space;
        column((
            row((
                row((caption("TERMINALS".to_string(), Tone::Faint),))
                    .grow(1.0),
                button(glyph(Glyph::Plus, ICON).tone(Tone::Dim))
                    .padding(UiRect::all(px(4.0)))
                    .rules(link_row)
                    .on_activate_with(|world, node| {
                        let anchor =
                            crate::menu::rect_of(world, node);
                        world.resource_mut::<Adding>().0 = anchor;
                    }),
            ))
            .align(AlignItems::Center)
            .justify(JustifyContent::SpaceBetween)
            .padding(UiRect::new(
                px(space.section),
                px(space.gap),
                px(6.0),
                px(6.0),
            )),
            hairline(cx.theme().palette.border),
            scroll_area((keyed(
                derived(|world: &World| {
                    (
                        world.resource::<Listed>().clone(),
                        world.resource::<Folded>().clone(),
                        world
                            .resource::<Renaming>()
                            .0
                            .map(|(t, _)| t),
                    )
                })
                .when(every_frame()),
                |(listed, folded, renaming)| {
                    groups(listed, folded, *renaming).boxed()
                },
            )
            .within(column(()).gap(0.0)),)),
        ))
        .gap(0.0)
        .width(px(cx.theme().terminal.list))
        .shrink(0.0)
        .fill(cx.theme().palette.card)
    })
}

fn groups(
    listed: &Listed,
    folded: &Folded,
    renaming: Option<Entity>,
) -> impl View {
    let groups = listed
        .0
        .iter()
        .map(|group| {
            let shut = folded.0.contains(&group.key);
            let rows = if shut {
                Vec::new()
            } else {
                group
                    .items
                    .iter()
                    .map(|item| {
                        entry(
                            item.clone(),
                            renaming == Some(item.terminal),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            column((heading(group, shut), column(rows).gap(0.0)))
                .gap(0.0)
                .boxed()
        })
        .collect::<Vec<_>>();
    column(groups).gap(0.0).padding(UiRect::bottom(px(8.0)))
}

/// A group's title, which folds the group away or opens it, with how
/// many it holds while folded.
fn heading(group: &Group, shut: bool) -> impl View {
    let key = group.key.clone();
    let title = group.title.to_uppercase();
    let count = shut.then(|| group.items.len().to_string());
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let mark = if shut { Glyph::Into } else { Glyph::Down };
        let mut words = vec![
            glyph(mark, ICON).tone(Tone::Faint).boxed(),
            caption(title.clone(), Tone::Dim).boxed(),
        ];
        if let Some(count) = &count {
            words.push(caption(count.clone(), Tone::Faint).boxed());
        }
        let key = key.clone();
        button(row(words).gap(4.0).align(AlignItems::Center))
            .justify(JustifyContent::FlexStart)
            .padding(UiRect::new(
                px(space.section - 4.0),
                px(space.section),
                px(space.gap),
                px(2.0),
            ))
            .rules(link_row)
            .on_activate(move |world| fold(world, &key))
    })
}

/// Folds the group of `key` away, or opens it.
fn fold(world: &mut World, key: &str) {
    let mut folded = world.resource_mut::<Folded>();
    if !folded.0.remove(key) {
        folded.0.insert(key.to_owned());
    }
}

/// One terminal: its name and owner, what it is, the last command it
/// ran, and its close. While it is named, a field holds its name.
fn entry(item: Item, renaming: bool) -> AnyView {
    with_cx(move |cx| {
        let theme = cx.theme();
        let small = theme.text.small;
        let terminal = item.terminal;
        let mut about = vec![item.owner.clone()];
        if item.runs {
            about.push("runs".into());
        }
        if !item.sandboxed {
            about.push("unsandboxed".into());
        }
        if let Some(code) = item.exit {
            about.push(format!("exit {code}"));
        }
        let mark = if item.runs {
            Glyph::Play
        } else {
            Glyph::Terminal
        };
        let tone = if item.active { Tone::Accent } else { Tone::Dim };
        let fill = if item.active {
            theme.palette.button
        } else {
            Color::NONE
        };
        let name = if renaming {
            rename_field(terminal, item.name.clone())
        } else {
            label(item.name.clone())
                .size(small)
                .bold(item.active)
                .tone(if item.active {
                    Tone::Body
                } else {
                    Tone::Dim
                })
                .wrap(false)
                .boxed()
        };
        let mut lines = vec![
            name,
            label(about.join(" \u{b7} "))
                .size(theme.text.caption)
                .tone(if item.exit.is_some_and(|code| code != 0) {
                    Tone::Critical
                } else {
                    Tone::Faint
                })
                .wrap(false)
                .boxed(),
        ];
        if let Some(last) = &item.last {
            // What the row leaves for the line: the list less its
            // paddings, the mark, the close and the gaps between.
            let room = theme.terminal.list
                - theme.space.section
                - 4.0
                - ICON
                - theme.space.gap * 2.0
                - (ICON + 8.0);
            let most = (room / (theme.text.caption * MONO_ADVANCE))
                .floor()
                .max(4.0) as usize;
            lines.push(
                label(elided(&format!("$ {last}"), most))
                    .size(theme.text.caption)
                    .tone(Tone::Faint)
                    .wrap(false)
                    .tagged(MonoText)
                    .boxed(),
            );
        }
        row((
            button(
                row((
                    glyph(mark, ICON).tone(tone),
                    column(lines)
                        .gap(1.0)
                        .min_width(px(0.0))
                        .grow(1.0),
                ))
                .gap(theme.space.gap)
                .align(AlignItems::Center),
            )
            .grow(1.0)
            .min_width(px(0.0))
            .justify(JustifyContent::FlexStart)
            .padding(UiRect::new(
                px(theme.space.section),
                px(4.0),
                px(5.0),
                px(5.0),
            ))
            .rules(link_row)
            .on_activate(move |world| activate(world, terminal))
            .tagged(Entry(terminal)),
            close_button(move |world| close(world, terminal)),
        ))
        .align(AlignItems::Center)
        .padding(UiRect::right(px(theme.space.gap)))
        .fill(fill)
    })
    .boxed()
}

/// The width of a monospace letter, in ems.
const MONO_ADVANCE: f32 = 0.6;

/// `text` in at most `most` letters, three dots ending what is cut:
/// the monospace face has no ellipsis.
fn elided(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut cut = text
        .chars()
        .take(most.saturating_sub(3))
        .collect::<String>();
    cut.push_str("...");
    cut
}

/// The field `terminal`'s name is typed in. Enter names it; Escape,
/// or the keys going elsewhere, leaves it as it was.
fn rename_field(terminal: Entity, name: String) -> AnyView {
    text_field(name, move |world, name| {
        // Enter keeps the field focused while it commits; focus
        // leaving first drops the name.
        let entered = world
            .resource::<InputFocus>()
            .get()
            .and_then(|input| world.get::<ChildOf>(input))
            .is_some_and(|parent| {
                world.get::<RenameField>(parent.parent()).is_some()
            });
        if !entered {
            return;
        }
        terminals::rename(world, terminal, &name);
        world.resource_mut::<Renaming>().0 = None;
        world.resource_mut::<Handing>().0 = true;
    })
    .fills_width()
    .rules(text_input)
    .tagged(RenameField)
    .boxed()
}

/// Names the terminal of a row clicked twice in quick succession.
fn start_rename(
    click: On<PointerClick>,
    entries: Query<&Entry>,
    mut renaming: ResMut<Renaming>,
) {
    let Ok(entry) = entries.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary && click.count == 2 {
        renaming.0 = Some((entry.0, false));
    }
}

/// Puts the keyboard in a new rename field, its name chosen so what
/// is typed replaces it.
fn focus_field(
    fields: Query<&Children, Added<RenameField>>,
    mut texts: Query<&mut EditableText>,
    mut focus: ResMut<InputFocus>,
    mut renaming: ResMut<Renaming>,
) {
    for children in &fields {
        if let Some(&text) =
            children.iter().find(|&&kid| texts.contains(kid))
        {
            focus.set(text, FocusCause::Navigated);
            if let Ok(mut field) = texts.get_mut(text) {
                field.queue_edit(TextEdit::SelectAll);
            }
            if let Some((terminal, _)) = renaming.0 {
                renaming.0 = Some((terminal, true));
            }
        }
    }
}

/// Leaves the name being typed on Escape, or once the keyboard is
/// elsewhere after the field had it.
fn end_rename(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    fields: Query<Entity, With<RenameField>>,
    parents: Query<&ChildOf>,
    mut renaming: ResMut<Renaming>,
    mut commands: Commands,
) {
    let Some((_, had_keys)) = renaming.0 else {
        return;
    };
    let inside = fields.iter().any(|field| {
        focus.get().is_some_and(|input| {
            input == field
                || parents
                    .iter_ancestors(input)
                    .any(|above| above == field)
        })
    });
    if keys.just_pressed(KeyCode::Escape) {
        renaming.0 = None;
        commands.queue(|world: &mut World| {
            world.resource_mut::<Handing>().0 = true;
        });
    } else if had_keys && !inside {
        renaming.0 = None;
    }
}

/// Closes `terminal`. Closing the one shown gives the keys to the
/// one shown next.
fn close(world: &mut World, terminal: Entity) {
    let others = world.resource::<Listed>().items().count() > 1;
    if others && world.resource::<Active>().0 == Some(terminal) {
        world.resource_mut::<Handing>().0 = true;
    }
    terminals::close(world, terminal);
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client as protocol;
    use serde_json::json;

    use super::*;

    fn view(
        id: &str,
        title: &str,
        named: bool,
    ) -> terminals::TerminalView {
        serde_json::from_value(json!({
            "agent": null, "cols": 80, "rows": 24, "cwd": "/",
            "exit": null, "hamlet": null, "id": id, "kind": "shell",
            "name": id, "named": named, "title": title, "sandboxed": false,
            "last": "cargo test"
        }))
        .unwrap()
    }

    #[test]
    fn a_long_command_is_cut_with_an_ellipsis() {
        assert_eq!(elided("$ ls", 10), "$ ls");
        assert_eq!(elided("$ cargo test --all", 10), "$ cargo...");
        assert_eq!(elided("$ ls", 4), "$ ls");
    }

    #[test]
    fn a_name_given_shows_over_the_title_and_a_group_folds_and_opens()
    {
        let mut app = crate::testing::panel_app();
        let mut state = hamlet_testing::realm::state();
        state.revision = 1;
        state.terminals = vec![
            view("plain", "vim", false),
            view("named", "vim", true),
        ];
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.init_resource::<Active>()
            .init_resource::<Listed>()
            .init_resource::<Folded>();
        list(app.world_mut());
        let listed = app.world().resource::<Listed>().clone();
        let names = listed
            .items()
            .map(|item| (item.name.as_str(), item.last.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                ("named", Some("cargo test")),
                ("vim", Some("cargo test"))
            ]
        );
        fold(app.world_mut(), "");
        assert!(app.world().resource::<Folded>().0.contains(""));
        fold(app.world_mut(), "");
        assert!(app.world().resource::<Folded>().0.is_empty());
    }
}
