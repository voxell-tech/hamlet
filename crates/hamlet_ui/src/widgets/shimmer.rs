//! Words a bright band crosses letter by letter while what they name
//! goes on, or that pulse slowly while less motion is asked for.

use std::f32::consts::TAU;

use bevy::app::{App, PostUpdate};
use bevy::color::Mix;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{With, Without};
use bevy::ecs::system::{Query, Res};
use bevy::text::{TextColor, TextFont, TextSpan};
use bevy::time::Time;
use bevy_fynix::ReducedMotion;
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::label;

use crate::theme::Themed;
use crate::{AnyView, Cx};

/// The seconds the band takes to cross a word, from before its first
/// letter to past its last.
const PERIOD: f64 = 2.4;

/// The letters either side of the band's middle that it brightens.
const SPREAD: f32 = 3.0;

/// The seconds one pulse takes, and how far toward bright it goes.
const PULSE: f64 = 2.4;
const PULSE_REACH: f32 = 0.6;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, glide);
}

/// Words drawn by [`shimmer`], one span to a letter.
#[derive(Component)]
pub struct Shimmer;

/// `word` in the dim tone, a band of the body's tone crossing it.
/// The rules of its scope set its size and weight.
pub fn shimmer(word: impl Into<String>) -> AnyView {
    let word = word.into();
    AnyView::new(move |cx: &mut Cx| {
        let root = cx.build(label("").wrap(false));
        let font = cx
            .world
            .get::<TextFont>(root)
            .cloned()
            .unwrap_or_default();
        cx.world.entity_mut(root).insert(Shimmer);
        for letter in word.chars() {
            cx.world.spawn((
                TextSpan(letter.to_string()),
                font.clone(),
                TextColor::default(),
                ChildOf(root),
            ));
        }
        root
    })
}

/// How bright each letter of every shimmer is now.
fn glide(
    time: Res<Time>,
    reduced: Option<Res<ReducedMotion>>,
    theme: Themed,
    words: Query<(&Children, &TextFont), With<Shimmer>>,
    mut letters: Query<
        (&mut TextColor, &mut TextFont),
        (With<TextSpan>, Without<Shimmer>),
    >,
) {
    let (dim, bright) =
        (theme.tone(Tone::Dim), theme.tone(Tone::Body));
    let now = time.elapsed_secs_f64();
    let still = reduced.is_some_and(|reduced| reduced.0);
    let pulse = (0.5
        - 0.5 * (TAU * (now % PULSE / PULSE) as f32).cos())
        * PULSE_REACH;
    let crossed = (now % PERIOD / PERIOD) as f32;
    for (children, font) in &words {
        let count = children.len() as f32;
        // From before the first letter to past the last.
        let band = crossed * (count + 2.0 * SPREAD) - SPREAD;
        for (at, child) in children.iter().enumerate() {
            let Ok((mut colour, mut kept)) = letters.get_mut(*child)
            else {
                continue;
            };
            let lift = match still {
                true => pulse,
                false => {
                    let off = (at as f32 + 0.5 - band).abs() / SPREAD;
                    let near = (1.0 - off).max(0.0);
                    near * near * (3.0 - 2.0 * near)
                }
            };
            colour.0 = dim.mix(&bright, lift);
            if kept.font_size != font.font_size
                || kept.weight != font.weight
            {
                *kept = font.clone();
            }
        }
    }
}
