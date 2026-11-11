//! A bar of every hue that runs along itself, for what has no limit.

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, Res};
use bevy::time::Time;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient, px};
use bevy_fynix::views::{BehaviorExt, frame};

use crate::View;

/// How far the hues run along the bar, in degrees of hue a second.
const SPEED: f32 = 90.0;

/// The hues across the bar at once.
const STOPS: usize = 7;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, run);
}

/// A bar [`rainbow_bar`] draws, whose hues [`run`] moves.
#[derive(Component)]
#[require(BackgroundGradient)]
pub struct Rainbow;

/// A bar `width` by `height`, round at its ends, of every hue.
pub fn rainbow_bar(width: f32, height: f32) -> impl View {
    frame()
        .width(px(width))
        .height(px(height))
        .shrink(0.0)
        .radius(height / 2.0)
        .tagged(Rainbow)
}

/// Moves the hues of every bar along it as time passes.
fn run(
    time: Res<Time>,
    mut bars: Query<&mut BackgroundGradient, With<Rainbow>>,
) {
    let start = time.elapsed_secs() * SPEED;
    for mut bar in &mut bars {
        let stops = (0..STOPS)
            .map(|at| {
                let hue = (start
                    + at as f32 * 360.0 / (STOPS - 1) as f32)
                    .rem_euclid(360.0);
                ColorStop::auto(Color::hsl(hue, 0.8, 0.62))
            })
            .collect();
        *bar =
            BackgroundGradient::from(LinearGradient::to_right(stops));
    }
}
