//! Tabs within a panel: lettering that shares the width over a
//! hairline, the one shown filled. Where the panel is
//! too narrow for them, only the one shown stays, beside a button
//! that lists them all.

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, ComputedNode, Display, Node, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, menu_button, row,
};
use bevy_fynix::{ScopedExt, ViewExt, style};

use super::effects::Underglow;
use super::icons::Glyph;
use super::platform::show;
use crate::styles::{ICON, hairline, headed};
use crate::theme::HamletTheme;
use crate::{Cx, View, with_cx};

pub fn plugin(app: &mut App) {
    app.add_systems(Update, fit);
}

/// The tab of a strip that chooses among the values of `R`.
#[derive(Component, Clone, Copy)]
pub struct TabOf<R>(R);

/// On the tab that is shown.
#[derive(Component)]
pub struct Shown;

/// On the row of a strip: the width its tabs need side by side, in
/// logical pixels, while the row is narrower than that and they are
/// folded into a list.
#[derive(Component, Default)]
pub struct Folds(Option<f32>);

/// On the button of a strip that lists its tabs while they are
/// folded.
#[derive(Component)]
pub struct Listed;

/// A tab: lettering alone, bright on the one shown.
pub fn tab_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .radius(0.0)
                .padding(UiRect::axes(px(t.space.gap / 2.0), px(0.0)))
                .min_height(px(t.space.tab_height))
        })
        .label(|l, t| l.size(t.text.small).bold(true))
        .tone(Tone::Dim)
        .hovered(|s| {
            s.fill(|t: &HamletTheme| t.palette.button)
                .tone(Tone::Body)
        })
        .pressed(|s| {
            s.fill(|t: &HamletTheme| t.palette.button_hover)
                .tone(Tone::Faint)
        })
        .when::<Shown>(|s| {
            s.fill(|t: &HamletTheme| t.palette.button_hover)
                .tone(Tone::Body)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// A tab's icon and its name.
pub fn tab_name(which: Glyph, name: &'static str) -> impl View {
    headed(which, label(name).wrap(false))
}

/// A strip of `tabs`, each pressed to `choose` its value. The tabs
/// are built once and marked as the value changes, so that a press
/// moves nothing.
pub fn strip<R: Resource + Copy>(
    tabs: &[(R, Glyph, &'static str)],
    choose: fn(&mut World, R),
) -> impl View {
    let values = tabs.iter().map(|tab| tab.0).collect::<Vec<_>>();
    let list = row((menu_button(
        "",
        tabs.iter().map(|tab| tab.2),
        move |world, at| choose(world, values[at]),
    )
    .icon(Glyph::Menu.image(ICON)),))
    .align(AlignItems::Center)
    .display(Display::None)
    .tagged(Listed)
    .boxed();
    let tabs = tabs
        .iter()
        .map(|&(of, which, name)| {
            column((button(tab_name(which, name))
                .rules(tab_look)
                .tagged(TabOf(of))
                .tagged(Underglow::new(false))
                .on_activate(move |world| choose(world, of)),))
            .grow(1.0)
            .gap(0.0)
            .boxed()
        })
        .chain([list])
        .collect::<Vec<_>>();
    column((
        row(tabs).gap(0.0).tagged(Folds::default()),
        with_cx(|cx| hairline(cx.theme().palette.border)),
    ))
    .gap(0.0)
}

/// Folds the tabs of a strip too narrow for them into a list beside
/// the one shown, and sets them side by side again when there is
/// room.
pub fn fit(
    mut strips: Query<(&mut Folds, &ComputedNode, &Children)>,
    mut parts: Query<(
        &mut Node,
        &ComputedNode,
        Option<&Children>,
        Has<Listed>,
    )>,
    shown: Query<(), With<Shown>>,
) {
    for (mut folds, strip, children) in &mut strips {
        let scale = strip.inverse_scale_factor();
        let room = strip.size().x * scale;
        match folds.0 {
            Some(needed) if room >= needed => folds.0 = None,
            Some(_) => {}
            None => {
                let needed = children
                    .iter()
                    .filter_map(|part| parts.get(*part).ok())
                    .filter(|(.., listed)| !listed)
                    .map(|(_, tab, ..)| tab.size().x * scale)
                    .sum::<f32>();
                // Half a pixel, for the rounding of the layout.
                if needed > room + 0.5 {
                    folds.0 = Some(needed);
                }
            }
        }
        let folded = folds.0.is_some();
        for part in children.iter() {
            let Ok((mut node, _, within, listed)) =
                parts.get_mut(*part)
            else {
                continue;
            };
            let lit = within.is_some_and(|within| {
                within.iter().any(|tab| shown.contains(*tab))
            });
            show(
                &mut node,
                if listed { folded } else { !folded || lit },
            );
        }
    }
}

/// Marks the tab of the value of `R`.
pub fn mark<R: Resource + Copy + PartialEq>(
    mut commands: Commands,
    shown: Res<R>,
    tabs: Query<(Entity, &TabOf<R>, Has<Shown>)>,
) {
    for (tab, of, marked) in &tabs {
        let on = of.0 == *shown;
        if on && !marked {
            commands.entity(tab).try_insert(Shown);
        } else if !on && marked {
            commands.entity(tab).try_remove::<Shown>();
        }
    }
}
