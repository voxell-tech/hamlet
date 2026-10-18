//! The server's endpoints and model catalogue in Settings.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps, column, label, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Providers, Settings};
use hamlet_protocol::v0;

use super::choice::{chooser, home_default};
use super::{
    Asked, Draft, SHOWN, Shelf, catalogue, edit, read_models,
};
use crate::icons::Glyph;
use crate::looks::{
    CONTROL, Fills, action, bad_action, bare, good_action, note,
    tick, well,
};
use crate::roll::{either, moved};
use crate::{AnyView, View};

fn small(text: &'static str) -> impl View {
    label(text).size(11.0).tone(Tone::Faint)
}

fn remove(world: &mut World, name: &str) {
    let endpoints = world
        .resource::<Settings>()
        .0
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.name != name)
        .map(|endpoint| v0::Endpoint {
            name: endpoint.name.clone(),
            base_url: endpoint.base_url.clone(),
        })
        .collect();
    edit(world, |id| v0::Edit::Settings {
        id,
        set: v0::EditSettingsSet {
            endpoints: Some(endpoints),
            model: None,
            reflect_every: None,
            allow_net: None,
            workspace: None,
        },
    });
}

fn endpoint(endpoint: &v0::SettingsViewEndpoints) -> AnyView {
    let gone = endpoint.name.clone();
    column((
        row((
            row((label(endpoint.name.clone()).bold(true),))
                .fills_width(),
            note(if endpoint.key { "Key set" } else { "No key" }),
            bad_action(Glyph::Gone, "REMOVE", move |world| {
                remove(world, &gone)
            }),
        ))
        .gap(12.0)
        .align(AlignItems::Center),
        note(endpoint.base_url.clone()),
    ))
    .gap(2.0)
    .boxed()
}

/// The server's endpoints, configured providers and login entry.
pub fn endpoints() -> impl View {
    let known = keyed(
        resource::<Settings, _>(|settings| {
            settings.0.endpoints.clone()
        }),
        |endpoints| {
            column(endpoints.iter().map(endpoint).collect::<Vec<_>>())
                .gap(6.0)
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
            column(
                providers
                    .iter()
                    .cloned()
                    .map(note)
                    .collect::<Vec<_>>(),
            )
            .gap(4.0)
            .boxed()
        },
    )
    .within(bare());
    column((
        note(
            "Models run through the server. A model is named \
             endpoint::model. Credentials stay on the server.",
        ),
        known,
        providers,
        row((good_action(
            Glyph::Plus,
            "ADD A PLACE FOR MODELS",
            crate::welcome::add,
        ),)),
        crate::root::connection_test(),
    ))
    .gap(8.0)
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
                v0::ListedPriced::Unknown | v0::ListedPriced::Yours
            ) {
                return row((label(format!("{value:.2}"))
                    .size(12.0)
                    .tone(Tone::Faint),))
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
                    .unwrap_or(v0::Price {
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
                    v0::EditCatalogueSet {
                        price: Some(Some(price)),
                        allowed: None,
                        note: None,
                    },
                );
            })
            .range(0.0, 10_000.0)
            .step(0.05)
            .precision(2)
            .width(px(PRICE_WIDE))
            .rules(well)
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

fn entry(model: &v0::Listed) -> AnyView {
    let name = model.name.clone();
    let allow = name.clone();
    let describe = name.clone();
    let source = match model.priced {
        v0::ListedPriced::Unknown => "no price",
        v0::ListedPriced::Listed => "listed",
        v0::ListedPriced::Assigned => "assigned",
        v0::ListedPriced::Yours => "yours",
    };
    let detail = model.context.map_or(source.into(), |context| {
        format!("{source}, holds {}k", context / 1000)
    });
    column((
        row((
            tick(model.allowed).on_change(move |world, allowed| {
                catalogue(
                    world,
                    &allow,
                    v0::EditCatalogueSet {
                        allowed: Some(allowed),
                        price: None,
                        note: None,
                    },
                )
            }),
            row((label(name.clone()),)).fills_width(),
        ))
        .gap(10.0)
        .align(AlignItems::Center),
        row((
            row(()).width(px(CONTROL)).shrink(0.0),
            small("IN"),
            price(&name, false),
            small("OUT"),
            price(&name, true),
            label(detail).size(11.0).tone(Tone::Faint),
        ))
        .gap(8.0)
        .align(AlignItems::Center),
        text_field(model.note.clone(), move |world, note| {
            catalogue(
                world,
                &describe,
                v0::EditCatalogueSet {
                    note: Some(note),
                    allowed: None,
                    price: None,
                },
            )
        })
        .rules(well),
    ))
    .gap(4.0)
    .boxed()
}

pub(crate) fn narrowed<N: AsRef<str>>(
    names: impl Iterator<Item = N>,
    narrow: &str,
) -> (Vec<String>, usize) {
    let words = narrow.to_lowercase();
    let mut names = names
        .filter(|name| {
            let name = name.as_ref().to_lowercase();
            words.split_whitespace().all(|word| name.contains(word))
        })
        .map(|name| name.as_ref().to_string())
        .collect::<Vec<_>>();
    let more = names.len().saturating_sub(SHOWN);
    names.truncate(SHOWN);
    (names, more)
}

fn save_default(world: &mut World, model: &str) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        v0::ClientMessage::Edit(v0::Edit::Settings {
            id,
            set: v0::EditSettingsSet {
                model: Some(model.into()),
                endpoints: None,
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
    let shown = bevy_fynix::derived(|world: &World| {
        let shelf = world.resource::<Shelf>();
        let (names, more) = narrowed(
            shelf.models.iter().map(|model| &model.name),
            &world.resource::<Draft>().narrow,
        );
        (
            shelf
                .models
                .iter()
                .filter(|model| names.contains(&model.name))
                .cloned()
                .collect::<Vec<_>>(),
            more,
        )
    })
    .when(either(moved::<Shelf>(), moved::<Draft>()));
    let models = keyed(shown, |(models, more)| {
        if models.is_empty() {
            return note(
                "No models yet. Choose List Models to ask the \
                 server.",
            )
            .boxed();
        }
        let mut rows = models.iter().map(entry).collect::<Vec<_>>();
        if *more > 0 {
            rows.push(
                note(format!(
                    "{more} more. Type part of a name to narrow \
                     them."
                ))
                .boxed(),
            );
        }
        column(rows).gap(10.0).boxed()
    })
    .within(bare());
    column((
        column((
            small("DEFAULT MODEL"),
            chooser(home_default, save_default),
            note(
                "Used wherever nobody chose a model. It is always \
                 allowed.",
            ),
        ))
        .gap(4.0),
        note(
            "Tick a model to allow it. Prices are dollars per \
             million tokens in and out, used when the endpoint does \
             not bill a call.",
        ),
        row((
            action(Glyph::Restore, "LIST MODELS", |world| {
                read_models(world, true)
            }),
            small("FIND"),
            text_field(
                resource::<Draft, _>(|draft| draft.narrow.clone()),
                |world, text| {
                    world.resource_mut::<Draft>().narrow = text
                },
            )
            .grow(1.0)
            .rules(well),
        ))
        .gap(12.0)
        .align(AlignItems::Center),
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
        .size(12.0)
        .tone(Tone::Critical),
        models,
    ))
    .gap(8.0)
    .boxed()
}
