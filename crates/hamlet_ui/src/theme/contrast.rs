//! How well one colour reads on another, as WCAG 2 measures it.

use bevy::color::{Color, LinearRgba, Luminance, Oklcha};

/// AA for normal text (WCAG 2 1.4.3). No panel text is WCAG-large:
/// `assets/measures.toml` sizes run 11 to 16 px.
pub const TEXT: f32 = 4.5;

/// The contrast of two colours.
pub fn contrast(one: Color, other: Color) -> f32 {
    let (a, b) = (light(one), light(other));
    a.max(b) / a.min(b)
}

/// `colour`, its hue kept, made lighter or darker, whichever is
/// nearer, until it reads at `ratio` on every one of `grounds`. Black
/// or white when no tone of its hue does.
pub fn reach(colour: Color, grounds: &[Color], ratio: f32) -> Color {
    let reads = |colour: &Color| {
        grounds
            .iter()
            .all(|ground| contrast(*colour, *ground) >= ratio)
    };
    let paint = Oklcha::from(colour);
    let toned = |lightness: f32| {
        Color::from(Oklcha {
            lightness: lightness.clamp(0.0, 1.0),
            ..paint
        })
    };
    (0..=100)
        .map(|step| step as f32 * 0.01)
        .flat_map(|by| {
            [toned(paint.lightness + by), toned(paint.lightness - by)]
        })
        .find(reads)
        .unwrap_or_else(|| {
            let least = |colour: Color| {
                grounds
                    .iter()
                    .map(|ground| contrast(colour, *ground))
                    .fold(f32::INFINITY, f32::min)
            };
            clearer_by(Color::BLACK, Color::WHITE, least)
        })
}

/// Whichever of `one` and `other` reads better on `ground`.
pub fn clearer(one: Color, other: Color, ground: Color) -> Color {
    clearer_by(one, other, |colour| contrast(colour, ground))
}

/// The relative luminance of `colour`, as WCAG 2 has it, plus its
/// flare.
pub(super) fn light(colour: Color) -> f32 {
    LinearRgba::from(colour).luminance() + 0.05
}

fn clearer_by(
    one: Color,
    other: Color,
    reads: impl Fn(Color) -> f32,
) -> Color {
    if reads(one) >= reads(other) {
        one
    } else {
        other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_moves_away_from_the_ground() {
        // Reads at about 4:1 on both grounds, so it must move on
        // each.
        let paint = Color::srgb(0.7, 0.35, 0.2);
        let lightness =
            |colour: Color| Oklcha::from(colour).lightness;
        for (ground, darkens) in [
            (Color::srgb(0.96, 0.93, 0.85), true),
            (Color::srgb(0.07, 0.06, 0.06), false),
        ] {
            let reached = reach(paint, &[ground], TEXT);
            assert!(contrast(reached, ground) >= TEXT);
            assert_eq!(
                lightness(reached) < lightness(paint),
                darkens,
                "{reached:?} on {ground:?}"
            );
        }
    }
}
