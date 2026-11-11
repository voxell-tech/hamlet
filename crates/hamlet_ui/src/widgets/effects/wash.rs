//! The light over what is picked or under the pointer: the glow,
//! strongest at one edge and fading to nothing across, as lamplight
//! falls. One look for a picked row, a shown tab and the pointer on
//! either, in both modes.

use bevy::color::Alpha;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::picking::hover::Hovered;
use bevy::time::Time;
use bevy::ui::{
    BackgroundGradient, ColorStop, Gradient, LinearGradient, percent,
};

use super::{Tuned, ease, lit_panels};
use crate::theme::{HamletTheme, Themed};

/// How strong the glow is at its edge, over dark panels and over
/// light ones, where the same glow shows far less: across a row, and
/// up a tab, which is short, so the light there is softer.
const PEAK: [f32; 2] = [0.16, 0.6];
const TAB_PEAK: [f32; 2] = [0.12, 0.38];
/// The stops of a tab's light, which falls away quickly from its
/// underline and then slowly, with no line where it ends.
const TAB_STOPS: u8 = 6;
/// How much of that the pointer brings, over dark panels and light.
const HOVER: [f32; 2] = [0.7, 0.85];

/// The edge a wash falls from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    /// A row's start, fading across it.
    Left,
    /// A tab's underline, fading up it.
    Bottom,
}

/// A node washed with light while it is `held` and, less, under the
/// pointer.
#[derive(Component, Clone, Copy, Debug)]
#[require(Hovered)]
pub struct Wash {
    pub held: bool,
    pub from: Edge,
    level: f32,
}

impl Wash {
    pub fn new(held: bool, from: Edge) -> Self {
        Self {
            held,
            from,
            level: 0.0,
        }
    }
}

/// The wash of `theme`'s glow from `from` at `level`, 1 for what is
/// picked.
pub fn wash(theme: &HamletTheme, from: Edge, level: f32) -> Gradient {
    let lit = usize::from(lit_panels(theme));
    let glow = theme.palette.glow;
    match from {
        Edge::Left => {
            let peak = PEAK[lit] * level;
            Gradient::from(LinearGradient::to_right(vec![
                ColorStop::new(glow.with_alpha(peak), percent(0.0)),
                ColorStop::new(glow.with_alpha(0.0), percent(100.0)),
            ]))
        }
        Edge::Bottom => {
            let peak = TAB_PEAK[lit] * level;
            let stops = (0..=TAB_STOPS)
                .map(|step| {
                    let t = f32::from(step) / f32::from(TAB_STOPS);
                    let left = (1.0 - t) * (1.0 - t);
                    ColorStop::new(
                        glow.with_alpha(peak * left),
                        percent(t * 100.0),
                    )
                })
                .collect();
            Gradient::from(LinearGradient::to_top(stops))
        }
    }
}

/// Eases each wash toward its hold or the pointer, and paints it.
pub(super) fn update_washes(
    mut commands: Commands,
    time: Res<Time>,
    tuned: Tuned,
    theme: Themed,
    mut washes: Query<(
        Entity,
        &mut Wash,
        &Hovered,
        Has<BackgroundGradient>,
    )>,
) {
    let set = tuned.get();
    let dt = time.delta_secs();
    let hover = HOVER[usize::from(lit_panels(&theme))];
    for (node, mut wash_of, hovered, shown) in &mut washes {
        let want = if wash_of.held {
            1.0
        } else if hovered.0 {
            hover
        } else {
            0.0
        };
        let level = ease(wash_of.level, want, set.glow_rate, dt);
        if level == wash_of.level && (shown || level == 0.0) {
            continue;
        }
        wash_of.level = level;
        if level <= 0.001 {
            commands.entity(node).try_remove::<BackgroundGradient>();
            continue;
        }
        commands.entity(node).try_insert(BackgroundGradient(vec![
            wash(&theme, wash_of.from, level),
        ]));
    }
}
