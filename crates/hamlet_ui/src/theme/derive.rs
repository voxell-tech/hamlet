//! Every role of a [`Palette`], made from the fifteen colours of a
//! theme variant. The ramp gives the surfaces and the text ladder,
//! the hues give the accent and the statuses, and each ink is moved
//! in OKLCH lightness, its hue and as much chroma as the gamut holds
//! kept, until it reads on the grounds it is drawn on. The mix
//! amounts are the constants below, one set for each mode.

use bevy::color::{Alpha, Color, Mix, Oklcha};

use super::base::{Base, Hue, Mode};
use super::contrast::{MARK, TEXT, contrast};
use super::{Palette, Status, Statuses};

/// How far each role is taken from one colour toward another, and
/// how far a tinted fill is from its panel, in one mode.
struct Steps {
    /// A card, from the surface toward its end of the ramp.
    card: f32,
    /// A button under the pointer, from the button toward the
    /// hairline.
    hover: f32,
    /// A code block, from the card or surface toward the button.
    code: f32,
    /// What is selected, from the surface toward the glow.
    selected: f32,
    /// A gold button's fill under the pointer, from the panel
    /// toward the accent.
    gold_hover: f32,
    /// The glow that gathers at the bottom of a surface.
    bottom: f32,
    /// A primary button's fill away from its glow and near it,
    /// from the panel toward the accent.
    primary_low: f32,
    primary_high: f32,
    /// Pressable text, from the body toward the warm accent.
    pressable: f32,
    /// A mark of the accent, from the body toward the warm accent.
    mark: f32,
    /// The bright centre of a glow, from the glow toward the
    /// lightest end of the ramp.
    core: f32,
    /// How opaque the highlight behind selected words is.
    wash: f32,
    /// How far a status fill is from the surface in lightness, and
    /// the most chroma it keeps; and the same for its fill under
    /// the pointer.
    fill: f32,
    fill_chroma: f32,
    fill_hover: f32,
    fill_hover_chroma: f32,
    /// The contrast an ink of a status reaches, above [`TEXT`].
    ink: f32,
}

const DARK: Steps = Steps {
    card: 0.5,
    hover: 0.43,
    code: 0.5,
    selected: 0.19,
    gold_hover: 0.21,
    bottom: 0.04,
    primary_low: 0.27,
    primary_high: 0.40,
    pressable: 0.38,
    mark: 0.70,
    core: 0.60,
    wash: 0.2,
    fill: 0.11,
    fill_chroma: 0.07,
    fill_hover: 0.16,
    fill_hover_chroma: 0.09,
    ink: 4.75,
};

const LIGHT: Steps = Steps {
    card: 1.0,
    hover: 0.31,
    code: 0.5,
    selected: 0.58,
    gold_hover: 0.10,
    bottom: 0.15,
    primary_low: 0.52,
    primary_high: 1.0,
    pressable: 0.17,
    mark: 1.0,
    core: 0.70,
    wash: 0.4,
    fill: -0.022,
    fill_chroma: 0.035,
    fill_hover: -0.05,
    fill_hover_chroma: 0.04,
    ink: 7.0,
};

/// The lightness the glow keeps at most over paper, where the bright
/// hue of the lamp would not be seen.
const GLOW_LIGHTNESS: f32 = 0.76;
/// The lightness of the agents' colour over paper.
const AGENT_LIGHTNESS: f32 = 0.60;
/// How far the accent's ink is turned toward orange over paper, where
/// amber darkened alone reads as olive.
const ACCENT_WARM: f32 = 0.35;
/// How far above [`TEXT`] an ink of the ladder is moved to read on
/// the fills.
const TEXT_MARGIN: f32 = 0.1;
/// The contrast a mark reaches on the panels, above [`MARK`].
const MARK_MARGIN: f32 = 0.2;
/// The contrast of the gold that fills a button under its words.
const GOLD: f32 = 5.2;
/// The contrast of the light words on a solid pill, circle or
/// button of a status.
const SOLID: f32 = 4.6;
/// The lightness and chroma of a danger button's border and glow.
const BORDER: (f32, f32) = (0.57, 0.17);
const DANGER_GLOW: (f32, f32) = (0.62, 0.2);
/// How opaque the shadow under what floats is.
const SHADOW: [f32; 2] = [0.28, 0.72];

/// Which way [`reach`] may move a colour.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    /// Lighter or darker, whichever is nearer.
    Nearest,
    Darker,
}

/// `colour` as an OKLCH colour with `chroma` cut until it fits the
/// gamut of sRGB.
fn oklch(lightness: f32, chroma: f32, hue: f32) -> Color {
    let lightness = lightness.clamp(0.0, 1.0);
    let mut chroma = chroma.max(0.0);
    loop {
        let colour = Color::from(Oklcha {
            lightness,
            chroma,
            hue,
            alpha: 1.0,
        });
        let rgb = colour.to_srgba();
        let inside = [rgb.red, rgb.green, rgb.blue]
            .iter()
            .all(|c| (-1e-4..=1.0 + 1e-4).contains(c));
        if inside || chroma <= 0.0 {
            return Color::srgb(
                rgb.red.clamp(0.0, 1.0),
                rgb.green.clamp(0.0, 1.0),
                rgb.blue.clamp(0.0, 1.0),
            );
        }
        chroma -= 0.002;
    }
}

fn lch(colour: Color) -> Oklcha {
    let mut paint = Oklcha::from(colour);
    if !paint.hue.is_finite() {
        paint.hue = 0.0;
    }
    paint
}

/// `colour` with its lightness set to `lightness`.
fn with_lightness(colour: Color, lightness: f32) -> Color {
    let paint = lch(colour);
    oklch(lightness, paint.chroma, paint.hue)
}

/// `colour`, its lightness moved until it reads at `need` on every
/// one of `grounds`; black or white when no tone of its hue does.
fn reach(
    colour: Color,
    grounds: &[Color],
    need: f32,
    way: Way,
) -> Color {
    let reads = |colour: &Color| {
        grounds
            .iter()
            .all(|ground| contrast(*colour, *ground) >= need)
    };
    if reads(&colour) {
        return colour;
    }
    let paint = lch(colour);
    for step in 1..=100 {
        let by = step as f32 * 0.01;
        let tries = match way {
            Way::Nearest => {
                [paint.lightness + by, paint.lightness - by]
            }
            Way::Darker => [paint.lightness - by; 2],
        };
        for lightness in tries {
            let moved = oklch(lightness, paint.chroma, paint.hue);
            if reads(&moved) {
                return moved;
            }
        }
    }
    let ground = grounds.first().copied().unwrap_or(Color::BLACK);
    if contrast(Color::BLACK, ground)
        >= contrast(Color::WHITE, ground)
    {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

impl Palette {
    /// The palette of one variant of a theme: every role, from the
    /// ramp and the hues of `base`.
    pub fn derive(base: &Base, mode: Mode) -> Self {
        let dark = mode == Mode::Dark;
        let steps = if dark { &DARK } else { &LIGHT };
        let ramp = base.ramp();
        let [input, surface, ..] = ramp;
        let [_, _, button, border, _, mark, _, dim, body] = ramp;
        // The lightest end of the ramp is the text's in dark and the
        // paper's in light; solids carry it as their words.
        let lightest = if dark { body } else { input };
        let accent = base.hue(base.accent);

        let card = surface
            .mix(&if dark { ramp[2] } else { input }, steps.card);
        let button_hover = button.mix(&border, steps.hover);
        let code = if dark {
            card.mix(&button, steps.code)
        } else {
            surface.mix(&button, steps.code)
        };
        let gold_6 = if dark {
            button_hover.mix(&border, 0.5)
        } else {
            surface.mix(&button, 0.8)
        };
        let glow = if dark {
            accent
        } else {
            let paint = lch(accent);
            with_lightness(
                accent,
                paint.lightness.min(GLOW_LIGHTNESS),
            )
        };
        let gold_4 = reach(accent, &[surface], GOLD, Way::Nearest);
        let warm = if dark { accent } else { gold_4 };
        let statuses = Statuses::derive(
            base,
            &Grounds {
                surface,
                card,
                input,
                code,
                button,
                button_hover,
                lightest,
                dim: mark,
            },
            steps,
        );
        let danger = statuses.danger.fill;
        let danger_hover = statuses.danger.fill_hover;
        let green_deep = statuses.success.fill;
        let selected = surface.mix(&glow, steps.selected);
        let gold_hover = if dark { surface } else { input }
            .mix(&accent, steps.gold_hover);
        let primary_low = if dark { surface } else { input }
            .mix(&accent, steps.primary_low);
        let primary_high = surface.mix(&accent, steps.primary_high);

        // What a button is while it is pressed.
        let pressed = button_hover.mix(&selected, 0.5);
        let fills = [
            pressed,
            surface,
            card,
            input,
            code,
            button,
            button_hover,
            gold_6,
            selected,
            gold_hover,
            primary_low,
            danger,
            danger_hover,
            green_deep,
        ];
        let text = TEXT + TEXT_MARGIN;
        let on_fills =
            |colour: Color| reach(colour, &fills, text, Way::Nearest);
        let panels = [surface, card];
        let on_panels = |colour: Color| {
            reach(colour, &panels, MARK + MARK_MARGIN, Way::Nearest)
        };

        let accent_ink = if dark || base.accent != Hue::Yellow {
            accent
        } else {
            accent.mix(&base.orange, ACCENT_WARM)
        };
        let particle = base.orange;

        Self {
            surface,
            grey_1: on_fills(dim),
            button,
            button_hover,
            border,
            selected,
            gold_1: on_fills(body),
            gold_2: on_fills(body.mix(&warm, steps.pressable)),
            gold_3: on_panels(body.mix(&warm, steps.mark)),
            gold_4,
            gold_5: on_panels(mark),
            gold_6,
            gold_hover,
            accent: on_fills(accent_ink),
            glow,
            glow_core: glow.mix(&lightest, steps.core),
            particle,
            primary_low,
            primary_high,
            agent: if dark {
                base.blue
            } else {
                with_lightness(base.blue, AGENT_LIGHTNESS)
            },
            card,
            input,
            code,
            text_selection: accent.with_alpha(steps.wash),
            bottom_glow: surface.mix(&glow, steps.bottom),
            red: statuses.danger.ink,
            danger,
            danger_border: on_panels(oklch(
                BORDER.0,
                BORDER.1,
                lch(base.red).hue,
            )),
            danger_hover,
            danger_glow: oklch(
                DANGER_GLOW.0,
                DANGER_GLOW.1,
                lch(base.red).hue,
            ),
            green: statuses.success.ink,
            green_deep,
            shadow: if dark {
                Color::BLACK.with_alpha(SHADOW[1])
            } else {
                body.with_alpha(SHADOW[0])
            },
            status: statuses,
        }
    }
}

/// The colours a status is read on.
struct Grounds {
    surface: Color,
    card: Color,
    input: Color,
    code: Color,
    button: Color,
    button_hover: Color,
    /// The words of a solid.
    lightest: Color,
    /// The ramp's muted mark, the ink of what is at rest.
    dim: Color,
}

impl Statuses {
    fn derive(base: &Base, on: &Grounds, steps: &Steps) -> Self {
        let surface_lightness = lch(on.surface).lightness;
        let status = |hue: Color| {
            let paint = lch(hue);
            let fill = oklch(
                surface_lightness + steps.fill,
                paint.chroma.min(steps.fill_chroma),
                paint.hue,
            );
            let fill_hover = oklch(
                surface_lightness + steps.fill_hover,
                paint.chroma.min(steps.fill_hover_chroma),
                paint.hue,
            );
            let grounds = [
                on.surface,
                on.card,
                on.input,
                on.code,
                on.button,
                on.button_hover,
                fill,
                fill_hover,
            ];
            let ink = reach(hue, &grounds, steps.ink, Way::Nearest);
            Status {
                ink,
                fill,
                fill_hover,
                on_fill: reach(ink, &[fill], TEXT, Way::Nearest),
                solid: reach(hue, &[on.lightest], SOLID, Way::Darker),
                on_solid: on.lightest,
            }
        };
        let rest = {
            let fill = on.button;
            let grounds = [
                on.surface,
                on.card,
                on.input,
                on.code,
                on.button,
                on.button_hover,
            ];
            let ink =
                reach(on.dim, &grounds, steps.ink, Way::Nearest);
            Status {
                ink,
                fill,
                fill_hover: on.button_hover,
                on_fill: reach(ink, &[fill], TEXT, Way::Nearest),
                solid: reach(
                    on.dim,
                    &[on.lightest],
                    SOLID,
                    Way::Darker,
                ),
                on_solid: on.lightest,
            }
        };
        Self {
            success: status(base.green),
            merged: status(base.purple),
            warning: status(base.yellow),
            danger: status(base.red),
            info: status(base.blue),
            attention: status(base.orange),
            rest,
        }
    }

    /// Each status by name.
    pub fn named(&self) -> [(&'static str, &Status); 7] {
        [
            ("success", &self.success),
            ("merged", &self.merged),
            ("warning", &self.warning),
            ("danger", &self.danger),
            ("info", &self.info),
            ("attention", &self.attention),
            ("rest", &self.rest),
        ]
    }
}
