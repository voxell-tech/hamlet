//! The server's endpoints and model catalogue in Settings.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, TooltipExt, column, label,
    number_field, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Providers, Settings};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    Fills, ICON, Tipped, action, bad_action, bare, beside, close,
    description, good_action, group, link_row, list, note,
    note_rules, pill, radio, section, small_rules, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::{AnyView, View, built, with_cx};

use super::choice::home_default;
use super::{
    Asked, Draft, SHOWN, Shelf, catalogue, context_words, edit, mark,
    provider_name, provider_of, read_models, title_of,
};
use crate::roll::{either, moved};

fn remove(world: &mut World, name: &str) {
    let endpoints = world
        .resource::<Settings>()
        .0
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.name != name)
        .map(|endpoint| protocol::Endpoint {
            name: endpoint.name.clone(),
            base_url: endpoint.base_url.clone(),
        })
        .collect();
    edit(world, |id| protocol::Edit::Settings {
        id,
        set: protocol::EditSettingsSet {
            endpoints: Some(endpoints),
            model: None,
            character: None,
            effort: None,
            newcomers: None,
            reflect_every: None,
            allow_net: None,
            workspace: None,
        },
    });
}

fn endpoint(endpoint: &protocol::SettingsViewEndpoints) -> AnyView {
    let gone = endpoint.name.clone();
    close((
        beside((
            row((label(endpoint.name.clone()).bold(true),))
                .fills_width(),
            note(if endpoint.key { "Key set" } else { "No key" }),
            bad_action(Glyph::Gone, "Remove", move |world| {
                remove(world, &gone);
            }),
        )),
        note(endpoint.base_url.clone()),
    ))
    .boxed()
}

/// A provider the server holds a sign-in or a key for, by its mark
/// and name.
fn configured(id: &str, name: &str) -> AnyView {
    beside((
        row((mark(id, ICON), label(name.to_string()).bold(true)))
            .gap(8.0)
            .align(AlignItems::Center)
            .fills_width(),
        pill("Configured"),
    ))
    .boxed()
}

/// The server's endpoints, configured providers and login entry.
pub fn endpoints() -> impl View {
    let known = keyed(
        resource::<Settings, _>(|settings| {
            settings.0.endpoints.clone()
        }),
        |endpoints| {
            list(endpoints.iter().map(endpoint).collect::<Vec<_>>())
                .boxed()
        },
    )
    .within(bare());
    let providers = keyed(
        resource::<Providers, _>(|providers| {
            providers
                .0
                .iter()
                .filter(|provider| provider.configured)
                .map(|provider| {
                    (provider.id.clone(), provider.name.clone())
                })
                .collect::<Vec<_>>()
        }),
        |providers| {
            list(
                providers
                    .iter()
                    .map(|(id, name)| configured(id, name))
                    .collect::<Vec<_>>(),
            )
            .boxed()
        },
    )
    .within(bare());
    group((
        description("Credentials stay on the server."),
        known,
        super::problem(),
        providers,
        row((good_action(
            Glyph::Plus,
            "Add provider",
            crate::welcome::add,
        ),)),
        crate::root::connection_test(),
    ))
}

const PRICE_WIDE: f32 = 84.0;

fn price(name: &str, output: bool) -> AnyView {
    let (known, name) = (name.to_string(), name.to_string());
    keyed(
        resource::<Shelf, _>(move |shelf| {
            shelf
                .models
                .iter()
                .find(|model| model.name == known)
                .cloned()
        }),
        move |model| {
            let Some(model) = model else {
                return row(()).boxed();
            };
            let value = model.price.as_ref().map_or(0.0, |price| {
                if output { price.output } else { price.input }
            });
            if !matches!(
                model.priced,
                protocol::ListedPriced::Unknown
                    | protocol::ListedPriced::Yours
            ) {
                return row((label(format!("{value:.2}"))
                    .tone(Tone::Faint)
                    .rules(small_rules),))
                .width(px(PRICE_WIDE))
                .justify(JustifyContent::FlexEnd)
                .boxed();
            }
            let name = name.clone();
            number_field(value, move |world, value: f64| {
                let mut price = world
                    .resource::<Shelf>()
                    .models
                    .iter()
                    .find(|model| model.name == name)
                    .and_then(|model| model.price.clone())
                    .unwrap_or(protocol::Price {
                        input: 0.0,
                        output: 0.0,
                    });
                if output {
                    price.output = value;
                } else {
                    price.input = value;
                }
                catalogue(
                    world,
                    &name,
                    protocol::EditCatalogueSet {
                        images: None,
                        price: Some(Some(price)),
                        note: None,
                    },
                );
            })
            .range(0.0, 10_000.0)
            .step(0.05)
            .precision(2)
            .width(px(PRICE_WIDE))
            .rules(text_input)
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

const LIMIT_WIDE: f32 = 72.0;

/// The bytes in a megabyte of an image's limit.
const MEGABYTE: f64 = 1_048_576.0;

/// Sets what the model `name` takes of images: `None` leaves it to
/// the endpoint's listing, which for a custom endpoint is none.
fn save_images(
    world: &mut World,
    name: &str,
    images: Option<protocol::ImageInput>,
) {
    catalogue(
        world,
        name,
        protocol::EditCatalogueSet {
            images: Some(images),
            price: None,
            note: None,
        },
    );
}

/// The field of one limit of the model `name`, showing `value`,
/// that `set` writes into what the model takes. `range` is what it
/// may be, `step` how much a pixel of dragging changes it, and
/// `decimals` how many it is shown with.
fn limit(
    name: &str,
    title: &'static str,
    value: f64,
    range: (f64, f64),
    (step, decimals): (f64, usize),
    set: fn(&mut protocol::ImageInput, f64),
) -> AnyView {
    let name = name.to_string();
    row((
        section(title),
        number_field(value, move |world, value: f64| {
            let mut input = world
                .resource::<Shelf>()
                .models
                .iter()
                .find(|model| model.name == name)
                .and_then(|model| model.images.clone())
                .unwrap_or_default();
            set(&mut input, value);
            save_images(world, &name, Some(input));
        })
        .range(range.0, range.1)
        .step(step)
        .precision(decimals)
        .width(px(LIMIT_WIDE))
        .rules(text_input),
    ))
    .align(AlignItems::Center)
    .boxed()
}

/// A switch that says whether the model `name` takes images, and the
/// limits it takes them within once it does. A model its endpoint
/// lists as taking images keeps doing so when the switch is off.
fn images(name: &str) -> AnyView {
    let (known, name) = (name.to_string(), name.to_string());
    keyed(
        resource::<Shelf, _>(move |shelf| {
            shelf
                .models
                .iter()
                .find(|model| model.name == known)
                .map(|model| model.images.clone())
        }),
        move |model| {
            let Some(images) = model else {
                return row(()).boxed();
            };
            let switched = name.clone();
            let toggle = labelled_switch(
                "IMAGES",
                images.is_some(),
                move |world, on| {
                    let input =
                        on.then(protocol::ImageInput::default);
                    save_images(world, &switched, input);
                },
            );
            let Some(input) = images else {
                return row((toggle,)).boxed();
            };
            row((
                toggle,
                limit(
                    &name,
                    "WIDTH",
                    f64::from(
                        input
                            .max_width
                            .unwrap_or(protocol::ImageInput::WIDTH),
                    ),
                    (1.0, 16_000.0),
                    (10.0, 0),
                    |input, value| {
                        input.max_width = Some(value as u32);
                    },
                ),
                limit(
                    &name,
                    "HEIGHT",
                    f64::from(
                        input
                            .max_height
                            .unwrap_or(protocol::ImageInput::HEIGHT),
                    ),
                    (1.0, 16_000.0),
                    (10.0, 0),
                    |input, value| {
                        input.max_height = Some(value as u32);
                    },
                ),
                limit(
                    &name,
                    "MB",
                    input
                        .max_bytes
                        .unwrap_or(protocol::ImageInput::BYTES)
                        as f64
                        / MEGABYTE,
                    (0.5, 20.0),
                    (0.1, 1),
                    |input, value| {
                        input.max_bytes =
                            Some((value * MEGABYTE) as u64);
                    },
                ),
                limit(
                    &name,
                    "PER LINE",
                    f64::from(
                        input.per_line.unwrap_or(
                            protocol::ImageInput::PER_LINE,
                        ),
                    ),
                    (1.0, f64::from(protocol::ImageInput::PER_LINE)),
                    (0.2, 0),
                    |input, value| {
                        input.per_line = Some(value as u32);
                    },
                ),
            ))
            .gap(8.0)
            .align(AlignItems::Center)
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// The widths of the table's columns after the model's name: its
/// provider, its context and what stands at the row's end.
const PROVIDER_WIDE: f32 = 96.0;
/// The side of a provider's mark, and the room after it.
const MARK: f32 = 16.0;
const MARK_GAP: f32 = 8.0;
const CONTEXT_WIDE: f32 = 60.0;
const END_WIDE: f32 = 64.0;

/// The table's columns, `name` first, each as wide as its column, the
/// numbers set to its right edge and the default mark centred.
fn columns(
    name: impl View + 'static,
    provider: impl View + 'static,
    prices: (AnyView, AnyView),
    context: impl View + 'static,
    end: impl View + 'static,
) -> impl View {
    with_cx(move |cx| {
        let cell =
            |content: AnyView, wide: f32, at: JustifyContent| {
                row((content,))
                    .width(px(wide))
                    .shrink(0.0)
                    .justify(at)
                    .boxed()
            };
        let (left, right) =
            (JustifyContent::FlexStart, JustifyContent::FlexEnd);
        row((
            row((name,)).grow(1.0).min_width(px(0.0)),
            cell(provider.boxed(), PROVIDER_WIDE, left),
            cell(prices.0, PRICE_WIDE, right),
            cell(prices.1, PRICE_WIDE, right),
            cell(context.boxed(), CONTEXT_WIDE, right),
            cell(end.boxed(), END_WIDE, JustifyContent::Center),
        ))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
    })
}

/// The names over the table's columns.
fn header() -> AnyView {
    columns(
        row((section("Model"),))
            .padding(UiRect::left(px(MARK + MARK_GAP))),
        section("Provider"),
        (section("In $/M").boxed(), section("Out $/M").boxed()),
        section("Context"),
        section("Default"),
    )
    .boxed()
}

/// A model as a row of the table: its mark and name, its provider,
/// its prices, how much it holds, and the way to make it the default
/// or the mark that it is.
fn entry(model: &protocol::Listed, default: &str) -> AnyView {
    let name = model.name.clone();
    let id = model.name.clone();
    let words = row((
        mark(provider_of(&name), MARK),
        one_line(label(title_of(model))),
    ))
    .gap(MARK_GAP)
    .align(AlignItems::Center)
    .min_width(px(0.0));
    let provider = one_line(label(
        derived(move |world: &World| {
            provider_name(world, provider_of(&id))
        })
        .when(moved::<Providers>()),
    ))
    .rules(note_rules);
    let context =
        note(model.context.map_or("-".to_string(), context_words));
    let chosen = model.name.clone();
    let end = if model.name == default {
        radio(true).boxed()
    } else {
        views::button(radio(false))
            .padding(UiRect::all(px(4.0)))
            .rules(link_row)
            .tagged(Variant::Text)
            .tip("Make default")
            .on_activate(move |world| save_default(world, &chosen))
            .boxed()
    };
    let table = columns(
        words,
        provider,
        (price(&name, false), price(&name, true)),
        context,
        end,
    );
    // The image limits sit under the name, past its mark.
    column((
        table,
        row((images(&name),))
            .padding(UiRect::left(px(MARK + MARK_GAP))),
    ))
    .gap(4.0)
    .boxed()
}

/// The names holding every word of `narrow`, as the `page`th page of
/// them, the last when there are fewer, with that page's number and
/// how many pages there are.
pub(crate) fn narrowed<N: AsRef<str>>(
    names: impl Iterator<Item = N>,
    narrow: &str,
    page: usize,
) -> (Vec<String>, usize, usize) {
    let words = narrow.to_lowercase();
    let names = names
        .filter(|name| {
            let name = name.as_ref().to_lowercase();
            words.split_whitespace().all(|word| name.contains(word))
        })
        .map(|name| name.as_ref().to_string())
        .collect::<Vec<_>>();
    let pages = names.len().div_ceil(SHOWN).max(1);
    let page = page.min(pages - 1);
    let shown = names.into_iter().skip(page * SHOWN).take(SHOWN);
    (shown.collect(), page, pages)
}

/// Turns the catalogue `by` pages on, within its pages.
fn turn(world: &mut World, by: isize, pages: usize) {
    let mut draft = world.resource_mut::<Draft>();
    let page = draft.page.saturating_add_signed(by).min(pages - 1);
    draft.page = page;
}

/// Makes `model` the realm's default, which the Hand speaks with.
fn save_default(world: &mut World, model: &str) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Settings {
            id,
            set: protocol::EditSettingsSet {
                model: Some(model.into()),
                endpoints: None,
                character: None,
                effort: None,
                newcomers: None,
                reflect_every: None,
                allow_net: None,
                workspace: None,
            },
        })
    });
    world
        .resource_mut::<Shelf>()
        .requests
        .insert(id, Asked::Default);
}

pub fn shelf() -> AnyView {
    let shown = derived(|world: &World| {
        let shelf = world.resource::<Shelf>();
        let draft = world.resource::<Draft>();
        let (names, page, pages) = narrowed(
            shelf.models.iter().map(|model| &model.name),
            &draft.narrow,
            draft.page,
        );
        (
            shelf
                .models
                .iter()
                .filter(|model| names.contains(&model.name))
                .cloned()
                .collect::<Vec<_>>(),
            page,
            pages,
            home_default(world),
        )
    })
    .when(either(
        either(moved::<Shelf>(), moved::<Draft>()),
        moved::<Settings>(),
    ));
    let models = keyed(shown, |(models, page, pages, default)| {
        if models.is_empty() {
            return description(
                "No models yet. Choose List Models to ask the \
                 server.",
            )
            .boxed();
        }
        let mut rows = vec![header()];
        rows.extend(models.iter().map(|model| entry(model, default)));
        let (page, pages) = (*page, *pages);
        if pages > 1 {
            rows.push(
                beside((
                    action(Glyph::Back, "Previous", move |world| {
                        turn(world, -1, pages);
                    }),
                    note(format!("Page {} of {pages}", page + 1)),
                    action(Glyph::ToEnd, "Next", move |world| {
                        turn(world, 1, pages);
                    }),
                ))
                .boxed(),
            );
        }
        list(rows).boxed()
    })
    .within(bare());
    group((
        beside((
            action(Glyph::Restore, "Refresh", |world| {
                read_models(world, true);
            })
            .tooltip(|| {
                note("Asks every endpoint for its models again")
            }),
            row((built(|cx| {
                let narrow =
                    cx.world.resource::<Draft>().narrow.clone();
                cx.build(crate::search::search(
                    "Search models",
                    narrow,
                    |world, text| {
                        let mut draft = world.resource_mut::<Draft>();
                        if draft.narrow != text {
                            draft.narrow = text;
                            draft.page = 0;
                        }
                    },
                ))
            }),))
            .grow(1.0),
        )),
        note(resource::<Shelf, _>(|shelf| {
            if shelf.asking {
                "Asking...".into()
            } else {
                String::new()
            }
        })),
        label(resource::<Shelf, _>(|shelf| {
            shelf.problem.clone().unwrap_or_default()
        }))
        .tone(Tone::Critical),
        models,
    ))
    .boxed()
}
