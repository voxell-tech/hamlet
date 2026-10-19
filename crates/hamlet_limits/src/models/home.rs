//! The endpoints and the catalogue of models, as pages of the
//! settings.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps, column, label, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{
    HomeDir, ListModels, Listing, SaveSettings, Settings,
    TestConnection, Tokio,
};
use hamlet_model::catalogue::{Model, Priced};
use hamlet_model::chatgpt;
use hamlet_model::config::Config;
use hamlet_sdk::checks::{either, moved};
use hamlet_sdk::settings::AddEndpoint;

use super::choice::{chooser, home_default};
use super::{Draft, SHOWN, Shelf};
use crate::icons::Glyph;
use crate::looks::{
    CONTROL, Fills, action, bad_action, bare, good_action, note,
    tick, well,
};
use crate::{AnyView, View};

/// Changes one model of the catalogue and writes it.
fn change(
    world: &mut World,
    name: &str,
    act: impl FnOnce(&mut Model),
) {
    let home = world.resource::<HomeDir>().0.root.clone();
    let mut models = world.resource_mut::<ModelCatalogue>();
    let Some(model) = models.catalogue.get_mut(name) else {
        return;
    };
    act(model);
    let saved = models.catalogue.save(&home);
    models.reread();
    world.resource_mut::<Shelf>().problem =
        saved.err().map(|e| format!("{e:#}"));
}

fn remove(world: &mut World, name: &str) {
    let home = world.resource::<HomeDir>().0.root.clone();
    let mut config = world.resource::<Settings>().config.clone();
    let signed_in = config.endpoint(name).is_some_and(|e| e.chatgpt);
    config.endpoints.retain(|endpoint| endpoint.name != name);
    save(world, config);
    Config::forget_key(&home, name);
    if signed_in {
        let tokio = world.resource::<Tokio>().0.clone();
        // Signed out here even when OpenAI cannot be told.
        tokio.spawn(async move {
            let _ = chatgpt::sign_out(&home).await;
        });
    }
}

fn save(world: &mut World, config: Config) {
    world.trigger(SaveSettings(config));
    let failed = world.resource::<Settings>().error.clone();
    world.resource_mut::<Shelf>().problem = failed;
}

fn small(text: &'static str) -> impl View {
    label(text).size(11.0).tone(Tone::Faint)
}

/// The endpoints of the settings, and a row to add one.
pub fn endpoints() -> impl View {
    let known = keyed(
        resource::<Settings, _>(|settings| {
            settings
                .config
                .endpoints
                .iter()
                .map(|e| {
                    (
                        e.name.clone(),
                        e.base_url.clone(),
                        e.api_key.is_some(),
                        e.chatgpt,
                    )
                })
                .collect::<Vec<_>>()
        }),
        |endpoints| {
            let rows = endpoints
                .iter()
                .map(|(name, base_url, keyed, plan)| {
                    let gone = name.clone();
                    let key = match (*plan, *keyed) {
                        (true, _) => "Using ChatGPT plan",
                        (false, true) => "Key set",
                        (false, false) => "No key",
                    };
                    let mut parts = vec![
                        row((label(name.clone()).bold(true),))
                            .fills_width()
                            .boxed(),
                        note(key).boxed(),
                    ];
                    if *plan {
                        parts.push(action(
                            Glyph::Meter,
                            "MANAGE USAGE",
                            |_| {
                                hamlet_desktop::open::browse(
                                    chatgpt::USAGE,
                                );
                            },
                        ));
                    }
                    parts.push(bad_action(
                        Glyph::Gone,
                        "REMOVE",
                        move |world| remove(world, &gone),
                    ));
                    column((
                        row(parts)
                            .gap(12.0)
                            .align(AlignItems::Center),
                        note(base_url.clone()),
                    ))
                    .gap(2.0)
                    .boxed()
                })
                .collect::<Vec<_>>();
            column(rows).gap(6.0).boxed()
        },
    )
    .within(bare());
    column((
        note(
            "Any server that speaks the OpenAI chat protocol works. \
             A model is named `endpoint::model`.",
        ),
        known,
        row((good_action(
            Glyph::Plus,
            "ADD A PLACE FOR MODELS",
            |world| {
                world.trigger(AddEndpoint);
                world.flush();
            },
        ),))
        .boxed(),
    ))
    .gap(8.0)
}

/// The answers of the endpoints when they were last asked.
fn said(listing: &Listing) -> String {
    let mut words = listing
        .said
        .iter()
        .map(|(endpoint, said)| match said {
            Ok(count) => format!("{endpoint}: {count} models."),
            Err(why) => format!("{endpoint}: {why}."),
        })
        .collect::<Vec<_>>();
    if listing.asking > 0 {
        words.push("Asking...".into());
    }
    words.join(" ")
}

/// The source of a model's price, in a word.
fn source(priced: Priced) -> &'static str {
    match priced {
        Priced::Unknown => "no price",
        Priced::Listed => "listed",
        Priced::Assigned => "assigned",
        Priced::Yours => "yours",
    }
}

/// The width of a price, as a field or as words.
const PRICE_WIDE: f32 = 84.0;

/// A price of the model `name` for a million tokens: a field where
/// nobody else has said it, and greyed words where the endpoint or
/// the home gave it.
fn price(
    name: &str,
    get: fn(&Model) -> f64,
    set: fn(&mut Model, f64),
) -> AnyView {
    let (known, name) = (name.to_string(), name.to_string());
    keyed(
        resource::<ModelCatalogue, _>(move |models| {
            models.catalogue.get(&known).is_none_or(|model| {
                matches!(
                    model.priced,
                    Priced::Unknown | Priced::Yours
                )
            })
        }),
        move |open| {
            let (shown, changed) = (name.clone(), name.clone());
            let value =
                resource::<ModelCatalogue, _>(move |models| {
                    models.catalogue.get(&shown).map_or(0.0, get)
                });
            if !open {
                return row((label(
                    value.map(|v| format!("{v:.2}")),
                )
                .size(12.0)
                .tone(Tone::Faint),))
                .width(px(PRICE_WIDE))
                .boxed();
            }
            number_field(value, move |world, value: f64| {
                change(world, &changed, |model| {
                    set(model, value);
                    model.priced = Priced::Yours;
                });
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

/// One model of the catalogue: its allowance and its prices.
fn entry(name: &str) -> AnyView {
    let name = name.to_string();
    let (allowed, allow) = (name.clone(), name.clone());
    let from = name.clone();
    column((
        row((
            tick(resource::<ModelCatalogue, _>(move |models| {
                models
                    .catalogue
                    .get(&allowed)
                    .is_some_and(|model| model.allowed)
            }))
            .on_change(move |world, on| {
                change(world, &allow, |model| model.allowed = on);
            }),
            row((label(name.clone()),)).fills_width(),
        ))
        .gap(10.0)
        .align(AlignItems::Center),
        row((
            row(()).width(px(CONTROL)).shrink(0.0),
            small("IN"),
            price(&name, |m| m.input, |m, v| m.input = v),
            small("OUT"),
            price(&name, |m| m.output, |m, v| m.output = v),
            label(resource::<ModelCatalogue, _>(move |models| {
                let Some(model) = models.catalogue.get(&from) else {
                    return String::new();
                };
                match model.context {
                    Some(context) => format!(
                        "{}, holds {}k",
                        source(model.priced),
                        context / 1000
                    ),
                    None => source(model.priced).to_string(),
                }
            }))
            .size(11.0)
            .tone(Tone::Faint),
        ))
        .gap(8.0)
        .align(AlignItems::Center),
    ))
    .gap(4.0)
    .boxed()
}

/// The `names` that `narrow` leaves, and the count of those past what
/// is shown.
pub fn narrowed<N: AsRef<str>>(
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

/// The catalogue: every model the endpoints serve, for the chief
/// to allow, price and describe.
pub fn shelf() -> AnyView {
    let shown = bevy_fynix::derived(|world: &World| {
        let all = &world.resource::<ModelCatalogue>().catalogue;
        narrowed(
            all.models.iter().map(Model::name),
            &world.resource::<Draft>().narrow,
        )
    })
    .when(either(
        moved::<ModelCatalogue>(),
        either(moved::<Shelf>(), moved::<Draft>()),
    ));
    let models = keyed(shown, |(names, more)| {
        if names.is_empty() {
            return note(
                "No models yet. Choose List Models to ask the \
                 endpoints.",
            )
            .boxed();
        }
        let mut rows =
            names.iter().map(|name| entry(name)).collect::<Vec<_>>();
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
            chooser(home_default, |world, picked| {
                let mut config =
                    world.resource::<Settings>().config.clone();
                config.model = picked.to_string();
                save(world, config.clone());
                world.trigger(TestConnection(config));
            }),
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
                world.trigger(ListModels)
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
        note(resource::<Listing, _>(said)),
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

#[cfg(test)]
mod tests {
    use hamlet_model::catalogue::Catalogue;

    use super::*;

    #[test]
    fn typing_narrows_the_catalogue_and_the_rest_is_counted() {
        let model = |id: String| Model {
            id,
            ..Model::default()
        };
        let all = Catalogue {
            models: (0..SHOWN + 5)
                .map(|n| model(format!("vendor/large-{n}")))
                .chain([model("Other/Small".into())])
                .collect(),
        };
        let names = || all.models.iter().map(Model::name);
        let (shown, more) = narrowed(names(), "");
        assert_eq!((shown.len(), more), (SHOWN, 6));
        assert_eq!(
            narrowed(names(), "small other"),
            (vec!["Other/Small".into()], 0)
        );
    }
}
