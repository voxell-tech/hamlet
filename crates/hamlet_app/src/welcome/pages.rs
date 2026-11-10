//! The pages of first-run setup.

use bevy::color::{Alpha, Color, Mix};
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, BackgroundGradient, ColorStop, JustifyContent,
    RadialGradient, RadialGradientShape, UiPosition, UiRect, percent,
    px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, column, frame, label, overlay,
    row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::realm::{Providers, Settings};
use hamlet_protocol::client as protocol;
use hamlet_ui::markdown::{Face, Ink, Span, Style, Target, rich};
use hamlet_ui::styles::{
    self, CONTROL, ICON, Looks, back, bare, card_choice,
    close_button, command, description, good_action, group, hairline,
    headed, lettered, marked, note, parts, scroll_area, small_rules,
    text_input,
};
use hamlet_ui::theme::Palette;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::hint::hinted;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, built, kinds, rules, with_cx};

use super::providers::{self, Tile};
use super::{NOTHING_LOCAL, Pick, Step, Welcome, go, look, pick};
use crate::models::{
    Detail, ModelRow, legend, mark, model_row, narrowed, provider_of,
};
use crate::roll::{either, moved};

pub(super) fn said(text: impl Into<String>) -> AnyView {
    description(text.into()).boxed()
}

/// The news of the last thing asked: that it is awaited, or the
/// reason it did not work.
pub(super) fn news(waiting: &'static str) -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| {
            (welcome.asking, welcome.problem.clone())
        }),
        move |(asking, problem)| match problem {
            _ if *asking => said(waiting).boxed(),
            Some(why) if why == NOTHING_LOCAL => linked(
                NOTHING_LOCAL,
                "ollama.com",
                "https://ollama.com",
            ),
            Some(why) => label(why.clone())
                .tone(Tone::Critical)
                .rules(small_rules)
                .boxed(),
            None => frame().boxed(),
        },
    )
    .within(bare())
    .boxed()
}

/// `text` with `site` in it as a link to `url`.
pub(super) fn linked(
    text: &str,
    site: &'static str,
    url: &'static str,
) -> AnyView {
    let (before, after) = text.split_once(site).unwrap_or((text, ""));
    let span = |text: &str, link| Span {
        text: text.into(),
        style: Style::default(),
        link,
    };
    let spans = [
        span(before, None),
        span(site, Some(Target::Web(url.into()))),
        span(after, None),
    ];
    with_cx(move |cx| {
        rich(
            &spans,
            Ink {
                size: Some(cx.theme().text.small),
                tone: Tone::Body,
                face: Face::Body,
            },
        )
    })
    .boxed()
}

/// A first-run page: its name and what it is for beside the way to
/// close it, the track of the steps with the step `at` lit, `body`,
/// and the buttons of `foot` under a rule.
pub(super) fn card(
    at: usize,
    body: impl View + 'static,
    foot: Vec<AnyView>,
) -> AnyView {
    let named = keyed(
        resource::<Welcome, _>(|welcome| welcome.adding),
        |adding| {
            let (name, says) = if *adding {
                (
                    "Add provider",
                    "Add an account or a key that models come from. \
                     Your current model stays as it is.",
                )
            } else {
                (
                    "Welcome to Hamlet",
                    "Your realm needs a model to think with. Choose \
                     where it comes from.",
                )
            };
            row((
                column((styles::title(name), description(says)))
                    .gap(4.0)
                    .grow(1.0)
                    .min_width(px(0.0)),
                close_button(super::close),
            ))
            .gap(12.0)
            .align(AlignItems::FlexStart)
            .boxed()
        },
    )
    .within(bare());
    with_cx(move |cx| {
        let theme = cx.theme();
        let space = &theme.space;
        let line = theme.palette.gold_6;
        let pad = UiRect::all(px(space.overlay_padding));
        column((
            column((named, track(at)))
                .gap(space.section)
                .padding(pad),
            hairline(line),
            column((body,)).gap(space.gap).padding(pad),
            hairline(line),
            row(foot)
                .gap(8.0)
                .align(AlignItems::Center)
                .justify(JustifyContent::FlexEnd)
                .padding(UiRect::axes(
                    px(space.overlay_padding),
                    px(space.section),
                ))
                .tagged(hamlet_ui::widgets::platform::Wraps),
        ))
        .gap(0.0)
    })
    .boxed()
}

/// The steps of setup in a track, each numbered, the step `at` lit
/// and those before it checked: two while a provider is added, as
/// its models go to Settings, else three.
fn track(at: usize) -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| welcome.adding),
        move |adding| {
            let names: &'static [&'static str] = if *adding {
                &["Provider", "Sign in"]
            } else {
                &["Provider", "Sign in", "Model"]
            };
            with_cx(move |cx| {
                let theme = cx.theme();
                let space = &theme.space;
                let palette = &theme.palette;
                let steps = names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let number = index + 1;
                        let lit = number == at;
                        let tone = if lit {
                            Tone::Body
                        } else {
                            Tone::Faint
                        };
                        let inside = if number < at {
                            glyph(Glyph::Check, 11.0)
                                .tone(tone)
                                .boxed()
                        } else {
                            label(number.to_string())
                                .size(theme.text.small)
                                .bold(true)
                                .tone(tone)
                                .boxed()
                        };
                        let disc = column((inside,))
                            .width(px(22.0))
                            .height(px(22.0))
                            .shrink(0.0)
                            .radius(11.0)
                            .border(space.border)
                            .border_color(if lit {
                                palette.accent
                            } else {
                                palette.gold_5
                            })
                            .align(AlignItems::Center)
                            .justify(JustifyContent::Center);
                        row((
                            disc,
                            label(*name).tone(tone).wrap(false),
                        ))
                        .gap(8.0)
                        .align(AlignItems::Center)
                        .padding(UiRect::axes(px(12.0), px(6.0)))
                        .radius(space.round)
                        .grow(1.0)
                        .width(px(0.0))
                        .min_width(px(0.0))
                        .fill(if lit {
                            palette.selected
                        } else {
                            Color::NONE
                        })
                        .boxed()
                    })
                    .collect::<Vec<_>>();
                row(steps)
                    .gap(space.track_padding)
                    .padding(UiRect::all(px(space.track_padding)))
                    .fill(palette.input)
                    .border(space.border)
                    .border_color(palette.gold_5)
                    .radius(space.track_radius)
            })
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// The button that leaves setup: for later on first run, else a
/// cancel.
fn later() -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| welcome.adding),
        |adding| {
            let text = if *adding {
                "Cancel"
            } else {
                "I'll set this up later"
            };
            lettered(text)
                .variant(Variant::Text)
                .on_activate(super::close)
                .boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// The way back to the providers.
pub(super) fn to_providers(text: &'static str) -> AnyView {
    back(text, |world| go(world, Step::Provider)).boxed()
}

/// The side of a mark on a provider's tile.
const MARK: f32 = 18.0;

/// How tall the providers are while every one is shown, whatever
/// the search finds.
const TALL: f32 = 296.0;

/// The space between the tiles.
const TILE_GAP: f32 = 12.0;

/// A tile's face: the provider's mark, or a chip for this computer.
fn face(tile: &Tile, size: f32) -> AnyView {
    match &tile.mark {
        Some(id) => mark(id, size),
        None => glyph(Glyph::Chip, size).boxed(),
    }
}

/// A provider as a tile to pick, checked while it is the one picked.
fn tile(tile: &Tile, picked: bool) -> AnyView {
    let check = if picked {
        glyph(Glyph::Check, ICON).tone(Tone::Accent).boxed()
    } else {
        frame().boxed()
    };
    let pick = tile.pick.clone();
    views::button(
        row((
            face(tile, MARK),
            label(tile.name.clone()).bold(true).wrap(false),
            frame().grow(1.0),
            check,
        ))
        .gap(TILE_GAP)
        .align(AlignItems::Center)
        .width(percent(100.0)),
    )
    .grow(1.0)
    .width(px(0.0))
    .min_width(px(0.0))
    .rules(card_choice(picked))
    .on_activate(move |world| {
        world.resource_mut::<Welcome>().picked = Some(pick.clone());
    })
    .boxed()
}

/// The tiles shown, two to a row.
fn tiles() -> AnyView {
    keyed(
        derived(|world: &World| {
            let welcome = world.resource::<Welcome>();
            let mut tiles = providers::tiles(world);
            if !welcome.more {
                tiles.truncate(providers::FEW);
            }
            (tiles, super::picked(world))
        })
        .when(either(moved::<Welcome>(), moved::<Providers>())),
        |(tiles, picked)| {
            if tiles.is_empty() {
                return said("No providers found. Try another name.");
            }
            let rows = tiles
                .chunks(2)
                .map(|pair| {
                    let mut cells = pair
                        .iter()
                        .map(|shown| {
                            tile(
                                shown,
                                picked.as_ref() == Some(&shown.pick),
                            )
                        })
                        .collect::<Vec<_>>();
                    // A lone last tile keeps to its half.
                    if cells.len() == 1 {
                        cells.push(
                            frame().grow(1.0).width(px(0.0)).boxed(),
                        );
                    }
                    row(cells).gap(TILE_GAP).boxed()
                })
                .collect::<Vec<_>>();
            column(rows).gap(TILE_GAP).boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// The first few providers, or every one with a search over them.
fn providers_shown() -> AnyView {
    keyed(resource::<Welcome, _>(|welcome| welcome.more), |more| {
        if !*more {
            return column((
                tiles(),
                row((views::button(headed(
                    glyph(Glyph::Chevron, ICON).rotation(180.0),
                    label("Show more").wrap(false),
                ))
                // Less on the chevron's side, which has air of its
                // own.
                .padding(UiRect::new(
                    px(10.0),
                    px(13.0),
                    px(0.0),
                    px(0.0),
                ))
                .min_height(px(CONTROL))
                .variant(Variant::Text)
                .on_activate(|world| {
                    world.resource_mut::<Welcome>().more = true;
                }),)),
            ))
            .gap(8.0)
            .boxed();
        }
        // Built once, so it keeps its focus and text as the
        // tiles change.
        let search = built(|cx| {
            let seek = cx.world.resource::<Welcome>().seek.clone();
            cx.build(crate::search::search(
                "Search providers",
                seek,
                |world, text| providers::search(world, &text),
            ))
        });
        column((
            column((search,)).tagged(providers::ProviderSearch),
            column((scroll_area((tiles(),)),)).height(px(TALL)),
            row((marked(Glyph::Chevron, "Show fewer")
                .variant(Variant::Text)
                .on_activate(|world| {
                    let mut welcome = world.resource_mut::<Welcome>();
                    welcome.more = false;
                    welcome.seek.clear();
                }),)),
        ))
        .gap(8.0)
        .boxed()
    })
    .within(bare())
    .boxed()
}

/// The button that goes on with the tile picked, named for it.
fn onward() -> AnyView {
    keyed(
        derived(|world: &World| match super::picked(world) {
            Some(Pick::Local) => {
                Some((None, "Look on this computer".to_string()))
            }
            Some(Pick::Provider(id)) => world
                .resource::<Providers>()
                .0
                .iter()
                .find(|provider| provider.id == id)
                .map(|provider| {
                    let text = if provider.methods.contains(
                        &protocol::LoginMethod::Subscription,
                    ) {
                        provider.label.clone()
                    } else {
                        format!("Continue with {}", provider.name)
                    };
                    (Some(provider.id.clone()), text)
                }),
            None => None,
        })
        .when(either(moved::<Welcome>(), moved::<Providers>())),
        |onward| {
            let Some((id, text)) = onward else {
                return frame().boxed();
            };
            let face = match id {
                Some(id) => mark(id, ICON),
                None => glyph(Glyph::Chip, ICON).boxed(),
            };
            views::button(headed(
                face,
                label(text.clone()).wrap(false),
            ))
            // Less on the mark's side, which has air of its own.
            .padding(UiRect::new(px(8.0), px(12.0), px(0.0), px(0.0)))
            .min_height(px(CONTROL))
            .variant(Variant::Primary)
            .on_activate(super::proceed)
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

fn provider_page() -> AnyView {
    card(
        1,
        providers_shown(),
        vec![
            later(),
            command("Configure manually", |world| {
                go(world, Step::Manual);
            })
            .boxed(),
            onward(),
        ],
    )
}

/// The model a new hamlet is offered first: the realm's default
/// when it is listed, else the first listed.
fn recommended(models: &[String], default: &str) -> Option<String> {
    models
        .iter()
        .find(|name| *name == default)
        .or(models.first())
        .cloned()
}

/// The model recommended of those listed.
fn best(world: &World) -> Option<String> {
    let default = world
        .get_resource::<Settings>()
        .map(|settings| settings.0.model.clone())
        .unwrap_or_default();
    recommended(&world.resource::<Welcome>().models, &default)
}

/// The model Continue tries: the one picked, else the one
/// recommended.
fn chosen(world: &World) -> Option<String> {
    let welcome = world.resource::<Welcome>();
    welcome
        .chosen
        .clone()
        .filter(|name| welcome.models.contains(name))
        .or_else(|| best(world))
}

/// The provider every listed model comes from, when they share one.
fn one_provider(models: &[String]) -> Option<String> {
    let first = provider_of(models.first()?);
    models
        .iter()
        .all(|name| provider_of(name) == first)
        .then(|| first.to_string())
}

fn models() -> AnyView {
    let list = keyed(
        derived(|world: &World| {
            let welcome = world.resource::<Welcome>();
            let best = best(world);
            // The one recommended leads the list until a search.
            let mut order = welcome.models.clone();
            if welcome.narrow.trim().is_empty()
                && let Some(at) = order
                    .iter()
                    .position(|name| Some(name) == best.as_ref())
            {
                let first = order.remove(at);
                order.insert(0, first);
            }
            let (ids, _, pages) =
                narrowed(order.iter(), &welcome.narrow, 0);
            let detail = match one_provider(&welcome.models) {
                Some(_) => Detail::Id,
                None => Detail::Source,
            };
            (ids, pages, best, chosen(world), detail)
        })
        .when(either(moved::<Welcome>(), moved::<Settings>())),
        |(ids, pages, best, chosen, detail)| {
            let mut rows = ids
                .iter()
                .map(|id| {
                    let picked = id.clone();
                    model_row(
                        ModelRow {
                            name: id.clone(),
                            picked: Some(id) == chosen.as_ref(),
                            tag: (Some(id) == best.as_ref())
                                .then_some("Recommended"),
                            end: None,
                            detail: *detail,
                        },
                        move |world| {
                            world.resource_mut::<Welcome>().chosen =
                                Some(picked.clone());
                        },
                    )
                })
                .collect::<Vec<_>>();
            if ids.is_empty() {
                rows.push(said("No models found. Try another name."));
            }
            if *pages > 1 {
                rows.push(said(
                    "There are more. Type part of a name to narrow \
                     them.",
                ));
            }
            column((legend(*detail), column(rows).gap(2.0)))
                .gap(4.0)
                .boxed()
        },
    )
    .within(bare());
    let search = built(|cx| {
        let narrow = cx.world.resource::<Welcome>().narrow.clone();
        cx.build(crate::search::search(
            "Search models",
            narrow,
            |world, text| {
                let mut welcome = world.resource_mut::<Welcome>();
                if welcome.narrow != text {
                    welcome.narrow = text;
                }
            },
        ))
    });
    // The provider's mark leads what it listed, once for every row.
    let from = keyed(
        resource::<Welcome, _>(|welcome| {
            (welcome.from.clone(), one_provider(&welcome.models))
        }),
        |(from, provider)| {
            let says = description(format!(
                "{from} listed these models. Pick the one your \
                 realm will think with."
            ));
            match provider {
                Some(id) => row((mark(id, ICON), says))
                    .gap(8.0)
                    .align(AlignItems::Center)
                    .boxed(),
                None => says.boxed(),
            }
        },
    )
    .within(bare());
    card(
        3,
        parts((
            from,
            group((search, news("Asking it to say hello..."), list)),
            note(
                "You can change it later in Settings, under Models, \
                 and for one agent from the model under its chat.",
            ),
        )),
        vec![
            to_providers("Back"),
            good_action(Glyph::Check, "Continue", |world| {
                if let Some(model) = chosen(world) {
                    pick(world, &model);
                }
            }),
        ],
    )
}

fn local() -> AnyView {
    card(
        2,
        parts((
            said(
                "Hamlet looks for Ollama or LM Studio on this \
                 computer.",
            ),
            news("Looking..."),
        )),
        vec![
            to_providers("Back"),
            good_action(Glyph::Restore, "Look again", look),
        ],
    )
}

/// An endpoint typed in by hand: its name and the address it
/// answers at.
fn manual() -> AnyView {
    let field = |read: fn(&Welcome) -> String,
                 write: fn(&mut World, String),
                 hint: &'static str| {
        hinted(
            text_field(
                resource::<Welcome, _>(read),
                move |world: &mut World, text| write(world, text),
            )
            .rules(text_input),
            hint,
        )
    };
    card(
        2,
        parts((
            said(
                "A model server that answers as the OpenAI API \
                 does, on this computer or another.",
            ),
            group((
                field(
                    |welcome| welcome.endpoint.0.clone(),
                    super::name_endpoint,
                    "Name, such as studio",
                ),
                field(
                    |welcome| welcome.endpoint.1.clone(),
                    super::address_endpoint,
                    "Address, such as http://localhost:1234/v1",
                ),
            )),
            news("Adding it..."),
        )),
        vec![
            to_providers("Back"),
            good_action(Glyph::Plus, "Add endpoint", super::commit),
        ],
    )
}

pub fn view() -> impl View {
    built(|cx| {
        cx.world.init_resource::<providers::Login>();
        cx.world.init_resource::<super::Browser>();
        cx.world.init_resource::<super::Committing>();
        cx.build(screen())
    })
}

fn screen() -> AnyView {
    keyed(
        resource::<Welcome, _>(|welcome| {
            (
                welcome.open,
                welcome.step,
                welcome.adding,
                welcome.connection.clone(),
            )
        }),
        |(open, step, adding, connection)| {
            let page = if let Some(words) = connection {
                let words = words.clone();
                with_cx(move |cx| {
                    column((parts((
                        headed(
                            glyph(Glyph::Signal, ICON),
                            styles::title("Connect to a server"),
                        ),
                        said(words.clone()),
                        said(
                            "Start a server with hamlet server, \
                             then open its window with hamlet \
                             window. For local development, use \
                             hamlet dev.",
                        ),
                    )),))
                    .padding(UiRect::all(px(cx
                        .theme()
                        .space
                        .overlay_padding)))
                })
                .boxed()
            } else if !open {
                return frame().boxed();
            } else {
                match step {
                    Step::Provider => provider_page(),
                    Step::Key => providers::key_page(),
                    Step::SignIn => providers::sign_in_page(),
                    Step::Local => local(),
                    Step::Manual => manual(),
                    Step::Models => models(),
                }
            };
            // Adding a provider over an open desk dims the desk;
            // first run and the server question have no desk to
            // show.
            let dims = *adding && connection.is_none();
            with_cx(move |cx| over(cx, page, dims)).boxed()
        },
    )
    .within(overlay(()))
    .boxed()
}

/// How far the warmth at the middle of the first-run ground goes
/// from the bottom glow toward the lamp's glow.
const WARMTH: f32 = 0.14;

/// How much of the desk a dimming scrim hides.
const DIM: f32 = 0.85;

/// How strong the lamp at the foot of a card over a dimmed desk is.
const LAMP: f32 = 0.16;

/// Lamplight rising from the foot of a card, as it rises from a
/// lit tab's.
fn lamp(palette: &Palette) -> BackgroundGradient {
    let glow = |strength: f32, at: f32| {
        ColorStop::new(
            palette.glow.with_alpha(LAMP * strength),
            percent(at),
        )
    };
    BackgroundGradient::from(RadialGradient::new(
        UiPosition::BOTTOM,
        RadialGradientShape::Ellipse(percent(75.0), percent(55.0)),
        vec![
            glow(1.0, 0.0),
            glow(0.5, 30.0),
            glow(0.15, 65.0),
            glow(0.0, 100.0),
        ],
    ))
}

/// The card over its ground: the window's own colour, opaque, or with
/// `dims` the desk behind it, dimmed.
fn over(cx: &Cx, page: AnyView, dims: bool) -> impl View + use<> {
    let theme = cx.theme();
    let space = &theme.space;
    // Opaque by default: colours mix in linear light, so a scrim
    // thin enough to show the realm leaves its words readable behind
    // the card. Dimmed, the desk is meant to be seen, not read.
    let scrim = match dims {
        true => theme.palette.shadow.with_alpha(DIM),
        false => theme.background(),
    };
    // The page pads its own parts, so its rules run edge to edge.
    let card =
        kinds::card((scroll_area((column((page,)).gap(0.0),)),))
            .width(percent(100.0))
            .max_height(percent(100.0))
            .padding(UiRect::all(px(0.0)))
            .gap(0.0)
            .tagged(theme.shadow())
            .tagged(match dims {
                true => lamp(&theme.palette),
                false => BackgroundGradient(Vec::new()),
            })
            .rules(rules::floating);
    let hung = column((card,))
        .width(percent(100.0))
        .max_width(px(space.overlay_width))
        .max_height(percent(100.0))
        .gap(0.0);
    // First run pools a lamp's warmth behind the card, fading to the
    // window's own colour at its edges. Dimmed, the card carries its
    // own lamp instead.
    let palette = &theme.palette;
    let ground = match dims {
        true => BackgroundGradient(Vec::new()),
        false => BackgroundGradient::from(RadialGradient::new(
            UiPosition::CENTER,
            RadialGradientShape::Ellipse(
                percent(60.0),
                percent(70.0),
            ),
            vec![
                ColorStop::new(
                    palette.bottom_glow.mix(&palette.glow, WARMTH),
                    percent(0.0),
                ),
                ColorStop::new(palette.bottom_glow, percent(45.0)),
                ColorStop::new(scrim, percent(100.0)),
            ],
        )),
    };
    row((hung,))
        .width(percent(100.0))
        .height(percent(100.0))
        .padding(UiRect::all(px(space.panel)))
        .align(AlignItems::Center)
        .justify(JustifyContent::Center)
        .fill(scrim)
        .tagged(ground)
}
