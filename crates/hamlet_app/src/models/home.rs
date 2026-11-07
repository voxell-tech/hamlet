//! The server's endpoints and model catalogue in Settings.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, frame, label, number_field, row,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Providers, Settings};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    Fills, action, bad_action, bare, beside, button, close,
    description, good_action, group, lettered, list, note,
    note_rules, pill, section, small_rules, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use super::choice::home_default;
use super::{
    Asked, Draft, SHOWN, Shelf, catalogue, edit, mark, model_name,
    provider_name, provider_of, read_models,
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
                .map(|provider| provider.label.clone())
                .collect::<Vec<_>>()
        }),
        |providers| {
            close(
                providers
                    .iter()
                    .cloned()
                    .map(note)
                    .collect::<Vec<_>>(),
            )
            .boxed()
        },
    )
    .within(bare());
    group((
        description(
            "Models run through the server. A model is named \
             endpoint::model. Credentials stay on the server.",
        ),
        known,
        super::problem(),
        providers,
        row((good_action(
            Glyph::Plus,
            "Add a place for models",
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

/// The widths of the table's columns after the model's name: its
/// provider, its context and what stands at the row's end.
const PROVIDER_WIDE: f32 = 96.0;
/// The side of a provider's mark, and the room after it.
const MARK: f32 = 16.0;
const MARK_GAP: f32 = 8.0;
const CONTEXT_WIDE: f32 = 60.0;
const END_WIDE: f32 = 112.0;

/// The table's columns, `name` first, each as wide as its column.
fn columns(
    name: impl View + 'static,
    provider: impl View + 'static,
    prices: (AnyView, AnyView),
    context: impl View + 'static,
    end: impl View + 'static,
) -> impl View {
    with_cx(move |cx| {
        let cell = |content: AnyView, wide: f32| {
            row((content,)).width(px(wide)).shrink(0.0).boxed()
        };
        row((
            row((name,)).grow(1.0).min_width(px(0.0)),
            cell(provider.boxed(), PROVIDER_WIDE),
            cell(prices.0, PRICE_WIDE),
            cell(prices.1, PRICE_WIDE),
            cell(context.boxed(), CONTEXT_WIDE),
            cell(end.boxed(), END_WIDE),
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
        frame(),
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
        one_line(label(model_name(&name).to_string())),
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
        note(model.context.map_or("-".to_string(), |context| {
            format!("{}k", context / 1000)
        }));
    let chosen = model.name.clone();
    let end = if model.name == default {
        pill("Default").boxed()
    } else {
        lettered("Make default")
            .rules(button)
            .tagged(Variant::Default)
            .on_activate(move |world| save_default(world, &chosen))
            .boxed()
    };
    columns(
        words,
        provider,
        (price(&name, false), price(&name, true)),
        context,
        end,
    )
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
        description(
            "The default model is used wherever nobody chose one. \
             Prices are dollars per million tokens in and out, used \
             when the endpoint does not bill a call.",
        ),
        beside((
            action(Glyph::Restore, "List models", |world| {
                read_models(world, true);
            }),
            section("Find"),
            text_field(
                resource::<Draft, _>(|draft| draft.narrow.clone()),
                |world, text| {
                    let mut draft = world.resource_mut::<Draft>();
                    draft.narrow = text;
                    draft.page = 0;
                },
            )
            .grow(1.0)
            .rules(text_input),
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
