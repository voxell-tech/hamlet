//! The Hub's pages gone through, to go back and forward among them:
//! with the primary key and `[` or `]`, or the mouse's back and
//! forward buttons, while the pointer is over the Hub.

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, Layer, Mods, ScopeId, ScopeSpec,
    ShortcutAppExt,
};
use hamlet_realm::pick::{Selected, Thing, select};

use super::{Sheet, Showing, show};
use crate::convo::{Convo, seat};
use crate::trail::{
    Trail, Way, mouse_pressed, mouse_way, pointer_over,
};

/// The Hub's keys, in force while the pointer is over it.
pub(super) const KEYS: ScopeId = ScopeId("hub");

pub(super) fn plugin(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id: CommandId(id),
        label,
        scope: KEYS,
        run,
        enabled: |_| true,
        repeat: false,
    };
    let primary = |key| Chord {
        key,
        mods: Mods::PRIMARY,
    };
    app.init_resource::<Visits>()
        .add_scope(ScopeSpec {
            id: KEYS,
            label: "Hub",
            layer: Layer::Panel,
            active: None,
        })
        .add_command(
            command("hub.back", "Back", |world, _| {
                travel(world, Way::Back);
            }),
            &[primary(KeyCode::BracketLeft)],
        )
        .add_command(
            command("hub.forward", "Forward", |world, _| {
                travel(world, Way::Forward);
            }),
            &[primary(KeyCode::BracketRight)],
        )
        .add_command(
            CommandSpec {
                enabled: super::scholar::asking,
                ..command(
                    "hub.submit",
                    "Do what the scholar's form asks",
                    |world, _| super::scholar::submit(world),
                )
            },
            &crate::world_menu::SUBMIT_KEYS,
        )
        .add_systems(
            Update,
            (mouse_travel.run_if(mouse_pressed), record).chain(),
        );
}

/// One page of the Hub: what it shows, the agent it is seated at
/// where that matters, and the tab of a thread.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Visit {
    showing: Showing,
    agent: Option<Entity>,
    sheet: Sheet,
}

/// The pages behind the one shown and those ahead of it.
#[derive(Resource, Default)]
pub(crate) struct Visits(Trail<Visit>);

fn here(world: &World) -> Option<Visit> {
    let showing = *world.get_resource::<Showing>()?;
    let agent = matches!(
        showing,
        Showing::Thread | Showing::Sessions | Showing::Prompt
    )
    .then(|| world.get_resource::<Convo>().map(|c| c.agent))
    .flatten();
    let sheet = world.get_resource::<Sheet>().copied()?;
    Some(Visit {
        showing,
        agent,
        sheet,
    })
}

/// Keeps the page left behind whenever the Hub turns to another.
fn record(world: &mut World) {
    if let Some(visit) = here(world) {
        world.resource_mut::<Visits>().0.keep(visit);
    }
}

/// Whether a page there still has what it shows.
fn stands(world: &World, visit: &Visit) -> bool {
    let alive = |entity: Entity| world.get_entity(entity).is_ok();
    let shown = match visit.showing {
        Showing::Scholar(e)
        | Showing::Hamlet(e)
        | Showing::Gone(e)
        | Showing::Chief(e) => alive(e),
        _ => true,
    };
    shown && visit.agent.is_none_or(alive)
}

/// What is selected in the world while `visit` is shown, if the page
/// has a thing of its own.
fn thing_of(world: &World, visit: &Visit) -> Option<Option<Thing>> {
    Some(match visit.showing {
        Showing::Realm => None,
        Showing::Thread => {
            Some(crate::convo::home_of(world, visit.agent?))
        }
        Showing::Scholar(scholar) => Some(Thing::Scholar(scholar)),
        Showing::Hamlet(hamlet) => Some(Thing::Common(hamlet)),
        Showing::Gone(agent) => Some(Thing::Ruin(agent)),
        Showing::Chief(chief) => Some(Thing::Chief(chief)),
        Showing::Library => Some(Thing::Library),
        Showing::Sessions | Showing::Prompt => return None,
    })
}

/// Turns the Hub to the page behind, or the one ahead.
fn travel(world: &mut World, way: Way) {
    let Some(now) = here(world) else {
        return;
    };
    let mut trail =
        core::mem::take(&mut world.resource_mut::<Visits>().0);
    let next = trail.travel(way, now, |visit| stands(world, visit));
    world.resource_mut::<Visits>().0 = trail;
    let Some(next) = next else {
        return;
    };
    // The world's ring moves with it. A page of an agent's sessions
    // or prompt leaves the ring where it is, as selecting the agent
    // would turn the Hub to its chat.
    if let Some(thing) = thing_of(world, &next)
        && world.resource::<Selected>().0 != thing
    {
        select(world, thing);
    }
    if let Some(agent) = next.agent
        && world.resource::<Convo>().agent != agent
    {
        seat(world, agent);
    }
    if *world.resource::<Sheet>() != next.sheet {
        *world.resource_mut::<Sheet>() = next.sheet;
    }
    show(world, next.showing);
}

/// Turns the Hub by the mouse's back or forward button, when the
/// pointer is over it.
fn mouse_travel(world: &mut World) {
    if let Some(way) = mouse_way(world)
        && pointer_over(world, KEYS)
    {
        travel(world, way);
    }
}

#[cfg(test)]
mod tests {
    use hamlet_client::realm;

    use super::*;
    use crate::testing::panel_app;

    #[test]
    fn back_and_forward_walk_the_pages_gone_through() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        let ada = realm::named(app.world_mut(), "hire#2").unwrap();
        app.insert_resource(Showing::Realm)
            .init_resource::<Sheet>()
            .init_resource::<Selected>()
            .init_resource::<hamlet_realm::pick::Also>();
        app.update();
        crate::convo::talk_to(app.world_mut(), ada);
        show(app.world_mut(), Showing::Thread);
        app.update();
        *app.world_mut().resource_mut::<Sheet>() = Sheet::Details;
        app.update();
        travel(app.world_mut(), Way::Back);
        app.update();
        assert_eq!(*app.world().resource::<Sheet>(), Sheet::Chat);
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Thread
        );
        travel(app.world_mut(), Way::Back);
        app.update();
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Realm
        );
        travel(app.world_mut(), Way::Forward);
        travel(app.world_mut(), Way::Forward);
        app.update();
        assert_eq!(*app.world().resource::<Sheet>(), Sheet::Details);
        assert_eq!(app.world().resource::<Convo>().agent, ada);
    }
}
