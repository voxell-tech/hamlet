//! The sheet of controls held in each of their states, and the sheet
//! of icons.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, Overflow, Pressed, UiRect, percent, px};
use bevy::ui_widgets::Activate;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, row,
    text_field,
};
use bevy_fynix::{Bevy, Focused, ScopedExt, View, ViewExt};
use hamlet_app::{chat, lists, menu};
use hamlet_realm::sound::SoundSettings;
use hamlet_ui::Cx;
use hamlet_ui::styles::{
    self, back_link, danger_button, marked, primary_button,
    text_input,
};
use hamlet_ui::theme::{HamletTheme, StatusKind};
use hamlet_ui::widgets::effects::{Highlight, Underglow, Variant};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::{self, DisplayText};
use hamlet_ui::widgets::tabs;

use super::{Scene, Strike, Ui};

/// The update on which the chat is laid out and still.
const SETTLED: u32 = 120;

/// A control held in one state.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Held {
    Hover,
    Press,
    Done,
    Focus,
}

fn caption(text: impl Into<String>) -> Ui {
    label(text.into())
        .size(11.0)
        .tone(Tone::Dim)
        .wrap(false)
        .boxed()
}

fn heading(text: &'static str) -> Ui {
    label(text).size(12.0).bold(true).tone(Tone::Faint).boxed()
}

/// One cell of a row of states: a caption over what it names.
fn cell(name: &'static str, view: Ui) -> Ui {
    column((caption(name), view))
        .gap(8.0)
        .width(px(168.0))
        .align(AlignItems::FlexStart)
        .boxed()
}

type Look = fn(&mut Cx);

/// A button of one look in each of its states.
fn states(
    which: Glyph,
    text: &'static str,
    look: Look,
    variant: Variant,
) -> Ui {
    let one = move || {
        marked(which, text)
            .rules(look)
            .tagged(variant)
            .on_activate(|_| {})
    };
    row((
        cell("At rest", one().boxed()),
        cell("Under the pointer", one().tagged(Held::Hover).boxed()),
        cell("Held", one().tagged(Held::Press).boxed()),
        cell("Done", one().tagged(Held::Done).boxed()),
    ))
    .gap(16.0)
    .boxed()
}

fn field(text: &'static str) -> impl View<Bevy, HamletTheme> {
    text_field(text.to_string(), |_, _| {})
        .width(px(160.0))
        .rules(text_input)
}

fn fields() -> Ui {
    row((
        cell("At rest", field("A reason").boxed()),
        cell(
            "Under the pointer",
            field("A reason").tagged(Held::Hover).boxed(),
        ),
        cell(
            "Typed in",
            field("A reason").tagged(Held::Focus).boxed(),
        ),
    ))
    .gap(16.0)
    .boxed()
}

fn title(name: &'static str) -> impl View<Bevy, HamletTheme> {
    button(
        label(name)
            .size(13.0)
            .bold(true)
            .wrap(false)
            .tagged(DisplayText),
    )
    .height(px(34.0))
    .rules(menu::nav)
}

fn titles() -> Ui {
    row((
        cell(
            "At rest",
            title("Hamlet").tagged(Underglow::new(false)).boxed(),
        ),
        cell(
            "Under the pointer",
            title("View")
                .tagged(Underglow::new(false))
                .tagged(Held::Hover)
                .boxed(),
        ),
        cell(
            "Its menu open",
            title("View").tagged(Underglow::new(true)).boxed(),
        ),
    ))
    .gap(16.0)
    .boxed()
}

fn tab(
    which: Glyph,
    name: &'static str,
) -> impl View<Bevy, HamletTheme> {
    button(tabs::tab_name(which, name)).rules(tabs::tab_look)
}

fn tabs() -> Ui {
    let theme = HamletTheme::designed();
    let shown = column((
        tab(Glyph::Talk, "Talk")
            .tagged(Underglow::new(true))
            .tagged(tabs::Shown),
        frame().height(px(2.0)).fill(theme.palette.gold_3),
    ))
    .gap(0.0);
    row((
        cell(
            "At rest",
            tab(Glyph::Notes, "Memory")
                .tagged(Underglow::new(false))
                .boxed(),
        ),
        cell(
            "Under the pointer",
            tab(Glyph::Notes, "Memory")
                .tagged(Underglow::new(false))
                .tagged(Held::Hover)
                .boxed(),
        ),
        cell("The one shown", shown.boxed()),
    ))
    .gap(16.0)
    .boxed()
}

/// The hover card, as it is drawn over the world.
fn popups() -> Ui {
    let theme = HamletTheme::designed();
    let card = column((
        label("Juniper")
            .size(theme.text.heading)
            .bold(true)
            .wrap(false),
        label("Reading the orchard notes")
            .size(theme.text.small)
            .tone(Tone::Accent),
        label("Keeps the orchard's notes in order.")
            .size(theme.text.small),
        label("Click to talk, F to follow")
            .size(theme.text.small)
            .tone(Tone::Dim),
    ))
    .gap(3.0)
    .padding(UiRect::axes(px(12.0), px(9.0)))
    .border(1.0)
    .border_color(theme.palette.gold_5)
    .radius(theme.space.soft)
    .fill(theme.palette.card)
    .with(theme.shadow());
    let wide = |name, view: Ui| {
        column((caption(name), view))
            .gap(8.0)
            .align(AlignItems::FlexStart)
    };
    row((
        wide("Hover card", card.boxed()),
        wide(
            "A card in a list",
            lists::item(
                "Orchard".into(),
                "Prune, graft and keep the press.".into(),
                Tone::Body,
            )
            .boxed(),
        ),
    ))
    .gap(28.0)
    .boxed()
}

pub(super) fn controls() -> Ui {
    let theme = HamletTheme::designed();
    let part = |name: &'static str, view: Ui| {
        column((heading(name), view)).gap(10.0)
    };
    column((
        part(
            "A plain button",
            states(
                Glyph::NewSession,
                "New session",
                styles::button,
                Variant::Default,
            ),
        ),
        part(
            "The one primary action",
            states(
                Glyph::Check,
                "Allow once",
                primary_button,
                Variant::Primary,
            ),
        ),
        part(
            "What refuses or stops",
            states(
                Glyph::Cross,
                "Deny",
                danger_button,
                Variant::Danger,
            ),
        ),
        part(
            "A way back",
            states(
                Glyph::Back,
                "All hamlets",
                back_link,
                Variant::Text,
            ),
        ),
        part("A text field", fields()),
        part(
            "One of a few",
            row((
                cell(
                    "Two",
                    styles::segmented_words(
                        ["Manual", "Auto"],
                        0,
                        |_, _| {},
                    )
                    .boxed(),
                ),
                cell(
                    "Three, under the pointer",
                    styles::segmented_words(
                        ["A day", "A week", "A month"],
                        2,
                        |_, _| {},
                    )
                    .boxed(),
                ),
            ))
            .gap(16.0)
            .boxed(),
        ),
        part(
            "A slider",
            bevy_fynix::views::slider(bevy_fynix::resource::<
                SoundSettings,
                _,
            >(|s| s.volume))
            .on_change(|world, level| {
                world.resource_mut::<SoundSettings>().volume = level;
            })
            .boxed(),
        ),
        part("Menu titles", titles()),
        part("Tabs", tabs()),
        part("Cards and popups", popups()),
    ))
    .width(percent(100.0))
    .height(percent(100.0))
    .padding(UiRect::all(px(28.0)))
    .gap(20.0)
    .fill(theme.palette.surface)
    .overflow(Overflow::clip())
    .tagged(platform::Glow)
    .tagged(Highlight::default())
    .boxed()
}

/// The status hues as marks, pills, bars and chips, and the solid
/// buttons that complete a state.
pub(super) fn status_sheet() -> Ui {
    let theme = HamletTheme::designed();
    let part = |name: &'static str, view: Ui| {
        column((heading(name), view)).gap(10.0)
    };
    let kinds = [
        (Glyph::Check, "Done", StatusKind::Success),
        (Glyph::Merge, "Merged", StatusKind::Merged),
        (Glyph::Progress, "In progress", StatusKind::Warning),
        (Glyph::Stop, "Stopped", StatusKind::Danger),
        (Glyph::Bell, "Needs you", StatusKind::Attention),
        (Glyph::Moon, "At rest", StatusKind::Rest),
    ];
    let pills = kinds
        .into_iter()
        .map(|(which, text, kind)| {
            styles::status_pill(which, text, kind).boxed()
        })
        .collect::<Vec<_>>();
    let marks = kinds
        .into_iter()
        .map(|(which, _, kind)| {
            styles::status_mark(which, kind, 22.0).boxed()
        })
        .collect::<Vec<_>>();
    let icons = kinds
        .into_iter()
        .map(|(which, text, kind)| {
            row((glyph(which, 16.0).tone(kind.tone()), caption(text)))
                .gap(6.0)
                .align(AlignItems::Center)
                .boxed()
        })
        .collect::<Vec<_>>();
    column((
        part(
            "The merge button",
            states(
                Glyph::Merge,
                "Merge",
                styles::merge_button,
                Variant::Default,
            ),
        ),
        part(
            "The done button",
            states(
                Glyph::Restore,
                "Restore",
                styles::success_button,
                Variant::Default,
            ),
        ),
        part("Pills", row(pills).gap(10.0).boxed()),
        part("Marks", row(marks).gap(16.0).boxed()),
        part("Agent states, as icons", row(icons).gap(18.0).boxed()),
        part(
            "Bars",
            column((
                styles::progress_bar(0.6, StatusKind::Success)
                    .boxed(),
                styles::progress_bar(0.4, StatusKind::Warning)
                    .boxed(),
            ))
            .gap(10.0)
            .width(px(420.0))
            .boxed(),
        ),
        part(
            "References",
            row((
                styles::reference("home"),
                styles::reference("home#2"),
                styles::reference("feature/orchard-notes"),
            ))
            .gap(8.0)
            .boxed(),
        ),
    ))
    .width(percent(100.0))
    .height(percent(100.0))
    .padding(UiRect::all(px(28.0)))
    .gap(16.0)
    .fill(theme.palette.surface)
    .overflow(Overflow::clip())
    .tagged(platform::Glow)
    .tagged(Highlight::default())
    .boxed()
}

/// Every icon at each size, in the tones it is tinted with, and
/// large.
pub(super) fn icon_sheet() -> Ui {
    let theme = HamletTheme::designed();
    let cells = Glyph::ALL
        .into_iter()
        .map(|which| {
            let small = [12.0, 14.0, 16.0, 20.0]
                .into_iter()
                .map(|size| {
                    glyph(which, size).tone(Tone::Faint).boxed()
                })
                .chain([
                    glyph(which, 14.0).tone(Tone::Accent).boxed(),
                    glyph(which, 14.0).tone(Tone::Dim).boxed(),
                ])
                .collect::<Vec<_>>();
            column((
                caption(format!("{which:?}")),
                row((
                    glyph(which, 40.0).tone(Tone::Body),
                    row(small).gap(6.0).align(AlignItems::Center),
                ))
                .gap(10.0)
                .align(AlignItems::Center),
            ))
            .gap(6.0)
            .width(px(176.0))
            .boxed()
        })
        .collect::<Vec<_>>();
    row(cells)
        .gap(18.0)
        .width(percent(100.0))
        .height(percent(100.0))
        .padding(UiRect::all(px(24.0)))
        .fill(theme.palette.surface)
        .tagged(platform::Wraps)
        .boxed()
}

/// Holds the pointer on the last of the chief's messages.
pub(super) fn hold_message(
    mut commands: Commands,
    rows: Query<(Entity, &chat::Rewinds, &Hovered)>,
) {
    let last = rows.iter().max_by_key(|(_, row, _)| row.at);
    if let Some((row, _, hovered)) = last
        && !hovered.0
    {
        commands.entity(row).insert(Hovered(true));
    }
}

/// Asks before a rewind to the last of the chief's messages, as its
/// mark does, once the chat has settled.
pub(super) fn ask_rewind(world: &mut World, mut updates: Local<u32>) {
    *updates += 1;
    if *updates != SETTLED {
        return;
    }
    let mut rows = world.query::<&chat::Rewinds>();
    let last = rows.iter(world).max_by_key(|row| row.at).cloned();
    if let Some(row) = last {
        chat::ask_rewind(world, row.session, &row.entry);
    }
}

/// Keeps every held control in its state, and presses the ones shown
/// as done when the [`Strike`] comes.
pub(super) fn hold(
    mut commands: Commands,
    scene: Res<Scene>,
    strike: Option<Res<Strike>>,
    held: Query<(Entity, &Held, Option<&Hovered>)>,
    line: Query<&Children, With<chat::ChatField>>,
    texts: Query<(), With<EditableText>>,
    mut focus: ResMut<InputFocus>,
    mut update: Local<u32>,
) {
    *update += 1;
    for (control, state, hovered) in &held {
        let mut entity = commands.entity(control);
        if *state == Held::Focus {
            entity.insert(Focused);
            continue;
        }
        if hovered.is_none_or(|hovered| !hovered.0) {
            entity.insert(Hovered(true));
        }
        if *state == Held::Press {
            entity.insert((Pressed, bevy_fynix::Pressed));
        }
        if *state == Held::Done
            && strike.as_ref().is_some_and(|s| s.0 == *update)
        {
            commands.trigger(Activate { entity: control });
        }
    }
    if *scene != Scene::Field {
        return;
    }
    let typed = line
        .iter()
        .flat_map(|children| children.iter().copied())
        .find(|child| texts.contains(*child));
    if let Some(typed) = typed
        && focus.get() != Some(typed)
    {
        focus.set(typed, FocusCause::Pressed);
    }
}
