//! Erasures followed until the server makes them.

use bevy::app::{App, PreUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::Remove;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::world::{DeferredWorld, World};
use bevy::time::Time;
use hamlet_client::realm::{
    self as agents, Closed, Identity, Kind, Left, LivesIn, Parent,
};
use hamlet_realm::pick::{Selected, Thing, select};
use hamlet_ui::theme::StatusKind;

use super::{Showing, show};
use crate::PanelSystems;
use crate::convo::talk_to;
use crate::toasts::Toasts;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Erasures>()
        .add_observer(resolve)
        .add_systems(
            PreUpdate,
            settle.run_if(is_unsettled).in_set(PanelSystems::Follow),
        );
}

/// The erasures asked for that the server has yet to make.
#[derive(Resource, Default)]
pub(crate) struct Erasures(Vec<Erasure>);

struct Erasure {
    of: Erased,
    name: String,
    after: Landing,
}

/// What is being erased.
#[derive(Clone, PartialEq)]
enum Erased {
    /// An agent that left, or a scholar.
    Entity(Entity),
    /// A closed hamlet, by its id.
    Closed(String),
}

/// Where the Hub goes from the page of what was erased.
#[derive(Clone, Copy)]
enum Landing {
    /// The thread of the agent it was under.
    Thread(Entity),
    Page(Showing),
}

/// Follows the erasure of `agent`, which has left.
pub(crate) fn expect_agent(world: &mut World, agent: Entity) {
    let name = agents::shown_of(world, agent);
    expect(world, Erased::Entity(agent), name);
}

/// Sets where the Hub goes from an erased agent's page.
fn resolve(gone: On<Remove<Identity>>, mut world: DeferredWorld) {
    let agent = gone.entity;
    let erased = Erased::Entity(agent);
    if !world
        .resource::<Erasures>()
        .0
        .iter()
        .any(|e| e.of == erased)
    {
        return;
    }
    let after = landing(&world, agent);
    for erasure in &mut world.resource_mut::<Erasures>().0 {
        if erasure.of == erased {
            erasure.after = after;
        }
    }
}

/// Its lead's thread, else its hamlet's page, else the realm's.
fn landing(world: &World, agent: Entity) -> Landing {
    let home =
        |of: Entity| world.get::<LivesIn>(of).map(|lives| lives.0);
    let hamlet = home(agent)
        .filter(|hamlet| world.get_entity(*hamlet).is_ok());
    // A mayor heads its hamlet, under no lead of its own there.
    let lead = world
        .get::<Parent>(agent)
        .and_then(|parent| agents::agent(world, parent.0))
        .filter(|lead| world.get::<Left>(*lead).is_none())
        .filter(|lead| hamlet.is_none() || home(*lead) == hamlet);
    match (lead, hamlet) {
        (Some(lead), _) => Landing::Thread(lead),
        (None, Some(hamlet)) => {
            Landing::Page(Showing::Hamlet(hamlet))
        }
        (None, None) => Landing::Page(Showing::Realm),
    }
}

/// Follows the erasure of `scholar`.
pub(crate) fn expect_scholar(
    world: &mut World,
    scholar: Entity,
    name: String,
) {
    expect(world, Erased::Entity(scholar), name);
}

/// Follows the erasure of the closed hamlet `id`, called `name`.
pub(crate) fn expect_hamlet(
    world: &mut World,
    id: String,
    name: String,
) {
    expect(world, Erased::Closed(id), name);
}

fn expect(world: &mut World, of: Erased, name: String) {
    let after = Landing::Page(Showing::Realm);
    let mut erasures = world.get_resource_or_init::<Erasures>();
    erasures.0.retain(|erasure| erasure.of != of);
    erasures.0.push(Erasure { of, name, after });
}

/// The entity the Hub's page is of.
fn shown_entity(world: &World) -> Option<Entity> {
    match *world.get_resource::<Showing>()? {
        Showing::Scholar(of)
        | Showing::Hamlet(of)
        | Showing::Gone(of)
        | Showing::Chief(of) => Some(of),
        Showing::Thread
        | Showing::Prompt
        | Showing::Realm
        | Showing::Library => None,
    }
}

fn entity_of(thing: Thing) -> Option<Entity> {
    match thing {
        Thing::Agent(of)
        | Thing::House(of)
        | Thing::Ruin(of)
        | Thing::Scholar(of)
        | Thing::Common(of)
        | Thing::Chief(of)
        | Thing::ChiefPlace(of) => Some(of),
        _ => None,
    }
}

fn is_unsettled(world: &World) -> bool {
    !world.resource::<Erasures>().0.is_empty()
        || shown_entity(world)
            .is_some_and(|shown| world.get_entity(shown).is_err())
}

/// Settles the erasures made and leaves a page whose subject is gone.
fn settle(world: &mut World) {
    let made = {
        let closed = world.get_resource::<Closed>();
        let gone = |of: &Erased| match of {
            Erased::Entity(entity) => {
                world.get_entity(*entity).is_err()
            }
            Erased::Closed(id) => closed.is_none_or(|closed| {
                !closed.0.iter().any(|hamlet| hamlet.id == *id)
            }),
        };
        let erasures = &world.resource::<Erasures>().0;
        (0..erasures.len())
            .filter(|at| gone(&erasures[*at].of))
            .collect::<Vec<_>>()
    };
    for at in made.into_iter().rev() {
        let erasure = world.resource_mut::<Erasures>().0.remove(at);
        if let Erased::Entity(entity) = erasure.of {
            if shown_entity(world) == Some(entity) {
                land(world, erasure.after);
            }
            let selected = world
                .get_resource::<Selected>()
                .and_then(|selected| selected.0)
                .and_then(entity_of);
            if selected == Some(entity) {
                select(world, None);
            }
        }
        let now = world.resource::<Time>().elapsed_secs();
        if let Some(mut toasts) = world.get_resource_mut::<Toasts>() {
            toasts.note(
                StatusKind::Success,
                format!("{} was erased", erasure.name),
                String::new(),
                now,
            );
        }
    }
    // Gone for another reason, as erased by another client.
    if let Some(shown) = shown_entity(world)
        && world.get_entity(shown).is_err()
    {
        land(world, Landing::Page(Showing::Realm));
    }
}

/// Turns the Hub to `after`, else to the realm's page.
fn land(world: &mut World, after: Landing) {
    match after {
        Landing::Thread(lead)
            if world.get::<Kind>(lead).is_some()
                && world.get::<Left>(lead).is_none() =>
        {
            talk_to(world, lead);
        }
        Landing::Page(Showing::Hamlet(hamlet))
            if world.get_entity(hamlet).is_ok() =>
        {
            show(world, Showing::Hamlet(hamlet));
        }
        Landing::Page(
            showing @ (Showing::Realm | Showing::Library),
        ) => {
            show(world, showing);
        }
        _ => show(world, Showing::Realm),
    }
}
