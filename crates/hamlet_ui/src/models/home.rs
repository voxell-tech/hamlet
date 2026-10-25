//! The server's endpoints and model catalogue in Settings.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps, label, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Providers, Settings};
use hamlet_protocol::client as protocol;

use super::choice::{chooser, home_default};
use super::{
    Asked, Draft, SHOWN, Shelf, catalogue, edit, read_models,
};
use crate::icons::Glyph;
use crate::looks::{
    Fills, action, bad_action, bare, beside, close, description,
    good_action, group, list, note, section, well,
};
use crate::roll::{either, moved};
use crate::switch::labelled_switch;
use crate::{AnyView, View, with_cx};

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
            bad_action(Glyph::Gone, "REMOVE", move |world| {
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
        providers,
        row((good_action(
            Glyph::Plus,
            "ADD A PLACE FOR MODELS",
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

fn entry(model: &protocol::Listed) -> AnyView {
    let name = model.name.clone();
    let allow = name.clone();
    let describe = name.clone();
    let source = match model.priced {
        protocol::ListedPriced::Unknown => "no price",
        protocol::ListedPriced::Listed => "listed",
        protocol::ListedPriced::Assigned => "assigned",
        protocol::ListedPriced::Yours => "yours",
    };
    let detail = model.context.map_or(source.into(), |context| {
        format!("{source}, holds {}k", context / 1000)
    });
    let prices = with_cx(move |cx| {
        row((
            section("IN"),
            price(&name, false),
            section("OUT"),
            price(&name, true),
            note(detail),
        ))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
    });
    close((
        labelled_switch(
            model.name.clone(),
            model.allowed,
            move |world, allowed| {
                catalogue(
                    world,
                    &allow,
                    protocol::EditCatalogueSet {
                        allowed: Some(allowed),
                        price: None,
                        note: None,
                    },
                );
            },
        ),
        prices,
        text_field(model.note.clone(), move |world, note| {
            catalogue(
                world,
                &describe,
                protocol::EditCatalogueSet {
                    note: Some(note),
                    allowed: None,
                    price: None,
                },
            );
        })
        .rules(well),
    ))
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
        protocol::ClientMessage::Edit(protocol::Edit::Settings {
            id,
            set: protocol::EditSettingsSet {
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
            return description(
                "No models yet. Choose List Models to ask the \
                 server.",
            )
            .boxed();
        }
        let mut rows = models.iter().map(entry).collect::<Vec<_>>();
        if *more > 0 {
            rows.push(
                description(format!(
                    "{more} more. Type part of a name to narrow \
                     them."
                ))
                .boxed(),
            );
        }
        list(rows).boxed()
    })
    .within(bare());
    group((
        close((
            section("DEFAULT MODEL"),
            chooser(home_default, save_default),
            description(
                "Used wherever nobody chose a model. It is always \
                 allowed.",
            ),
        )),
        description(
            "Switch a model on to allow it. Prices are dollars per \
             million tokens in and out, used when the endpoint does \
             not bill a call.",
        ),
        beside((
            action(Glyph::Restore, "LIST MODELS", |world| {
                read_models(world, true);
            }),
            section("FIND"),
            text_field(
                resource::<Draft, _>(|draft| draft.narrow.clone()),
                |world, text| {
                    world.resource_mut::<Draft>().narrow = text;
                },
            )
            .grow(1.0)
            .rules(well),
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
