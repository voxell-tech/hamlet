//! Tabs within a panel: lettering that shares the width over a
//! hairline, the one shown lit and underlined.

use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::ui::{BackgroundColor, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, row,
};
use bevy_fynix::{Bevy, ScopedExt, View, ViewExt, style};

use crate::feel::FootLight;
use crate::icons::Glyph;
use crate::looks::{hairline, headed};
use crate::platform::DisplayText;
use crate::theme::{BRASS_3, BRASS_6, Lamplight};
use crate::{Build, Ui};

/// The tab of a strip that chooses among the values of `R`.
#[derive(Component, Clone, Copy)]
pub struct TabOf<R>(R);

/// The line under the tab of `R`.
#[derive(Component, Clone, Copy)]
pub struct Underline<R>(R);

/// On the tab that is shown.
#[derive(Component)]
pub struct Shown;

/// A tab: lettering alone, bright on the one shown.
pub fn tab_look(cx: &mut Build) {
    style::<Lamplight>()
        .frame(|f, _| {
            f.fill(Color::NONE)
                .radius(0.0)
                .padding(UiRect::axes(px(4.0), px(0.0)))
                .min_height(px(30.0))
        })
        .tone(Tone::Dim)
        .hovered(|s| s.fill(|_| Color::NONE).tone(Tone::Body))
        .pressed(|s| s.fill(|_| Color::NONE).tone(Tone::Faint))
        .when::<Shown>(|s| s.tone(Tone::Body))
        .transition(Motion::Interact)
        .apply(cx);
}

/// A tab's icon and its name.
pub fn tab_name(
    which: Glyph,
    name: &'static str,
) -> impl View<Bevy, Lamplight> {
    headed(
        which,
        label(name)
            .size(12.0)
            .bold(true)
            .wrap(false)
            .tagged(DisplayText),
    )
}

/// A strip of `tabs`, each pressed to `choose` its value. The tabs
/// are built once and marked as the value changes, so that a press
/// moves nothing.
pub fn strip<R: Resource + Copy>(
    tabs: &[(R, Glyph, &'static str)],
    choose: fn(&mut World, R),
) -> Ui {
    let tabs = tabs
        .iter()
        .map(|&(of, which, name)| {
            column((
                button(tab_name(which, name))
                    .rules(tab_look)
                    .tagged(TabOf(of))
                    .tagged(FootLight::new(false))
                    .on_activate(move |world| choose(world, of)),
                frame()
                    .height(px(2.0))
                    .fill(Color::NONE)
                    .tagged(Underline(of)),
            ))
            .grow(1.0)
            .gap(0.0)
            .boxed()
        })
        .collect::<Vec<_>>();
    column((row(tabs).gap(0.0), hairline(BRASS_6)))
        .gap(0.0)
        .boxed()
}

/// Marks the tab of the value of `R`, holds its light and draws its
/// underline.
pub fn mark<R: Resource + Copy + PartialEq>(
    mut commands: Commands,
    shown: Res<R>,
    mut tabs: Query<(Entity, &TabOf<R>, &mut FootLight, Has<Shown>)>,
    mut lines: Query<(&Underline<R>, &mut BackgroundColor)>,
) {
    for (tab, of, mut light, marked) in &mut tabs {
        let on = of.0 == *shown;
        if light.held != on {
            light.held = on;
        }
        if on && !marked {
            commands.entity(tab).try_insert(Shown);
        } else if !on && marked {
            commands.entity(tab).try_remove::<Shown>();
        }
    }
    for (of, mut fill) in &mut lines {
        let want = if of.0 == *shown { BRASS_3 } else { Color::NONE };
        if fill.0 != want {
            fill.0 = want;
        }
    }
}
