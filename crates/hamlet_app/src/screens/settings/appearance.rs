//! The Appearance page: light or dark, and the theme of the panels.

use bevy::color::{Alpha, Color};
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, BackgroundGradient, BoxShadow, ColorStop,
    RadialGradient, RadialGradientShape, UiPosition, UiRect, percent,
    px,
};
use bevy::window::WindowTheme;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_desktop::color_scheme::ColorScheme;
use hamlet_ui::styles::{
    ICON, bare, card_choice, card_name, group, note, parts,
    segmented_words,
};
use hamlet_ui::theme::{Palette, Theme, Themes};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, View, with_cx};

use super::field;
use crate::appearance::{Appearance, Mode, ZOOMS, Zoom};
use crate::roll::{either, moved};
use crate::root::Lasting;

/// The width of each variant's painted panel on a theme's tile, and
/// the gap between the two.
const TILE_SWATCH: f32 = 104.0;
const TILE_GAP: f32 = 6.0;
/// The width of what a mode's card holds.
const CHOICE: f32 = 120.0;

pub(super) fn page() -> impl View {
    parts((modes(), zoom(), themes()))
}

/// How large the panels are drawn.
fn zoom() -> AnyView {
    keyed(resource::<Zoom, _>(|zoom| zoom.at()), |at| {
        field(
            "Zoom",
            segmented_words(
                ZOOMS.iter().map(|(_, name)| *name),
                *at,
                |world, at| {
                    world.resource_mut::<Zoom>().0 = ZOOMS[at].0;
                },
            ),
        )
        .boxed()
    })
    .within(bare())
    .boxed()
}

/// The three modes, and why the panels stay as they are where the
/// theme has no variant for the mode.
fn modes() -> AnyView {
    let picked = derived(|world: &World| {
        let appearance = world.resource::<Appearance>();
        let scheme = *world.resource::<ColorScheme>();
        let missing = missing(appearance, scheme, world);
        let theme =
            world.resource::<Themes>().get(&appearance.pick().theme);
        let palette =
            |shade| theme.variants.get(shade).palette.clone();
        let palettes =
            (palette(WindowTheme::Light), palette(WindowTheme::Dark));
        (appearance.pick().mode, scheme, missing, palettes)
    })
    .when(either(
        either(moved::<Appearance>(), moved::<ColorScheme>()),
        moved::<Themes>(),
    ));
    keyed(picked, |(picked, scheme, missing, palettes)| {
        let options = [Mode::System, Mode::Light, Mode::Dark]
            .into_iter()
            .map(|mode| {
                mode_card(mode, *picked == mode, *scheme, palettes)
            })
            .collect::<Vec<_>>();
        let options = with_cx(move |cx| {
            row(options)
                .gap(cx.theme().space.gap)
                .align(AlignItems::Stretch)
                .tagged(Wraps)
        });
        let mut shown = vec![options.boxed()];
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

/// A mode as a card: how the panels look in it, over its name.
fn mode_card(
    mode: Mode,
    picked: bool,
    scheme: ColorScheme,
    (light, dark): &(Palette, Palette),
) -> AnyView {
    // System looks as the computer says now, else dark, as the panels
    // then stay.
    let shade = match (mode, scheme) {
        (Mode::Light, _)
        | (Mode::System, ColorScheme::Says(WindowTheme::Light)) => {
            light
        }
        _ => dark,
    };
    picked_card(
        format!("mode {mode:?}"),
        picked,
        column((
            row((swatch(shade),)).width(percent(100.0)),
            card_name(mode.name(), Tone::Body),
        ))
        .gap(6.0)
        .width(px(CHOICE)),
        move |world| {
            world.resource_mut::<Appearance>().pick_mode(mode);
        },
    )
}

fn shade_name(shade: WindowTheme) -> &'static str {
    match shade {
        WindowTheme::Light => "light",
        WindowTheme::Dark => "dark",
    }
}

/// A card for each theme, the one picked marked, where there is more
/// than one to pick.
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
        if themes.len() < 2 {
            return frame().boxed();
        }
        let cards = themes
            .iter()
            .map(|(picked, theme)| theme_card(theme, *picked))
            .collect::<Vec<_>>();
        field(
            "Theme",
            // Tiles of one width that wrap, as a gallery.
            with_cx(move |cx| {
                row(cards)
                    .gap(cx.theme().space.gap)
                    .align(AlignItems::Stretch)
                    .tagged(Wraps)
            }),
        )
        .boxed()
    })
    .within(bare())
    .boxed()
}

/// A theme as a tile: its name over each of its variants painted in
/// its own colours, light first, side by side.
fn theme_card(theme: &Theme, picked: bool) -> AnyView {
    let id = theme.id.clone();
    let variants = theme
        .variants
        .iter()
        .map(|(_, variant)| {
            row((swatch(&variant.palette),))
                .width(px(TILE_SWATCH))
                .boxed()
        })
        .collect::<Vec<_>>();
    picked_card(
        format!("theme {id}"),
        picked,
        column((
            card_name(theme.name.clone(), Tone::Body),
            row(variants).gap(TILE_GAP),
        ))
        .gap(8.0)
        // As wide as two variants, so that every tile is one width.
        .width(px(TILE_SWATCH * 2.0 + TILE_GAP)),
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
/// lamplight falling from its top, a card with a line of text and a
/// dim line, a primary button lit by its glow, and the colours that
/// mark what is live, refused and added.
fn swatch(palette: &Palette) -> impl View {
    let lamp = BackgroundGradient::from(RadialGradient::new(
        UiPosition::TOP,
        RadialGradientShape::Ellipse(percent(70.0), percent(65.0)),
        vec![
            ColorStop::new(
                palette.glow.with_alpha(0.22),
                percent(0.0),
            ),
            ColorStop::new(
                palette.glow.with_alpha(0.08),
                percent(45.0),
            ),
            ColorStop::new(
                palette.glow.with_alpha(0.0),
                percent(100.0),
            ),
        ],
    ));
    let lit = BoxShadow::new(
        palette.glow.with_alpha(0.55),
        px(0.0),
        px(1.0),
        px(0.0),
        px(9.0),
    );
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
            row((line(palette.gold_1, 100.0),))
                .width(px(34.0))
                .padding(UiRect::axes(px(6.0), px(5.0)))
                .radius(4.0)
                .border(1.0)
                .border_color(palette.glow.with_alpha(0.7))
                .fill(palette.primary_low)
                .tagged(lit),
            frame().grow(1.0),
            dot(palette.accent),
            dot(palette.red),
            dot(palette.green),
        ))
        .gap(5.0)
        .width(percent(100.0))
        .align(AlignItems::Center),
    ))
    .gap(8.0)
    .width(percent(100.0))
    .padding(UiRect::all(px(8.0)))
    .radius(6.0)
    .border(1.0)
    .border_color(palette.gold_5)
    .fill(palette.surface)
    .tagged(lamp)
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
