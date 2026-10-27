//! The light of a panel on its tab while another is in front: its
//! lamp and its flash rise from the tab's foot, and a badge with the
//! count of what waits burns on the tab.

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

use super::backdrop::Lamplight;
use super::{FeelSettings, MagicColors, pool, waver};
use crate::theme::Themed;

/// The most light a tab's lamp and its flash throw, as an alpha.
const LAMP: f32 = 0.2;
const FLASH: f32 = 0.4;

/// The most a badge counts to before it says there are more.
const COUNTED: usize = 99;

/// The light of a panel as its tab shows it: how far its lamp is
/// turned up, kept under 1 so that a ring does not blind it, how much
/// of its flash is left, and how many things wait in it.
#[derive(Clone, Copy, Default)]
struct Light {
    alarm: f32,
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

/// Lights each tab behind another as its panel is lit, and burns its
/// badge, with the count in it, while something waits.
pub(super) fn light(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<FeelSettings>,
    theme: Themed,
    lamps: Query<(Entity, &Lamplight)>,
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
    // The light of each pane, from the lamp in it.
    let lit: Vec<(DockPane, Light)> = lamps
        .iter()
        .filter(|(_, lamp)| lamp.alarm > 0.0 || lamp.flash > 0.0)
        .filter_map(|(node, lamp)| {
            let pane = parents
                .iter_ancestors(node)
                .find_map(|above| panes.get(above).ok())?;
            let light = Light {
                alarm: lamp.alarm.min(1.0),
                flash: lamp.flash,
                count: lamp.count,
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
    let colors = MagicColors::of(&theme);
    // In step with the flame over the panel.
    let flame = 1.0 + set.waver * waver(time.elapsed_secs(), 1.0);
    let hue = colors.alarm.mix(&colors.flame, 0.35);

    for (node, tab, front, has) in &tabs {
        let Light { alarm, flash, .. } = light_of(tab, front);
        if alarm <= 0.0 && flash <= 0.0 {
            if has {
                commands
                    .entity(node)
                    .try_remove::<BackgroundGradient>();
            }
            continue;
        }
        let mut lights = Vec::new();
        if alarm > 0.0 {
            lights.push(foot_pool(|near| {
                hue.with_alpha(alarm * flame * near * LAMP)
            }));
        }
        if flash > 0.0 {
            lights.push(foot_pool(|near| {
                colors.flash.with_alpha(flash * flash * near * FLASH)
            }));
        }
        commands.entity(node).try_insert(BackgroundGradient(lights));
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

/// A pool of light from under the middle of a tab, as the foot light
/// is, coloured by `paint` from how much light is left.
fn foot_pool(paint: impl Fn(f32) -> Color) -> Gradient {
    Gradient::from(RadialGradient::new(
        UiPosition::BOTTOM,
        RadialGradientShape::Ellipse(percent(80.0), percent(150.0)),
        pool(2.4, paint),
    ))
}
