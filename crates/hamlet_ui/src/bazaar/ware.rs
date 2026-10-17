//! A ware on a page of its own: all that is told of it, who serves
//! it, and the ways to add it.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label, row};
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_core::bazaar::Bazaar;
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::Settings;
use hamlet_model::bazaar::{Stall, Ware};

use super::artisans::named;
use super::shelf::{dollars, heading, price_words, tokens};
use super::{Browsing, Then, acquire, added, name_of};
use crate::AnyView;
use crate::icons::Glyph;
use crate::looks::{
    action, bare, buttons, good_action, note, page_head, stated,
};
use crate::roll::{either, moved};

/// The width of the name of a fact.
const NAMED: f32 = 118.0;

/// The ware `id` of the source shown, all that is told of it and the
/// ways to add it.
pub(super) fn page(id: String) -> AnyView {
    let found = derived(move |world: &World| {
        let browsing = world.resource::<Browsing>();
        let ware = browsing
            .wares(world.resource::<Bazaar>())
            .iter()
            .find(|ware| ware.id == id)
            .cloned();
        (ware, browsing.source.is_none())
    })
    .when(either(moved::<Bazaar>(), moved::<Browsing>()));
    let body = keyed(found, |(ware, openrouter)| match ware {
        None => note("This model is no longer on the shelf.").boxed(),
        Some(ware) => about(ware, *openrouter),
    })
    .within(bare());
    column((
        page_head(
            "BAZAAR",
            |world| world.resource_mut::<Browsing>().open = None,
            Glyph::Coins,
            "MODEL",
        ),
        heading(),
        body,
    ))
    .gap(16.0)
    .boxed()
}

fn about(ware: &Ware, openrouter: bool) -> AnyView {
    let mut parts = vec![named(ware.title.clone()).boxed()];
    if ware.tools == Some(false) {
        parts.push(
            label(
                "This model can't use tools, so agents can only \
                 chat with it.",
            )
            .size(12.0)
            .tone(Tone::Critical)
            .boxed(),
        );
    }
    parts.push(actions(ware.id.clone()));
    if let Some(first) = ware.description.split("\n\n").next()
        && !first.trim().is_empty()
    {
        parts.push(
            label(first.trim().to_string())
                .size(12.0)
                .tone(Tone::Dim)
                .boxed(),
        );
    }
    parts.push(facts(ware));
    if openrouter {
        parts.push(stalls(ware.id.clone()));
    }
    column(parts).gap(14.0).boxed()
}

/// What can be done with the ware `id`: add it, have the agent the
/// chief came for run it, make it the default.
fn actions(id: String) -> AnyView {
    let now = {
        let id = id.clone();
        derived(move |world: &World| {
            let browsing = world.resource::<Browsing>();
            let source = browsing.source.as_deref();
            let config = &world.resource::<Settings>().config;
            let default = name_of(config, source, &id)
                .is_some_and(|name| name == config.model);
            let for_agent = browsing
                .for_agent
                .is_some_and(|agent| world.get_entity(agent).is_ok());
            (added(world, source, &id), for_agent, default)
        })
        .when(either(
            moved::<Browsing>(),
            either(moved::<ModelCatalogue>(), moved::<Settings>()),
        ))
    };
    keyed(now, move |(added, for_agent, default)| {
        let mut all = Vec::new();
        if *added {
            all.push(
                stated(Glyph::Check, "ADDED", Tone::Accent, 12.0)
                    .boxed(),
            );
        } else {
            let id = id.clone();
            all.push(good_action(Glyph::Plus, "ADD", move |world| {
                acquire(world, &id, Then::Keep)
            }));
        }
        if *for_agent {
            let id = id.clone();
            all.push(action(
                Glyph::Robot,
                "USE FOR THIS AGENT",
                move |world| {
                    let agent =
                        world.resource::<Browsing>().for_agent;
                    if let Some(agent) = agent {
                        acquire(world, &id, Then::RunFor(agent));
                    }
                },
            ));
        }
        if *default {
            all.push(note("Your default model.").boxed());
        } else {
            let id = id.clone();
            all.push(action(
                Glyph::Sliders,
                "MAKE DEFAULT",
                move |world| acquire(world, &id, Then::MakeDefault),
            ));
        }
        buttons(all).boxed()
    })
    .within(bare())
    .boxed()
}

/// A fact of a ware: its name and what it is.
fn fact(name: &'static str, value: String) -> AnyView {
    row((
        row((label(name).size(12.0).tone(Tone::Faint),))
            .width(px(NAMED))
            .shrink(0.0),
        label(value).size(13.0),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

fn facts(ware: &Ware) -> AnyView {
    let mut all = Vec::new();
    let price = match ware.price() {
        Some(_) => {
            format!("{} per million tokens", price_words(ware))
        }
        None => price_words(ware),
    };
    all.push(fact("Price", price));
    if let Some(context) = ware.context {
        all.push(fact(
            "Context",
            format!("{} tokens", tokens(context)),
        ));
    }
    if !ware.inputs.is_empty() {
        all.push(fact("Reads", ware.inputs.join(", ")));
    }
    let released = ware.created.and_then(|at| {
        chrono::DateTime::from_timestamp(at as i64, 0)
    });
    if let Some(at) = released {
        all.push(fact("Released", at.format("%B %Y").to_string()));
    }
    if let Some(index) = ware.intelligence {
        all.push(fact("Intelligence", format!("{index:.0} / 100")));
    }
    column(all).gap(6.0).boxed()
}

/// One provider that serves the ware.
fn stall(stall: &Stall) -> AnyView {
    let mut told = Vec::new();
    if let Some(uptime) = stall.uptime {
        told.push(format!("{uptime:.1}% up over a day"));
    }
    if let (Some(input), Some(output)) = (stall.input, stall.output) {
        told.push(format!(
            "{} in, {} out",
            dollars(input),
            dollars(output)
        ));
    }
    if let Some(context) = stall.context {
        told.push(format!("{} context", tokens(context)));
    }
    if let Some(quantization) = &stall.quantization {
        told.push(quantization.clone());
    }
    column((
        label(stall.provider.clone()).size(13.0).bold(true),
        note(told.join(", ")),
    ))
    .gap(2.0)
    .boxed()
}

/// The providers that serve the ware `id` through OpenRouter.
fn stalls(id: String) -> AnyView {
    let known = derived(move |world: &World| {
        world.resource::<Bazaar>().stalls.get(&id).cloned()
    })
    .when(moved::<Bazaar>());
    let list = keyed(known, |known| match known {
        None | Some(None) => note("Asking OpenRouter...").boxed(),
        Some(Some(Err(why))) => {
            note(format!("OpenRouter could not be asked: {why}"))
                .boxed()
        }
        Some(Some(Ok(stalls))) if stalls.is_empty() => {
            note("No provider serves it now.").boxed()
        }
        Some(Some(Ok(stalls))) => column(
            stalls.iter().take(3).map(stall).collect::<Vec<_>>(),
        )
        .gap(8.0)
        .boxed(),
    })
    .within(bare());
    column((
        label("Providers").size(12.0).bold(true).tone(Tone::Faint),
        list,
    ))
    .gap(6.0)
    .boxed()
}
