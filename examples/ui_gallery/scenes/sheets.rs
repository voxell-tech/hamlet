//! The sheet of controls held in each of their states, and the sheet
//! of icons.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
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
use hamlet_assets::Tuning;
use hamlet_realm::sound::SoundSettings;
use hamlet_ui::feel::{FootLight, Lit, Mood};
use hamlet_ui::icons::{Glyph, glyph};
use hamlet_ui::platform::DisplayText;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::{
    Cx, brass, chat, grim, lamp, lists, marked, menu, platform, tabs,
    well,
};

use super::{Scene, Strike, Ui};

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
    label(text)
        .size(12.0)
        .bold(true)
        .tone(Tone::Faint)
        .tagged(DisplayText)
        .boxed()
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
    mood: Mood,
) -> Ui {
    let one = move || {
        marked(which, text)
            .rules(look)
            .tagged(mood)
            .on_activate(|_| {})
    };
    row((
        cell("AT REST", one().boxed()),
        cell("UNDER THE POINTER", one().tagged(Held::Hover).boxed()),
        cell("HELD", one().tagged(Held::Press).boxed()),
        cell("DONE", one().tagged(Held::Done).boxed()),
    ))
    .gap(16.0)
    .boxed()
}

fn field(text: &'static str) -> impl View<Bevy, HamletTheme> {
    text_field(text.to_string(), |_, _| {})
        .width(px(160.0))
        .rules(well)
}

fn fields() -> Ui {
    row((
        cell("AT REST", field("A reason").boxed()),
        cell(
            "UNDER THE POINTER",
            field("A reason").tagged(Held::Hover).boxed(),
        ),
        cell(
            "TYPED IN",
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
            "AT REST",
            title("HAMLET").tagged(FootLight::new(false)).boxed(),
        ),
        cell(
            "UNDER THE POINTER",
            title("VIEW")
                .tagged(FootLight::new(false))
                .tagged(Held::Hover)
                .boxed(),
        ),
        cell(
            "ITS MENU OPEN",
            title("VIEW").tagged(FootLight::new(true)).boxed(),
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
        tab(Glyph::Talk, "TALK")
            .tagged(FootLight::new(true))
            .tagged(tabs::Shown),
        frame().height(px(2.0)).fill(theme.palette.brass_3),
    ))
    .gap(0.0);
    row((
        cell(
            "AT REST",
            tab(Glyph::Notes, "NOTES")
                .tagged(FootLight::new(false))
                .boxed(),
        ),
        cell(
            "UNDER THE POINTER",
            tab(Glyph::Notes, "NOTES")
                .tagged(FootLight::new(false))
                .tagged(Held::Hover)
                .boxed(),
        ),
        cell("THE ONE SHOWN", shown.boxed()),
    ))
    .gap(16.0)
    .boxed()
}

/// The hover card, as it is drawn over the world.
fn popups() -> Ui {
    let theme = HamletTheme::designed();
    let card = column((
        label("JUNIPER")
            .size(13.0)
            .bold(true)
            .wrap(false)
            .tagged(DisplayText),
        label("Reading the orchard notes")
            .size(12.0)
            .tone(Tone::Accent),
        label("Keeps the orchard's notes in order.").size(12.0),
        label("Click to talk, F to follow")
            .size(12.0)
            .tone(Tone::Dim),
    ))
    .gap(3.0)
    .padding(UiRect::axes(px(12.0), px(9.0)))
    .border(1.0)
    .border_color(theme.palette.brass_5)
    .radius(theme.space.soft)
    .fill(theme.palette.card)
    .with(theme.shadow());
    let wide = |name, view: Ui| {
        column((caption(name), view))
            .gap(8.0)
            .align(AlignItems::FlexStart)
    };
    row((
        wide("HOVER CARD", card.boxed()),
        wide(
            "A CARD IN A LIST",
            lists::item(
                "ORCHARD".into(),
                "Prune, graft and keep the press.".into(),
                Tone::Body,
            ),
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
            "A PLAIN BUTTON",
            states(
                Glyph::NewSession,
                "NEW SESSION",
                brass,
                Mood::Plain,
            ),
        ),
        part(
            "THE ONE GOOD ACTION",
            states(Glyph::Check, "ALLOW ONCE", lamp, Mood::Good),
        ),
        part(
            "WHAT REFUSES OR STOPS",
            states(Glyph::Cross, "DENY", grim, Mood::Bad),
        ),
        part(
            "A WAY BACK",
            states(
                Glyph::Back,
                "ALL HAMLETS",
                hamlet_ui::retrace,
                Mood::Ink,
            ),
        ),
        part("A TEXT FIELD", fields()),
        part(
            "A SLIDER",
            bevy_fynix::views::slider(bevy_fynix::resource::<
                SoundSettings,
                _,
            >(|s| s.volume))
            .on_change(|world, level| {
                world.resource_mut::<SoundSettings>().volume = level;
            })
            .boxed(),
        ),
        part("MENU TITLES", titles()),
        part("TABS", tabs()),
        part("CARDS AND POPUPS", popups()),
    ))
    .width(percent(100.0))
    .height(percent(100.0))
    .padding(UiRect::all(px(28.0)))
    .gap(20.0)
    .fill(theme.palette.black)
    .overflow(Overflow::clip())
    .tagged(platform::Glow)
    .tagged(Lit::default())
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
        .fill(theme.palette.black)
        .tagged(platform::Wraps)
        .boxed()
}

/// Holds the pointer on the last of the chief's messages.
pub(super) fn hold_message(
    mut commands: Commands,
    rows: Query<(Entity, &chat::Rewinds, &Hovered)>,
) {
    let last = rows.iter().max_by_key(|(_, row, _)| row.entry);
    if let Some((row, _, hovered)) = last
        && !hovered.0
    {
        commands.entity(row).insert(Hovered(true));
    }
}

/// Keeps every held control in its state, and presses the ones shown
/// as done when the [`Strike`] comes.
#[allow(clippy::too_many_arguments)]
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
        .filter(|child| texts.contains(*child))
        // The field made last, of the two that are shown.
        .max();
    if let Some(typed) = typed
        && focus.get() != Some(typed)
    {
        focus.set(typed, FocusCause::Pressed);
    }
}
