//! The highlight of a panel on its tab while another is in front:
//! its glow and its flash rise from the tab's bottom edge, and a
//! badge with the count of what waits shows on the tab.

use bevy::camera::visibility::Visibility;
use bevy::color::{Alpha, Color, Mix};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{Has, With};
use bevy::ecs::system::{Commands, Query, Res};
use bevy::time::Time;
use bevy::ui::widget::Text;
use bevy::ui::{
    BackgroundColor, BackgroundGradient, Gradient, RadialGradient,
    RadialGradientShape, UiPosition, percent,
};
use bevy_fynix::dock::{
    ActiveTab, DockPane, DockTab, TabBadge, TabBadgeCount,
};

use super::highlight::HighlightState;
use super::{EffectColors, EffectSettings, flicker, pool};
use crate::theme::Themed;

/// The most glow a tab's attention glow and its flash throw, as an
/// alpha.
const GLOW: f32 = 0.2;
const FLASH: f32 = 0.4;

/// The most a badge counts to before it says there are more.
const COUNTED: usize = 99;

/// The highlight of a panel as its tab shows it: how far its glow is
/// raised, kept under 1 so that a boost does not blind it, how much
/// of its flash is left, and how many things wait in it.
#[derive(Clone, Copy, Default)]
struct Light {
    attention: f32,
    flash: f32,
    count: usize,
}

/// What a badge says of `count`: the number, or the most it counts to
/// and a plus.
fn counted(count: usize) -> String {
    if count > COUNTED {
        format!("{COUNTED}+")
    } else {
        count.to_string()
    }
}

/// Highlights each tab behind another as its panel is highlighted,
/// and shows its badge, with the count in it, while something waits.
pub(super) fn highlight_tabs(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<EffectSettings>,
    theme: Themed,
    states: Query<(Entity, &HighlightState)>,
    panes: Query<&DockPane>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    tabs: Query<(
        Entity,
        &DockTab,
        Has<ActiveTab>,
        Has<BackgroundGradient>,
    )>,
    mut badges: Query<
        (Entity, &mut Visibility, &mut BackgroundColor),
        With<TabBadge>,
    >,
    mut counts: Query<&mut Text, With<TabBadgeCount>>,
) {
    // The highlight of each pane, from the state in it.
    let lit: Vec<(DockPane, Light)> = states
        .iter()
        .filter(|(_, state)| {
            state.attention > 0.0 || state.flash > 0.0
        })
        .filter_map(|(node, state)| {
            let pane = parents
                .iter_ancestors(node)
                .find_map(|above| panes.get(above).ok())?;
            let light = Light {
                attention: state.attention.min(1.0),
                flash: state.flash,
                count: state.count,
            };
            Some((*pane, light))
        })
        .collect();
    // What shows on a tab: none on the one in front, whose panel
    // shows its own.
    let light_of = |tab: &DockTab, front: bool| -> Light {
        if front {
            return Light::default();
        }
        lit.iter()
            .find(|(pane, _)| {
                pane.leaf == tab.leaf && pane.tab == tab.tab
            })
            .map_or(Light::default(), |(_, light)| *light)
    };
    let colors = EffectColors::of(&theme);
    // In step with the glow over the panel.
    let brightness =
        1.0 + set.flicker * flicker(time.elapsed_secs(), 1.0);
    let hue = colors.attention.mix(&colors.glow_core, 0.35);

    for (node, tab, front, has) in &tabs {
        let Light {
            attention, flash, ..
        } = light_of(tab, front);
        if attention <= 0.0 && flash <= 0.0 {
            if has {
                commands
                    .entity(node)
                    .try_remove::<BackgroundGradient>();
            }
            continue;
        }
        let mut glows = Vec::new();
        if attention > 0.0 {
            glows.push(underglow_pool(|near| {
                hue.with_alpha(attention * brightness * near * GLOW)
            }));
        }
        if flash > 0.0 {
            glows.push(underglow_pool(|near| {
                colors.flash.with_alpha(flash * flash * near * FLASH)
            }));
        }
        commands.entity(node).try_insert(BackgroundGradient(glows));
    }

    for (badge, mut visibility, mut fill) in &mut badges {
        let count = parents
            .iter_ancestors(badge)
            .find_map(|above| tabs.get(above).ok())
            .map_or(0, |(_, tab, front, _)| {
                light_of(tab, front).count
            });
        // Solid, so that the number reads.
        let (shown, color) = if count > 0 {
            (Visibility::Inherited, hue)
        } else {
            (Visibility::Hidden, Color::NONE)
        };
        visibility.set_if_neq(shown);
        fill.set_if_neq(BackgroundColor(color));
        let said = if count > 0 {
            counted(count)
        } else {
            String::new()
        };
        for inside in children.iter_descendants(badge) {
            if let Ok(mut text) = counts.get_mut(inside)
                && text.0 != said
            {
                text.0.clone_from(&said);
            }
        }
    }
}

/// A pool of glow from under the middle of a tab, as the underglow
/// is, coloured by `paint` from how much glow is left.
fn underglow_pool(paint: impl Fn(f32) -> Color) -> Gradient {
    Gradient::from(RadialGradient::new(
        UiPosition::BOTTOM,
        RadialGradientShape::Ellipse(percent(80.0), percent(150.0)),
        pool(2.4, paint),
    ))
}
