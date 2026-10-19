//! The model an agent or a hamlet runs, chosen from those the home
//! allows.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, percent};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, dropdown, label, row};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_core::agents::{Account, LivesIn, Mayor, Settlement};
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::Settings;
use hamlet_home::limits::{self, Limits};
use hamlet_model::catalogue::Catalogue;
use hamlet_rules::hamlet::{Hamlet, MAYOR};
use hamlet_sdk::checks::{either, moved};

use super::Shelf;
use crate::icons::Glyph;
use crate::looks::{Fills, bare, note, tick};
use crate::roll::agents_moved;
use crate::{AnyView, Cx, View};

/// Puts the model `name` on the list of the hamlet `hamlet`, or takes
/// it off. A name on the list that the home does not allow is kept,
/// so that it holds again if the home allows it.
fn pick(world: &mut World, hamlet: &Hamlet, name: &str, on: bool) {
    let mut list = Limits::load(hamlet.root())
        .map(|limits| limits.models)
        .unwrap_or_default();
    list.retain(|known| known != name);
    if on {
        list.push(name.to_string());
    }
    let written = list
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let done = limits::set(
        hamlet.root(),
        "",
        "models",
        Some(&format!("[{written}]")),
    );
    let mut shelf = world.resource_mut::<Shelf>();
    match done {
        Ok(()) => {
            shelf.lists.insert(hamlet.name().to_string(), list);
        }
        Err(e) => shelf.problem = Some(format!("{e:#}")),
    }
}

/// The model a home runs where nothing else is chosen.
pub(super) fn home_default(world: &World) -> String {
    world
        .get_resource::<Settings>()
        .map(|settings| settings.config.model.clone())
        .unwrap_or_default()
}

/// The names a dropdown of models offers: the home's default, every
/// model the home allows, and `now` when it is none of those.
fn offered(
    catalogue: &Catalogue,
    default: &str,
    now: &str,
) -> Vec<String> {
    let mut names = vec![default.to_string()];
    for name in catalogue.allowed().map(|model| model.name()) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if !now.is_empty() && !names.iter().any(|name| name == now) {
        names.push(now.to_string());
    }
    names.retain(|name| !name.is_empty());
    names
}

/// A dropdown that shows the one model in use, which `read` tells,
/// and gives `write` another when one is picked.
pub(super) fn chooser(
    read: impl Fn(&World) -> String + Send + Sync + 'static,
    write: impl Fn(&mut World, &str) + Send + Sync + Clone + 'static,
) -> impl View {
    let options = bevy_fynix::derived(move |world: &World| {
        let now = read(world);
        let names = world
            .get_resource::<ModelCatalogue>()
            .map(|models| {
                offered(&models.catalogue, &home_default(world), &now)
            })
            .unwrap_or_default();
        (names, now)
    })
    // The model in use is an agent's or a hamlet's, and what is
    // offered is the home's.
    .when(either(
        agents_moved(),
        either(moved::<Shelf>(), moved::<Settings>()),
    ));
    keyed(options, move |(names, now)| {
        let at = names
            .iter()
            .position(|name| name == now)
            .unwrap_or(names.len());
        let (names, write, was) =
            (names.clone(), write.clone(), now.clone());
        let offered = names.clone();
        dropdown(
            offered,
            at,
            Glyph::Chevron.image(crate::looks::ICON),
            move |world, chosen| {
                if let Some(picked) =
                    names.get(chosen).filter(|picked| **picked != was)
                {
                    write(world, picked);
                }
            },
        )
        // As wide as the panel gives it: a model's name is long,
        // and the control is otherwise no wider than a short word.
        .width(percent(100.0))
        .max_width(percent(100.0))
        .boxed()
    })
    .within(bare())
}

/// Sets `key` of `section` in the limits of `hamlet` to the model
/// `model`, or takes it out, and shows why when it cannot.
fn set_model(
    world: &mut World,
    hamlet: &Hamlet,
    (section, key): (&str, &str),
    model: Option<&str>,
) {
    let written = model.map(|model| format!("{model:?}"));
    let done =
        limits::set(hamlet.root(), section, key, written.as_deref());
    let mut shelf = world.resource_mut::<Shelf>();
    shelf.problem = done.err().map(|e| format!("{e:#}"));
    // Read again at once.
    shelf.wrote = true;
}

/// The model an agent of the hamlet `place` runs when no other was
/// chosen for it, for its chief to change.
fn default_of(place: &Settlement) -> AnyView {
    let (at, place) = (place.name.clone(), place.clone());
    chooser(
        move |world| {
            world
                .get_resource::<Shelf>()
                .and_then(|shelf| shelf.defaults.get(&at).cloned())
                .flatten()
                .unwrap_or_else(|| home_default(world))
        },
        move |world, model| {
            // The home's own is what a hamlet has with none set.
            let own = *model == home_default(world);
            let model = (!own).then_some(model);
            set_model(world, &place.record(), ("", "model"), model);
        },
    )
    .boxed()
}

/// Has the agent `name` of `hamlet` run `model` from now on: the
/// hamlet is let run it, a choice the chief wrote into the limits by
/// hand is taken out, and the choice is written where the mayor's
/// are.
fn run_model(
    world: &mut World,
    hamlet: &Hamlet,
    name: &str,
    model: &str,
) {
    if name == MAYOR {
        // The mayor's own is the chief's alone to say.
        let own = *model == home_default(world);
        let model = (!own).then_some(model);
        return set_model(world, hamlet, ("pinned", MAYOR), model);
    }
    let limits = Limits::load(hamlet.root()).unwrap_or_default();
    if limits.pinned.contains_key(name) {
        set_model(world, hamlet, ("pinned", name), None);
    }
    // With none of its own it runs the hamlet's, or the home's.
    let default =
        limits.model.clone().unwrap_or_else(|| home_default(world));
    let chosen = if *model == default { "" } else { model };
    if !chosen.is_empty() && !limits.allows(chosen) {
        pick(world, hamlet, chosen, true);
    }
    let done =
        hamlet.choose_model(name, chosen, "The chief chose it.");
    if let Err(e) = done {
        world.resource_mut::<Shelf>().problem =
            Some(format!("{e:#}"));
    }
    hamlet_core::agents::sync(world);
}

/// The model `agent` runs, for its chief to change. Nothing for an
/// agent that is gone.
pub fn of_agent(agent: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let lives = cx.world.get::<LivesIn>(agent).map(|l| l.0);
        let place = lives
            .and_then(|hamlet| cx.world.get::<Settlement>(hamlet))
            .cloned();
        let Some(place) = place else {
            return cx.build(column(()));
        };
        // The office is the mayor's, whoever holds it.
        let name = match cx.world.get::<Mayor>(agent) {
            Some(_) => MAYOR.to_string(),
            None => hamlet_core::agents::name_of(cx.world, agent),
        };
        let pick = chooser(
            move |world| {
                world
                    .get::<Account>(agent)
                    .map(|account| account.0.model.clone())
                    .unwrap_or_default()
            },
            move |world, model| {
                run_model(world, &place.record(), &name, model);
            },
        );
        // The name over the control, which takes the whole width.
        cx.build(column((note("Model"), pick)).gap(4.0))
    })
}

/// The models the hamlet `hamlet` may run, picked from those the
/// home allows.
pub fn of_hamlet(hamlet: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(place) = cx.world.get::<Settlement>(hamlet).cloned()
        else {
            return cx.build(column(()));
        };
        let default = default_of(&place);
        let allowed = resource::<ModelCatalogue, _>(|models| {
            models.catalogue.allowed().cloned().collect::<Vec<_>>()
        });
        let rows = keyed(allowed, move |models| {
            if models.is_empty() {
                return label(
                    "The home allows no model besides its default \
                     yet. Allow some under Models in Settings.",
                )
                .size(11.0)
                .tone(Tone::Dim)
                .boxed();
            }
            let rows = models
                .iter()
                .map(|model| {
                    let name = model.name();
                    let (at, picked) =
                        (place.name.clone(), name.clone());
                    let record = place.record();
                    row((
                        tick(resource::<Shelf, _>(move |shelf| {
                            shelf.lists.get(&at).is_some_and(|list| {
                                list.contains(&picked)
                            })
                        }))
                        .on_change(
                            move |world, on| {
                                pick(world, &record, &name, on);
                            },
                        ),
                        row((note(model.card()),)).fills_width(),
                    ))
                    .gap(10.0)
                    .align(AlignItems::Center)
                    .boxed()
                })
                .collect::<Vec<_>>();
            column(rows).gap(6.0).boxed()
        })
        .within(bare());
        cx.build(
            column((
                hamlet_sdk::cards::heading(
                    Glyph::Chip,
                    "MODELS THIS HAMLET MAY RUN",
                ),
                label(
                    "The mayor picks from the ticked models. The \
                     default model is always allowed.",
                )
                .size(11.0)
                .tone(Tone::Dim),
                rows,
                column((note("Its default model"), default)).gap(4.0),
                label(
                    "Used when neither you nor the mayor chose a \
                     model.",
                )
                .size(11.0)
                .tone(Tone::Dim),
            ))
            .gap(6.0),
        )
    })
}

#[cfg(test)]
mod tests {
    use hamlet_model::catalogue::{Catalogue, Model};

    use super::*;

    #[test]
    fn a_chief_s_choice_of_model_is_written_and_taken_back() {
        let home = hamlet_testing::Scratch::new("models-choice");
        let mill = Hamlet::at(&home, "mill");
        std::fs::create_dir_all(mill.root()).unwrap();
        let mut world = World::new();
        world.init_resource::<Shelf>();
        let read = || Limits::load(mill.root()).unwrap();

        set_model(&mut world, &mill, ("", "model"), Some("far::big"));
        set_model(
            &mut world,
            &mill,
            ("pinned", "wool-carder"),
            Some("a:b"),
        );
        set_model(
            &mut world,
            &mill,
            ("pinned", MAYOR),
            Some("small"),
        );
        let kept = read();
        assert_eq!(kept.model.as_deref(), Some("far::big"));
        assert_eq!(kept.pinned["wool-carder"], "a:b");
        assert_eq!(kept.pinned[MAYOR], "small");
        assert!(world.resource::<Shelf>().problem.is_none());

        set_model(&mut world, &mill, ("", "model"), None);
        set_model(&mut world, &mill, ("pinned", "wool-carder"), None);
        let kept = read();
        assert_eq!(kept.model, None);
        assert_eq!(kept.pinned.len(), 1);

        // The model run is offered, though the home does not list it,
        // after the home's default.
        let catalogue = Catalogue::default();
        assert_eq!(
            offered(&catalogue, "own", "by-hand"),
            ["own", "by-hand"]
        );
        assert_eq!(offered(&catalogue, "own", "own"), ["own"]);
    }

    #[test]
    fn an_agent_runs_the_one_model_that_was_last_picked_for_it() {
        let home = hamlet_testing::Scratch::new("models-run");
        let mill = Hamlet::at(&home, "mill");
        std::fs::create_dir_all(mill.root()).unwrap();
        mill.create("carder", "Cards wool.", "test").unwrap();
        Catalogue {
            models: vec![Model {
                id: "big".into(),
                allowed: true,
                ..Model::default()
            }],
        }
        .save(&home)
        .unwrap();
        let mut world = World::new();
        world.init_resource::<Shelf>();

        // Picked, it is run: the hamlet is let run it on the way.
        run_model(&mut world, &mill, "carder", "big");
        assert_eq!(mill.model_of("carder").as_deref(), Some("big"));
        assert!(Limits::load(mill.root()).unwrap().allows("big"));
        // A choice written by hand gives way to the one picked.
        set_model(
            &mut world,
            &mill,
            ("pinned", "carder"),
            Some("old"),
        );
        run_model(&mut world, &mill, "carder", "big");
        assert_eq!(mill.model_of("carder").as_deref(), Some("big"));
        assert!(Limits::load(mill.root()).unwrap().pinned.is_empty());
        // The mayor's is the chief's to say, and kept with the
        // limits.
        run_model(&mut world, &mill, MAYOR, "big");
        assert_eq!(mill.model_of(MAYOR).as_deref(), Some("big"));
    }

    #[test]
    fn a_hamlet_s_pick_is_written_to_its_own_limits() {
        let home = hamlet_testing::Scratch::new("models-pick");
        let mill = Hamlet::at(&home, "mill");
        std::fs::create_dir_all(mill.root()).unwrap();
        let mut world = World::new();
        world.init_resource::<Shelf>();
        pick(&mut world, &mill, "far::large", true);
        pick(&mut world, &mill, "small", true);
        pick(&mut world, &mill, "far::large", false);
        let kept = Limits::load(mill.root()).unwrap().models;
        assert_eq!(kept, ["small"]);
        assert_eq!(world.resource::<Shelf>().lists["mill"], kept);
        // The first hamlet's list is another file and is untouched.
        assert!(Limits::load(&home).unwrap().models.is_empty());
    }
}
