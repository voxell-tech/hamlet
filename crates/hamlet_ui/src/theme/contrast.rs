//! How well one colour reads on another, as WCAG 2 measures it, and
//! the check every shipped theme passes.

use std::fmt::{self, Write};

use bevy::color::{
    Alpha, Color, ColorToPacked, LinearRgba, Luminance, Mix, Oklcha,
    Srgba,
};
use bevy_fynix::tokens::{SurfaceTokens, TextTokens, Tone};

use super::HamletTheme;

/// AA for normal text (WCAG 2 1.4.3). No panel text is WCAG-large:
/// `assets/measures.toml` sizes run 13 to 16 px.
pub const TEXT: f32 = 4.5;
/// Non-text contrast (WCAG 2 1.4.11): a control's bounds or state,
/// an icon.
pub const MARK: f32 = 3.0;

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

/// The part a colour of a [`super::Palette`] plays, which decides
/// what it is checked on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// What controls sit on.
    Panel,
    /// What text is drawn on inside a panel.
    Fill,
    /// Text and icons.
    Ink,
    /// The ink of a status, held to [`TEXT`] on the panels, a button,
    /// a field, code and its own fill, as `Report::of` pairs them.
    Status,
    /// A control's edge or state, held to [`MARK`].
    Mark,
    /// Light, a line or a shadow, which means nothing alone. Not
    /// checked.
    Light,
}

/// What a pair is held to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Need {
    Text,
    Mark,
}

impl Need {
    fn ratio(self) -> f32 {
        match self {
            Self::Text => TEXT,
            Self::Mark => MARK,
        }
    }
}

/// Whether a pair below its need fails the build or is only
/// reported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gate {
    Fail,
    Report,
}

/// Selected words are reported, not failed: they were read before
/// they were selected, and the highlight lasts only while it is held.
const SELECTION_GATE: Gate = Gate::Report;

/// A ground an ink is never drawn on.
struct Except {
    ink: &'static str,
    ground: &'static str,
    /// The search that shows it.
    why: &'static str,
}

/// Every ink not named here is held to [`TEXT`] on every ground. The
/// derivation moves each ink of the ladder until it reads on every
/// fill, so none is named.
const EXCEPT: &[Except] = &[];

/// The grounds the ink of a status is drawn on, besides its own
/// fill.
const STATUS_GROUNDS: [&str; 6] =
    ["surface", "card", "input", "code", "button", "button_hover"];

/// A colour as the report shows it: opaque, on the 8-bit grid a
/// checker reads.
#[derive(Clone, Debug)]
struct Named {
    name: String,
    colour: Srgba,
}

impl Named {
    fn new(
        name: impl Into<String>,
        colour: impl Into<Srgba>,
    ) -> Self {
        let [r, g, b, _] = colour.into().to_u8_array();
        Self {
            name: name.into(),
            colour: Srgba::rgb_u8(r, g, b),
        }
    }

    /// `rrggbb`.
    fn hex(&self) -> String {
        let [r, g, b] = self.colour.to_u8_array_no_alpha();
        format!("{r:02x}{g:02x}{b:02x}")
    }
}

struct Pair {
    front: Named,
    back: Named,
    need: Need,
    gate: Gate,
    ratio: f32,
}

impl Pair {
    fn new(
        front: Named,
        back: Named,
        need: Need,
        gate: Gate,
    ) -> Self {
        let ratio = contrast(front.colour.into(), back.colour.into());
        Self {
            front,
            back,
            need,
            gate,
            ratio,
        }
    }

    fn passes(&self) -> bool {
        self.ratio >= self.need.ratio()
    }

    /// The pair in WebAIM's contrast checker.
    fn webaim(&self) -> String {
        format!(
            "https://webaim.org/resources/contrastchecker/?fcolor={}&bcolor={}",
            self.front.hex(),
            self.back.hex()
        )
    }
}

/// `gold_5 on surface: #a99172 on #f4ecda, 2.55, a mark needs 3:1,
/// https://...`
impl fmt::Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let needs = match self.need {
            Need::Text => "text needs 4.5:1",
            Need::Mark => "a mark needs 3:1",
        };
        write!(
            f,
            "{} on {}: #{} on #{}, {}, {needs}, {}",
            self.front.name,
            self.back.name,
            self.front.hex(),
            self.back.hex(),
            Shown(self.ratio),
            self.webaim()
        )
    }
}

/// A ratio cut, not rounded, to two decimals, as WebAIM shows it.
struct Shown(f32);

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.2}",
            (f64::from(self.0) * 100.0).floor() / 100.0
        )
    }
}

/// Every pair of one theme, and the pairs excepted with their
/// reasons.
pub struct Report {
    title: String,
    pairs: Vec<Pair>,
    excepted: Vec<(Pair, &'static Except)>,
}

impl Report {
    pub fn of(title: impl Into<String>, theme: &HamletTheme) -> Self {
        let parts = theme.palette.parts();
        let of = |part: Part| {
            parts
                .iter()
                .filter(move |(_, is, _)| *is == part)
                .map(|(name, _, colour)| Named::new(*name, *colour))
        };
        // A translucent fill is a highlight: it is seen over a panel
        // or the chief's own bubble.
        let unders = of(Part::Panel)
            .chain([Named::new("gold_6", theme.palette.gold_6)])
            .collect::<Vec<_>>();
        let mut grounds = Vec::new();
        for (name, part, colour) in &parts {
            if !matches!(part, Part::Panel | Part::Fill) {
                continue;
            }
            if colour.alpha() < 1.0 {
                let top = Srgba::from(*colour);
                grounds.extend(unders.iter().map(|under| {
                    let seen = under
                        .colour
                        .mix(&top.with_alpha(1.0), top.alpha);
                    (
                        Named::new(
                            format!("{name} over {}", under.name),
                            seen,
                        ),
                        SELECTION_GATE,
                    )
                }));
            } else {
                grounds
                    .push((Named::new(*name, *colour), Gate::Fail));
            }
        }
        grounds.push((
            Named::new("pressed", theme.pressed()),
            Gate::Fail,
        ));

        let mut report = Self {
            title: title.into(),
            pairs: Vec::new(),
            excepted: Vec::new(),
        };
        for ink in of(Part::Ink) {
            for (ground, gate) in &grounds {
                let pair = Pair::new(
                    ink.clone(),
                    ground.clone(),
                    Need::Text,
                    *gate,
                );
                let except = EXCEPT.iter().find(|except| {
                    except.ink == ink.name
                        && except.ground == ground.name
                });
                match except {
                    None => report.pairs.push(pair),
                    Some(except) => {
                        report.excepted.push((pair, except));
                    }
                }
            }
        }
        let on_accent =
            Named::new("on_accent", theme.tone(Tone::OnAccent));
        for back in [
            Named::new("gold_4", theme.accent()),
            Named::new("gold_4 hovered", theme.accent_hover()),
            Named::new("gold_4 pressed", theme.accent_pressed()),
        ] {
            report.pairs.push(Pair::new(
                on_accent.clone(),
                back,
                Need::Text,
                Gate::Fail,
            ));
        }
        report.pairs.push(Pair::new(
            Named::new("hint", theme.hint(theme.hint_opacity())),
            Named::new("input", theme.palette.input),
            Need::Text,
            Gate::Fail,
        ));
        let on_status = grounds
            .iter()
            .filter(|(ground, _)| {
                STATUS_GROUNDS.contains(&ground.name.as_str())
            })
            .map(|(ground, _)| ground.clone())
            .collect::<Vec<_>>();
        for (name, status) in theme.palette.status.named() {
            let own = |ink: &str, ink_colour: Color| {
                Named::new(format!("{name} {ink}"), ink_colour)
            };
            let fill =
                Named::new(format!("{name} fill"), status.fill);
            let hovered = Named::new(
                format!("{name} fill hovered"),
                status.fill_hover,
            );
            let solid =
                Named::new(format!("{name} solid"), status.solid);
            let mut pairs = on_status
                .iter()
                .map(|ground| {
                    (own("ink", status.ink), ground.clone())
                })
                .collect::<Vec<_>>();
            let mut marks = on_status
                .iter()
                .map(|ground| {
                    (own("mark", status.mark), ground.clone())
                })
                .collect::<Vec<_>>();
            marks.push((own("mark", status.mark), fill.clone()));
            for (front, back) in marks {
                report.pairs.push(Pair::new(
                    front,
                    back,
                    Need::Mark,
                    Gate::Fail,
                ));
            }
            pairs.push((own("ink", status.ink), fill.clone()));
            pairs.push((own("ink", status.ink), hovered));
            pairs.push((own("on_fill", status.on_fill), fill));
            pairs.push((own("on_solid", status.on_solid), solid));
            for (front, back) in pairs {
                report.pairs.push(Pair::new(
                    front,
                    back,
                    Need::Text,
                    Gate::Fail,
                ));
            }
        }
        for mark in of(Part::Mark) {
            for panel in of(Part::Panel) {
                report.pairs.push(Pair::new(
                    mark.clone(),
                    panel,
                    Need::Mark,
                    Gate::Fail,
                ));
            }
        }
        report
    }

    /// Each pair gated to fail that misses its need, under the title,
    /// or None.
    pub fn failing(&self) -> Option<String> {
        let failures = self.failures().collect::<Vec<_>>();
        if failures.is_empty() {
            return None;
        }
        let mut message = match failures.len() {
            1 => format!("{}, 1 pair fails", self.title),
            n => format!("{}, {n} pairs fail", self.title),
        };
        for pair in failures {
            let _ = write!(message, "\n  {pair}");
        }
        Some(message)
    }

    /// One line: the pairs, the failures and the lowest text pair.
    pub fn summary(&self) -> String {
        let lowest = self
            .held(Need::Text, Gate::Fail)
            .min_by(|a, b| a.ratio.total_cmp(&b.ratio));
        let mut line = format!(
            "{} pairs, {} fail",
            self.pairs.len() + self.excepted.len(),
            self.failures().count()
        );
        if let Some(pair) = lowest {
            let _ = write!(
                line,
                ", lowest text pair {} ({} on {})",
                Shown(pair.ratio),
                pair.front.name,
                pair.back.name
            );
        }
        line
    }

    fn failures(&self) -> impl Iterator<Item = &Pair> {
        self.pairs
            .iter()
            .filter(|pair| pair.gate == Gate::Fail && !pair.passes())
    }

    fn held(
        &self,
        need: Need,
        gate: Gate,
    ) -> impl Iterator<Item = &Pair> {
        self.pairs.iter().filter(move |pair| {
            pair.need == need && pair.gate == gate
        })
    }
}

/// Markdown: the text pairs, the marks, the pairs only reported and
/// the excepted pairs with their reasons.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = |ratio: f32, need: f32| {
            if ratio >= need { "pass" } else { "fail" }
        };
        let colours = |pair: &Pair| {
            format!(
                "`#{}` on `#{}`",
                pair.front.hex(),
                pair.back.hex()
            )
        };
        writeln!(f, "# {}\n", self.title)?;
        writeln!(
            f,
            "{}. Ratios are WCAG 2's, cut to two decimals as WebAIM \
             shows them. Text needs 4.5:1 (AA), a control's edge or \
             an icon 3:1.\n",
            self.summary()
        )?;

        writeln!(f, "## Text\n")?;
        writeln!(
            f,
            "| Text | On | Colours | Ratio | AA | AA large | AAA | \
             AAA large | Check |"
        )?;
        writeln!(f, "|---|---|---|---|---|---|---|---|---|")?;
        for pair in self.held(Need::Text, Gate::Fail) {
            let r = pair.ratio;
            writeln!(
                f,
                "| {} | {} | {} | {} | {} | {} | {} | {} | \
                 [WebAIM]({}) |",
                pair.front.name,
                pair.back.name,
                colours(pair),
                Shown(pair.ratio),
                verdict(r, TEXT),
                verdict(r, 3.0),
                verdict(r, 7.0),
                verdict(r, 4.5),
                pair.webaim()
            )?;
        }

        writeln!(f, "\n## Edges and icons\n")?;
        writeln!(f, "| Mark | On | Colours | Ratio | 3:1 | Check |")?;
        writeln!(f, "|---|---|---|---|---|---|")?;
        for pair in self.held(Need::Mark, Gate::Fail) {
            writeln!(
                f,
                "| {} | {} | {} | {} | {} | [WebAIM]({}) |",
                pair.front.name,
                pair.back.name,
                colours(pair),
                Shown(pair.ratio),
                verdict(pair.ratio, MARK),
                pair.webaim()
            )?;
        }

        writeln!(f, "\n## Selected words, reported only\n")?;
        writeln!(f, "| Text | On | Colours | Ratio | AA | Check |")?;
        writeln!(f, "|---|---|---|---|---|---|")?;
        for pair in self.held(Need::Text, Gate::Report) {
            writeln!(
                f,
                "| {} | {} | {} | {} | {} | [WebAIM]({}) |",
                pair.front.name,
                pair.back.name,
                colours(pair),
                Shown(pair.ratio),
                verdict(pair.ratio, TEXT),
                pair.webaim()
            )?;
        }

        writeln!(f, "\n## Excepted\n")?;
        writeln!(
            f,
            "| Text | Never on | Colours | Ratio | Why | Check |"
        )?;
        writeln!(f, "|---|---|---|---|---|---|")?;
        for (pair, except) in &self.excepted {
            writeln!(
                f,
                "| {} | {} | {} | {} | {} | [WebAIM]({}) |",
                pair.front.name,
                pair.back.name,
                colours(pair),
                Shown(pair.ratio),
                except.why,
                pair.webaim()
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hamlet_assets::Tuning;

    use super::*;
    use crate::theme::{Measures, Themes};

    #[test]
    fn every_shipped_theme_reads() {
        let measures = Measures::designed();
        let failing = Themes::shipped()
            .iter()
            .flat_map(|theme| theme.reports(&measures))
            .filter_map(|(_, report)| report.failing())
            .collect::<Vec<_>>();
        assert!(failing.is_empty(), "{}", failing.join("\n"));
    }

    #[test]
    fn every_exception_names_a_part() {
        let parts = HamletTheme::designed().palette.parts();
        let is = |name: &str, wanted: &[Part]| {
            parts.iter().any(|(part_name, part, _)| {
                *part_name == name && wanted.contains(part)
            })
        };
        for except in EXCEPT {
            assert!(is(except.ink, &[Part::Ink]), "{}", except.ink);
            assert!(
                is(except.ground, &[Part::Panel, Part::Fill]),
                "{}",
                except.ground
            );
        }
    }

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
