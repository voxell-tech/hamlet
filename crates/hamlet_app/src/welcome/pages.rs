//! The pages of first-run setup.

use bevy::color::Alpha;
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, JustifyContent, Overflow, UiRect, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, BehaviorExt, Frame, FrameProps, Stack, column, frame,
    label, overlay, row, scroll, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, keyed, resource, style,
};
use hamlet_client::realm::{Providers, Settings};
use hamlet_protocol::client as protocol;
use hamlet_ui::markdown::{Face, Ink, Span, Style, Target, rich};
use hamlet_ui::styles::{
    self, CONTROL, ICON, Looks, back, bare, close_button, command,
    description, good_action, group, hairline, headed, lettered,
    marked, note, parts, scroll_area, section, small_rules,
    text_input,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::edges::faded;
use hamlet_ui::widgets::effects::{Edge, Variant, Wash};
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
        // Only the body scrolls, and only up and down: the name and
        // the buttons stay, and nothing passes the card's corners.
        let body = faded(
            scroll((column((body,))
                .gap(space.gap)
                .padding(pad)
                .width(percent(100.0))
                .min_width(px(0.0)),))
            .overflow(Overflow::scroll_y()),
        );
        column((
            column((named, track(at)))
                .gap(space.section)
                .padding(pad)
                .shrink(0.0),
            hairline(line).shrink(0.0),
            body,
            hairline(line).shrink(0.0),
            // Its buttons stretch to its height, so each stands
            // taller than one in a panel.
            row(foot)
                .gap(8.0)
                .align(AlignItems::Stretch)
                .min_height(px(FOOT + 2.0 * space.section))
                .justify(JustifyContent::FlexEnd)
                .padding(UiRect::axes(
                    px(space.overlay_padding),
                    px(space.section),
                ))
                .shrink(0.0)
                .tagged(hamlet_ui::widgets::platform::Wraps),
        ))
        .gap(0.0)
        // It may be shorter than all it holds, so the body scrolls.
        .min_height(px(0.0))
    })
    .boxed()
}

/// The side of a step's disc in the track.
const DISC: f32 = 24.0;

/// The steps of setup as a trail, each a numbered waypoint joined to
/// the next by a path: the step `at` lit in the accent, those before
/// it checked and the path walked so far gold. Two while a provider
/// is added, as its models go to Settings, else three.
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
                let mut trail = Vec::new();
                for (index, name) in names.iter().enumerate() {
                    let number = index + 1;
                    if index > 0 {
                        let walked = number <= at;
                        trail.push(
                            frame()
                                .grow(1.0)
                                .min_width(px(24.0))
                                .height(px(space.border))
                                .fill(if walked {
                                    palette.accent
                                } else {
                                    palette.gold_6
                                })
                                .boxed(),
                        );
                    }
                    let (lit, done) = (number == at, number < at);
                    let inside = if done {
                        glyph(Glyph::Check, 12.0)
                            .tone(Tone::Accent)
                            .boxed()
                    } else {
                        label(number.to_string())
                            .size(theme.text.small)
                            .bold(lit)
                            .tone(if lit {
                                Tone::OnAccent
                            } else {
                                Tone::Faint
                            })
                            .boxed()
                    };
                    let disc = column((inside,))
                        .width(px(DISC))
                        .height(px(DISC))
                        .shrink(0.0)
                        .radius(DISC / 2.0)
                        .fill(if lit {
                            palette.accent
                        } else {
                            palette.button
                        })
                        .border(space.border)
                        .border_color(if lit || done {
                            palette.accent
                        } else {
                            palette.gold_6
                        })
                        .align(AlignItems::Center)
                        .justify(JustifyContent::Center);
                    trail.push(
                        row((
                            disc,
                            label(*name)
                                .tone(if lit {
                                    Tone::Body
                                } else {
                                    Tone::Faint
                                })
                                .wrap(false),
                        ))
                        .gap(8.0)
                        .align(AlignItems::Center)
                        .shrink(0.0)
                        .boxed(),
                    );
                }
                row(trail).gap(12.0).align(AlignItems::Center)
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
    .within(footed())
    .boxed()
}

/// What a keyed button at a page's foot is built under: a row, which
/// stretches it to the foot's height as the foot stretches the row.
pub(super) fn footed() -> Stack<()> {
    row(()).gap(0.0)
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

/// How tall a tile is at least.
const TILE_TALL: f32 = 52.0;

/// How tall the buttons at a page's foot are.
const FOOT: f32 = 36.0;

/// The side of the disc that checks the picked tile.
const CHECKED: f32 = 20.0;

/// How a provider's tile stands over a default button's look: a
/// card's corner, its room left to the row inside, and while it is
/// `picked`, ringed in the accent with its words lit. The frame keeps
/// its width, so a tile keeps its size.
fn tile_rules(picked: bool) -> impl FnOnce(&mut Cx) + Send + Sync {
    move |cx| {
        let mut rules = style::<HamletTheme>().frame(move |f, t| {
            f.radius(t.space.round_card).padding(UiRect::all(px(0.0)))
        });
        if picked {
            let ringed = |f: Frame, t: &HamletTheme| {
                f.border_color(t.palette.accent)
            };
            rules = rules
                .frame(ringed)
                .label(|l, _| l.tone(Tone::Body))
                .icon(|i, _| i.tone(Tone::Body))
                .hovered(|s| s.frame(ringed))
                .pressed(|s| s.frame(ringed));
        }
        rules.apply(cx);
    }
}

/// A provider as a tile to pick, checked while it is the one picked.
fn tile(tile: &Tile, picked: bool) -> AnyView {
    let check = if picked {
        with_cx(|cx| {
            column((glyph(Glyph::Check, 12.0).tone(Tone::OnAccent),))
                .width(px(CHECKED))
                .height(px(CHECKED))
                .radius(CHECKED / 2.0)
                .fill(cx.theme().palette.accent)
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
        })
        .boxed()
    } else {
        frame().height(px(CHECKED)).boxed()
    };
    let pick = tile.pick.clone();
    let face = face(tile, MARK);
    let name = tile.name.clone();
    with_cx(move |cx| {
        let space = &cx.theme().space;
        // Washed from the left while picked, and less under the
        // pointer, as a row of the Hub's rail is. The wash is on the
        // row inside, as the button's own gradient is its glow.
        let words = row((
            face,
            label(name).wrap(false),
            frame().grow(1.0),
            check,
        ))
        .gap(TILE_GAP)
        .align(AlignItems::Center)
        .width(percent(100.0))
        .min_height(px(TILE_TALL))
        .padding(UiRect::axes(
            px(space.card_padding_x),
            px(space.card_padding_y),
        ))
        .radius(space.round_card)
        .tagged(Wash::new(picked, Edge::Left));
        views::button(words)
            .grow(1.0)
            .width(px(0.0))
            .min_width(px(0.0))
            .variant(Variant::Default)
            .rules(tile_rules(picked))
            .on_activate(move |world| {
                world.resource_mut::<Welcome>().picked =
                    Some(pick.clone());
            })
    })
    .boxed()
}

/// `tiles` two to a row, the one `picked` checked.
fn grid(tiles: &[Tile], picked: Option<&Pick>) -> AnyView {
    let rows = tiles
        .chunks(2)
        .map(|pair| {
            let mut cells = pair
                .iter()
                .map(|shown| tile(shown, picked == Some(&shown.pick)))
                .collect::<Vec<_>>();
            // A lone last tile keeps to its half.
            if cells.len() == 1 {
                cells.push(frame().grow(1.0).width(px(0.0)).boxed());
            }
            row(cells).gap(TILE_GAP).boxed()
        })
        .collect::<Vec<_>>();
    column(rows).gap(TILE_GAP).boxed()
}

/// The tiles shown, grouped by the way to each.
fn tiles() -> AnyView {
    keyed(
        derived(|world: &World| {
            let welcome = world.resource::<Welcome>();
            let mut tiles = providers::tiles(world);
            if !welcome.more {
                tiles.truncate(providers::FEW);
            }
            let seeking = !welcome.seek.trim().is_empty();
            (tiles, super::picked(world), seeking)
        })
        .when(either(moved::<Welcome>(), moved::<Providers>())),
        |(tiles, picked, seeking)| {
            if tiles.is_empty() {
                return said("No providers found. Try another name.");
            }
            let picked = picked.as_ref();
            // A search ranks the tiles, so they stand as one group.
            if *seeking {
                return grid(tiles, picked);
            }
            let groups = tiles
                .chunk_by(|one, next| one.way == next.way)
                .map(|group| {
                    column((
                        section(group[0].way.heading()),
                        grid(group, picked),
                    ))
                    .gap(8.0)
                    .boxed()
                })
                .collect::<Vec<_>>();
            column(groups).gap(16.0).boxed()
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
                row((views::button(
                    row((
                        label("Show more")
                            .tone(Tone::Faint)
                            .wrap(false),
                        glyph(Glyph::Chevron, 12.0)
                            .rotation(180.0)
                            .tone(Tone::Faint),
                    ))
                    .gap(6.0)
                    .align(AlignItems::Center),
                )
                .padding(UiRect::axes(px(8.0), px(0.0)))
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
    .within(footed())
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
            (welcome.open, welcome.step, welcome.connection.clone())
        }),
        |(open, step, connection)| {
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
            with_cx(move |cx| over(cx, page)).boxed()
        },
    )
    .within(overlay(()))
    .boxed()
}

/// How much of what is behind the card its veil hides: the realm on
/// first run, a desk while a provider is added.
const VEIL: f32 = 0.7;

/// The card over a veil on whatever is behind.
fn over(cx: &Cx, page: AnyView) -> impl View + use<> {
    let theme = cx.theme();
    let space = &theme.space;
    // The page pads its own parts, so its rules run edge to edge,
    // and scrolls its own body.
    let card = kinds::card((page,))
        .width(percent(100.0))
        .max_height(percent(100.0))
        .padding(UiRect::all(px(0.0)))
        .gap(0.0)
        .tagged(theme.shadow())
        .rules(rules::floating);
    let hung = column((card,))
        .width(percent(100.0))
        .max_width(px(space.overlay_width))
        .max_height(percent(100.0))
        .gap(0.0);
    row((hung,))
        .width(percent(100.0))
        .height(percent(100.0))
        .padding(UiRect::all(px(space.panel)))
        .align(AlignItems::Center)
        .justify(JustifyContent::Center)
        .fill(theme.palette.shadow.with_alpha(VEIL))
}
