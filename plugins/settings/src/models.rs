//! The Models page: the default model, and every model the endpoints
//! serve, for the person to allow and to price.

use agents_ui::narrowed;
use hamlet_engine_bevy::api::agents::{
    AgentsRequest, CatalogueModel, Listing, Priced,
};
use hamlet_engine_bevy::ui::{
    AnyView, FrameProps, Size, Space, Tone, UiExt, ViewExt, button,
    column, dropdown, field, keyed, label, resource, row, switch,
};

use crate::parts::{name, note, problem};
use crate::{Known, Page, SHOWN, SettingsWorldExt};

pub(crate) fn page() -> AnyView {
    column((
        column((
            name("DEFAULT MODEL"),
            default_model(),
            note(
                "Used wherever nobody chose a model. It is always \
                 allowed.",
            ),
        ))
        .space(Space::Half),
        note(
            "Tick a model to allow it. Prices are dollars per \
             million tokens in and out, used when the endpoint does \
             not bill a call.",
        ),
        row((
            button("LIST MODELS").on_click(|world| {
                world.ask(Page::Models, AgentsRequest::ListModels);
            }),
            name("FIND"),
            field(resource::<Known, _>(|known| known.narrow.clone()))
                .placeholder("Part of a name")
                .on_commit(|world, text| {
                    world.resource_mut::<Known>().narrow = text;
                }),
        )),
        note(resource::<Known, _>(|known| {
            known
                .settings
                .as_ref()
                .map(|settings| said(&settings.listing))
                .unwrap_or_default()
        })),
        problem(Page::Models),
        keyed(
            resource::<Known, _>(|known| {
                narrowed(
                    &known.models,
                    &known.narrow,
                    SHOWN,
                    |model| &model.name,
                )
            }),
            |(models, more)| shelf(models, *more),
        ),
    ))
    .boxed()
}

/// The model that runs where nobody chose one, to pick from the
/// models allowed.
fn default_model() -> AnyView {
    keyed(
        resource::<Known, _>(|known| {
            let now = known
                .settings
                .as_ref()
                .map(|settings| settings.model.clone())
                .unwrap_or_default();
            let mut names = vec![now.clone()];
            for model in known.models.iter().filter(|m| m.allowed) {
                if !names.contains(&model.name) {
                    names.push(model.name.clone());
                }
            }
            names.retain(|name| !name.is_empty());
            names
        }),
        |names| {
            let offered = names.clone();
            dropdown(names.clone(), 0)
                .on_pick(move |world, at| {
                    // The first is the model that runs now.
                    if at == 0 {
                        return;
                    }
                    if let Some(picked) = offered.get(at as usize) {
                        world.ask(
                            Page::Models,
                            AgentsRequest::SetModel(picked.clone()),
                        );
                    }
                })
                .boxed()
        },
    )
    .boxed()
}

/// What each endpoint answered when last asked for its models.
fn said(listing: &Listing) -> String {
    let mut words = listing
        .said
        .iter()
        .map(|listed| match &listed.models {
            Ok(count) => {
                format!("{}: {count} models.", listed.endpoint)
            }
            Err(why) => format!("{}: {why}.", listed.endpoint),
        })
        .collect::<Vec<_>>();
    if listing.asking > 0 {
        words.push("Asking...".into());
    }
    words.join(" ")
}

/// The models listed, and how many more there are.
fn shelf(models: &[CatalogueModel], more: usize) -> AnyView {
    if models.is_empty() && more == 0 {
        return note(
            "No models yet. Choose List Models to ask the endpoints.",
        )
        .boxed();
    }
    let mut rows = models.iter().map(entry).collect::<Vec<_>>();
    if more > 0 {
        rows.push(
            note(format!(
                "{more} more. Type part of a name to narrow them."
            ))
            .boxed(),
        );
    }
    column(rows).space(Space::Double).boxed()
}

/// One model: whether it is allowed, its prices and where they come
/// from.
fn entry(model: &CatalogueModel) -> AnyView {
    let (allowed, name) = (model.allowed, model.name.clone());
    let source = match model.priced {
        Priced::Unknown => "no price",
        Priced::Listed => "listed",
        Priced::Assigned => "assigned",
        Priced::Yours => "yours",
    };
    let about = match model.context {
        Some(context) => {
            format!("{source}, holds {}k", context / 1000)
        }
        None => source.to_string(),
    };
    column((
        switch(model.name.clone(), allowed).on_toggle(
            move |world, on| {
                world.ask(
                    Page::Models,
                    AgentsRequest::AllowModel {
                        model: name.clone(),
                        allowed: on,
                    },
                );
            },
        ),
        row((
            small("IN"),
            price(model, true),
            small("OUT"),
            price(model, false),
            label(about).size(Size::Small).tone(Tone::Faint),
        )),
    ))
    .space(Space::Half)
    .boxed()
}

fn small(text: &str) -> AnyView {
    label(text.to_string())
        .size(Size::Small)
        .tone(Tone::Faint)
        .boxed()
}

/// A price of `model` for a million tokens, in when `input`: a field
/// where nobody else has said it, else its words.
fn price(model: &CatalogueModel, input: bool) -> AnyView {
    let shown = if input { model.input } else { model.output };
    let text = format!("{shown:.2}");
    if !matches!(model.priced, Priced::Unknown | Priced::Yours) {
        return label(text)
            .size(Size::Small)
            .tone(Tone::Faint)
            .boxed();
    }
    let name = model.name.clone();
    field(text)
        .on_commit(move |world, typed| {
            // The other price as last told, which an edit of it may
            // have changed since the row was built.
            let Some(typed) = typed
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|price| (0.0..=10_000.0).contains(price))
            else {
                world.refuse(
                    Page::Models,
                    "A price is a number from 0 to 10000.".into(),
                );
                return;
            };
            // Kept at once, so an edit of the other price before the
            // catalogue is told again sends this one with it.
            let mut known = world.resource_mut::<Known>();
            let Some(model) = known
                .models
                .iter_mut()
                .find(|model| model.name == name)
            else {
                return;
            };
            if input {
                model.input = typed;
            } else {
                model.output = typed;
            }
            let (input, output) = (model.input, model.output);
            world.ask(
                Page::Models,
                AgentsRequest::PriceModel {
                    model: name.clone(),
                    input,
                    output,
                },
            );
        })
        .boxed()
}
