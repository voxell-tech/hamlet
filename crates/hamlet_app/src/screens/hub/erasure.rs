//! What the chief asked to erase, followed until the server has
//! erased it: the Hub leaves its page, the selection lets go of it
//! and a toast says it is gone.

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

/// One erasure asked for: of what, its name, and where the Hub goes
/// if it shows it when it is gone.
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

/// Follows the erasure of `agent`, which has left: once it is gone
/// the Hub goes to where it was last, as [`resolve`] finds it.
pub(crate) fn expect_agent(world: &mut World, agent: Entity) {
    let name = agents::shown_of(world, agent);
    let after = Landing::Page(Showing::Realm);
    expect(world, Erased::Entity(agent), name, after);
}

/// Where the Hub goes once an agent being erased is gone: to its
/// lead's thread, else its hamlet's page, else the realm's, as the
/// rail lists them over it. Found as it goes, so a hire moved after
/// its erasure was asked for lands where it was moved to.
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

/// Where the Hub goes from the page of `agent`, where it is now.
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
    let after = Landing::Page(Showing::Realm);
    expect(world, Erased::Entity(scholar), name, after);
}

/// Follows the erasure of the closed hamlet `id`, called `name`.
pub(crate) fn expect_hamlet(
    world: &mut World,
    id: String,
    name: String,
) {
    let after = Landing::Page(Showing::Realm);
    expect(world, Erased::Closed(id), name, after);
}

fn expect(
    world: &mut World,
    of: Erased,
    name: String,
    after: Landing,
) {
    let mut erasures = world.get_resource_or_init::<Erasures>();
    erasures.0.retain(|erasure| erasure.of != of);
    erasures.0.push(Erasure { of, name, after });
}

/// The entity the Hub's page is of, if it is of one. A thread
/// follows its agent itself, as the chat is seated again.
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

/// The entity `thing` is, or is of.
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

/// Whether an erasure waits, or the Hub shows a page whose subject is
/// gone.
fn is_unsettled(world: &World) -> bool {
    !world.resource::<Erasures>().0.is_empty()
        || shown_entity(world)
            .is_some_and(|shown| world.get_entity(shown).is_err())
}

/// Settles each erasure the server has made, and takes the Hub off a
/// page whose subject is gone for any reason.
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
    // The page of anything gone, erased elsewhere or by another
    // client, is the realm's.
    if let Some(shown) = shown_entity(world)
        && world.get_entity(shown).is_err()
    {
        land(world, Landing::Page(Showing::Realm));
    }
}

/// Turns the Hub to `after`, or to the realm's page if what it names
/// is gone too.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::panel_app;

    #[test]
    fn erasing_the_shown_agent_turns_the_hub_to_its_hamlet() {
        let mut app = panel_app();
        app.add_plugins(plugin)
            .init_resource::<Showing>()
            .init_resource::<Selected>()
            .init_resource::<hamlet_realm::pick::Also>();
        let world = app.world_mut();
        let ada = agents::named(world, "hire#2").unwrap();
        world.entity_mut(ada).insert(Left);
        *world.resource_mut::<Showing>() = Showing::Gone(ada);
        world.resource_mut::<Selected>().0 = Some(Thing::Ruin(ada));
        expect_agent(world, ada);
        app.update();
        // Nothing moves until the server has erased it.
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Gone(ada)
        );

        // Moved to another hamlet before it is gone, it lands there.
        let hamlet = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(ada).insert(LivesIn(hamlet));
        app.world_mut().despawn(ada);
        app.update();
        let world = app.world();
        assert_eq!(
            *world.resource::<Showing>(),
            Showing::Hamlet(hamlet)
        );
        assert_eq!(world.resource::<Selected>().0, None);
    }
}
