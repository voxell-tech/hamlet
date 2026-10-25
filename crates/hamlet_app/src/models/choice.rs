//! Model choices sent as the chief's edits of limits.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::percent;
use bevy_fynix::views::{FrameProps, column, dropdown, row};
use bevy_fynix::{ViewExt, keyed};
use hamlet_client::realm::{
    Account, Bylaws, LivesIn, MAYOR, Mayor, Settings, address_of,
};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{bare, close, description, group, section};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::{View, built};

use super::{Shelf, edit};
use crate::roll::{agents_moved, either, hamlets_moved, moved};

pub(super) fn home_default(world: &World) -> String {
    world
        .get_resource::<Settings>()
        .map(|settings| settings.0.model.clone())
        .unwrap_or_default()
}

fn offered(
    models: &[protocol::Listed],
    default: &str,
    now: &str,
) -> Vec<String> {
    let mut names = vec![default.to_string()];
    for name in models
        .iter()
        .filter(|model| model.allowed)
        .map(|model| model.name.as_str())
        .chain([now])
    {
        if !names.iter().any(|known| known == name) {
            names.push(name.to_string());
        }
    }
    names.retain(|name| !name.is_empty());
    names
}

pub(super) fn chooser(
    read: impl Fn(&World) -> String + Send + Sync + 'static,
    write: impl Fn(&mut World, &str) + Send + Sync + Clone + 'static,
) -> impl View {
    let options = bevy_fynix::derived(move |world: &World| {
        let now = read(world);
        let names = world
            .get_resource::<Shelf>()
            .map(|shelf| {
                offered(&shelf.models, &home_default(world), &now)
            })
            .unwrap_or_default();
        (names, now)
    })
    .when(either(
        either(agents_moved(), hamlets_moved()),
        either(moved::<Shelf>(), moved::<Settings>()),
    ));
    keyed(options, move |(names, now)| {
        let at = names
            .iter()
            .position(|name| name == now)
            .unwrap_or(names.len());
        let (names, write, was) =
            (names.clone(), write.clone(), now.clone());
        dropdown(
            names.clone(),
            at,
            Glyph::Chevron.image(hamlet_ui::styles::ICON),
            move |world, chosen| {
                if let Some(picked) =
                    names.get(chosen).filter(|picked| **picked != was)
                {
                    write(world, picked);
                }
            },
        )
        .width(percent(100.0))
        .max_width(percent(100.0))
        .boxed()
    })
    .within(bare())
}

fn whole(
    world: &mut World,
    hamlet: Entity,
    change: impl FnOnce(&mut protocol::Limits),
) {
    let Some(mut limits) =
        world.get::<Bylaws>(hamlet).map(|limits| limits.0.clone())
    else {
        return;
    };
    let Some(place) = world
        .get::<hamlet_client::realm::HamletId>(hamlet)
        .map(|place| place.0.clone())
    else {
        return;
    };
    change(&mut limits);
    let whole = crate::limits::whole_of(&limits);
    edit(world, |id| {
        protocol::Edit::Limits(protocol::EditLimits::Whole {
            id,
            hamlet: place,
            reason: None,
            whole,
        })
    });
}

fn pin(world: &mut World, agent: Entity, model: &str) {
    let Some(address) = address_of(world, agent) else {
        return;
    };
    let key = if world.get::<Mayor>(agent).is_some() {
        MAYOR.into()
    } else {
        address.agent
    };
    edit(world, |id| {
        protocol::Edit::Limits(protocol::EditLimits::Set {
            id,
            hamlet: address.hamlet,
            reason: None,
            set: protocol::LimitsEdit {
                section: "pinned".into(),
                key,
                value: model.into(),
            },
        })
    });
}

pub(crate) fn of_agent(agent: Entity) -> impl View {
    built(move |cx| {
        if cx.world.get::<LivesIn>(agent).is_none() {
            return cx.build(column(()));
        }
        cx.build(close((
            section("Model"),
            super::problem(),
            chooser(
                move |world| {
                    world
                        .get::<Account>(agent)
                        .map(|account| account.model.clone())
                        .unwrap_or_default()
                },
                move |world, model| pin(world, agent, model),
            ),
            row((hamlet_ui::styles::action(
                Glyph::Coins,
                "BROWSE THE BAZAAR",
                crate::bazaar::browse_models,
            ),)),
        )))
    })
}

pub(crate) fn of_hamlet(hamlet: Entity) -> impl View {
    built(move |cx| {
        if cx.world.get::<Bylaws>(hamlet).is_none() {
            return cx.build(column(()));
        }
        let default = chooser(
            move |world| {
                world
                    .get::<Bylaws>(hamlet)
                    .and_then(|limits| limits.0.model.clone())
                    .unwrap_or_else(|| home_default(world))
            },
            move |world, model| {
                let chosen = (model != home_default(world))
                    .then(|| model.to_string());
                whole(world, hamlet, |limits| limits.model = chosen);
            },
        );
        let choices = bevy_fynix::derived(move |world: &World| {
            let models = world
                .resource::<Shelf>()
                .models
                .iter()
                .filter(|model| model.allowed)
                .cloned()
                .collect::<Vec<_>>();
            let chosen = world
                .get::<Bylaws>(hamlet)
                .map(|limits| limits.0.models.clone())
                .unwrap_or_default();
            (models, chosen)
        })
        .when(either(moved::<Shelf>(), hamlets_moved()));
        let rows = keyed(choices, move |(models, chosen)| {
            if models.is_empty() {
                return description(
                    "The home allows no model besides its default \
                     yet. Allow some under Models in Settings, or \
                     add some in the Agent Bazaar.",
                )
                .boxed();
            }
            close(
                models
                    .iter()
                    .map(|model| {
                        let name = model.name.clone();
                        labelled_switch(
                            model.name.clone(),
                            chosen.contains(&name),
                            move |world, on| {
                                whole(world, hamlet, |limits| {
                                    limits.models.retain(|known| {
                                        known != &name
                                    });
                                    if on {
                                        limits
                                            .models
                                            .push(name.clone());
                                    }
                                });
                            },
                        )
                        .boxed()
                    })
                    .collect::<Vec<_>>(),
            )
            .boxed()
        })
        .within(bare());
        cx.build(group((
            crate::overview::heading(
                Glyph::Chip,
                "MODELS THIS HAMLET MAY RUN",
            ),
            description(
                "The mayor picks from the switched-on models. The \
                 default model is always allowed.",
            ),
            rows,
            super::problem(),
            close((section("Its default model"), default)),
            description(
                "Used when neither you nor the mayor chose a model.",
            ),
        )))
    })
}
