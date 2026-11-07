//! The Appearance page: light or dark, and the theme of the panels.

use bevy::color::Color;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy::window::WindowTheme;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_desktop::color_scheme::ColorScheme;
use hamlet_ui::styles::{
    ICON, bare, card_choice, card_name, description, group, note,
    parts,
};
use hamlet_ui::theme::{Palette, Theme, Themes};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, View, with_cx};

use super::field;
use crate::appearance::{Appearance, Mode};
use crate::roll::{either, moved};
use crate::root::Lasting;

/// The width of a theme's small painted panel.
const SWATCH: f32 = 128.0;

pub(super) fn page() -> impl View {
    parts((field("Light or dark", modes()), field("Theme", themes())))
}

/// The three modes, each with what it does, and why the panels stay
/// as they are where the theme has no variant for the mode.
fn modes() -> AnyView {
    let picked = derived(|world: &World| {
        let appearance = world.resource::<Appearance>();
        let scheme = *world.resource::<ColorScheme>();
        let missing = missing(appearance, scheme, world);
        (appearance.pick().mode, scheme, missing)
    })
    .when(either(
        either(moved::<Appearance>(), moved::<ColorScheme>()),
        moved::<Themes>(),
    ));
    keyed(picked, |(picked, scheme, missing)| {
        let options = [Mode::System, Mode::Light, Mode::Dark]
            .into_iter()
            .map(|mode| mode_card(mode, *picked == mode, *scheme))
            .collect::<Vec<_>>();
        let mut shown = vec![group(options).boxed()];
        if let Some(missing) = missing {
            shown.push(note(missing.clone()).boxed());
        }
        group(shown).boxed()
    })
    .within(bare())
    .boxed()
}

/// Why the panels are not in the mode picked: the theme has no
/// variant for it. None where it has.
fn missing(
    appearance: &Appearance,
    scheme: ColorScheme,
    world: &World,
) -> Option<String> {
    let wanted = appearance.light_or_dark(scheme)?;
    let theme =
        world.resource::<Themes>().get(&appearance.pick().theme);
    if theme.variants.iter().any(|(shade, _)| shade == wanted) {
        return None;
    }
    let (only, _) = theme.variants.iter().next()?;
    let only = shade_name(only);
    Some(format!(
        "{} comes only in {only}, so the panels stay {only}.",
        theme.name
    ))
}

fn mode_card(
    mode: Mode,
    picked: bool,
    scheme: ColorScheme,
) -> AnyView {
    let (name, does) = match mode {
        Mode::System => ("Match system", system_words(scheme)),
        Mode::Light => {
            ("Light", "Always light, however your computer is set.")
        }
        Mode::Dark => {
            ("Dark", "Always dark, however your computer is set.")
        }
    };
    picked_card(
        format!("mode {mode:?}"),
        picked,
        column((card_name(name, Tone::Body), description(does)))
            .gap(4.0),
        move |world| {
            world.resource_mut::<Appearance>().pick_mode(mode);
        },
    )
}

fn system_words(scheme: ColorScheme) -> &'static str {
    match scheme {
        ColorScheme::Says(WindowTheme::Light) => {
            "Follows your computer, which is light now."
        }
        ColorScheme::Says(WindowTheme::Dark) => {
            "Follows your computer, which is dark now."
        }
        ColorScheme::Silent => {
            "Follows your computer. It does not say which it is, so \
             the panels stay dark."
        }
        ColorScheme::Asking => {
            "Follows your computer, light when it is light and dark \
             when it is dark."
        }
    }
}

fn shade_name(shade: WindowTheme) -> &'static str {
    match shade {
        WindowTheme::Light => "light",
        WindowTheme::Dark => "dark",
    }
}

/// A card for each theme, the one picked marked.
fn themes() -> AnyView {
    let shown = derived(|world: &World| {
        let picked = world.resource::<Appearance>().pick().theme;
        let themes = world.resource::<Themes>();
        themes
            .iter()
            .map(|theme| (theme.id == picked, theme.clone()))
            .collect::<Vec<_>>()
    })
    .when(either(moved::<Appearance>(), moved::<Themes>()));
    keyed(shown, |themes| {
        let cards = themes
            .iter()
            .map(|(picked, theme)| theme_card(theme, *picked))
            .collect::<Vec<_>>();
        with_cx(move |cx| {
            row(cards)
                .gap(cx.theme().space.gap)
                .align(AlignItems::FlexStart)
                .tagged(Wraps)
        })
        .boxed()
    })
    .within(bare())
    .boxed()
}

/// A theme: each of its variants painted in its own colours, and its
/// name.
fn theme_card(theme: &Theme, picked: bool) -> AnyView {
    let id = theme.id.clone();
    let variants = theme
        .variants
        .iter()
        .map(|(shade, variant)| {
            column((
                swatch(&variant.palette),
                note(format!(
                    "{}, {}",
                    variant.name,
                    shade_name(shade)
                )),
            ))
            .gap(4.0)
            .boxed()
        })
        .collect::<Vec<_>>();
    picked_card(
        format!("theme {id}"),
        picked,
        column((
            card_name(theme.name.clone(), Tone::Body),
            row(variants).gap(12.0).tagged(Wraps),
        ))
        .gap(8.0),
        move |world| {
            world.resource_mut::<Appearance>().pick_theme(id.clone());
        },
    )
}

/// One of several answers as a card, marked while it is the one
/// picked.
fn picked_card(
    key: String,
    picked: bool,
    content: impl View + 'static,
    pick: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    let mark = if picked {
        glyph(Glyph::Check, ICON).tone(Tone::Accent).boxed()
    } else {
        frame().width(px(ICON)).boxed()
    };
    button(
        row((column((content,)).grow(1.0).min_width(px(0.0)), mark))
            .gap(12.0)
            .grow(1.0)
            .align(AlignItems::FlexStart),
    )
    .rules(card_choice(picked))
    .tagged(Lasting(format!("appearance {key}")))
    .on_activate(pick)
    .boxed()
}

/// A small panel in `palette`'s own colours, never the shown theme's:
/// a card with a line of text and a dim line, a button, and the
/// colours that mark what is live, lit, refused and added.
fn swatch(palette: &Palette) -> impl View {
    let line = |colour: Color, wide: f32| {
        frame()
            .width(percent(wide))
            .height(px(5.0))
            .radius(2.5)
            .fill(colour)
    };
    let dot = |colour: Color| {
        frame()
            .width(px(10.0))
            .height(px(10.0))
            .radius(5.0)
            .fill(colour)
    };
    column((
        column((
            line(palette.gold_1, 80.0),
            line(palette.grey_1, 55.0),
        ))
        .gap(5.0)
        .padding(UiRect::all(px(7.0)))
        .width(percent(100.0))
        .radius(4.0)
        .border(1.0)
        .border_color(palette.gold_6)
        .fill(palette.card),
        row((
            row((line(palette.gold_2, 100.0),))
                .width(px(34.0))
                .padding(UiRect::axes(px(6.0), px(5.0)))
                .radius(4.0)
                .fill(palette.button),
            frame().grow(1.0),
            dot(palette.accent),
            dot(palette.glow),
            dot(palette.red),
            dot(palette.green),
        ))
        .gap(5.0)
        .width(percent(100.0))
        .align(AlignItems::Center),
    ))
    .gap(8.0)
    .width(px(SWATCH))
    .padding(UiRect::all(px(8.0)))
    .radius(6.0)
    .border(1.0)
    .border_color(palette.gold_5)
    .fill(palette.surface)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::{Time, TimeUpdateStrategy, Virtual};
    use hamlet_assets::Tuning;
    use hamlet_ui::theme::{HamletTheme, Measures, ThemeId};

    use super::*;
    use crate::screens::settings::Pane;
    use crate::testing::{keeping, press_and_update, settings_page};

    #[test]
    fn picking_light_shows_parchment_and_keeps_the_pick() {
        let dir = hamlet_testing::Scratch::new("appearance-light");
        let mut app = settings_page(Pane::Front);
        app.insert_resource(Themes::shipped())
            .insert_resource(Measures::designed())
            .insert_resource(ColorScheme::Silent)
            // Each update a second, past the wait before the view
            // is written.
            .insert_resource(TimeUpdateStrategy::ManualDuration(
                Duration::from_secs(1),
            ))
            .add_plugins(crate::appearance::plugin);
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(Duration::from_secs(1));
        keeping(&mut app, dir.join("view"));

        press_and_update(&mut app, "Appearance");
        press_and_update(&mut app, "Light");
        app.update();

        let parchment = Themes::shipped()
            .get(&ThemeId::default())
            .variants
            .get(WindowTheme::Light)
            .palette
            .surface;
        let shown = app
            .world()
            .resource::<bevy_fynix::Theme<HamletTheme>>()
            .0
            .palette
            .surface;
        assert_eq!(shown, parchment);
        let kept =
            std::fs::read_to_string(dir.join("view/view.toml"))
                .unwrap()
                .parse::<toml::Table>()
                .unwrap();
        assert_eq!(
            kept["appearance"]["mode"].as_str(),
            Some("light")
        );
        assert_eq!(
            kept["appearance"]["theme"].as_str(),
            Some("hamlet")
        );
    }
}
