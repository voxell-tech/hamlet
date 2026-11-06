//! The Hub's pages gone through, to go back and forward among them:
//! with the primary key and `[` or `]`, or the mouse's back and
//! forward buttons.

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, GLOBAL, Mods, ShortcutAppExt,
};

use super::{Sheet, Showing, show};
use crate::convo::{Convo, seat};

/// How many pages are kept behind.
const KEPT: usize = 50;

pub(super) fn plugin(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id: CommandId(id),
        label,
        scope: GLOBAL,
        run,
        enabled: |_| true,
        repeat: false,
    };
    let primary = |key| Chord {
        key,
        mods: Mods::PRIMARY,
    };
    app.init_resource::<Visits>()
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
pub(crate) struct Visits {
    behind: Vec<Visit>,
    ahead: Vec<Visit>,
    now: Option<Visit>,
}

#[derive(Clone, Copy)]
enum Way {
    Back,
    Forward,
}

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
    let Some(visit) = here(world) else {
        return;
    };
    let mut visits = world.resource_mut::<Visits>();
    match visits.now.replace(visit) {
        Some(was) if was != visit => {
            visits.behind.push(was);
            if visits.behind.len() > KEPT {
                visits.behind.remove(0);
            }
            visits.ahead.clear();
        }
        _ => {}
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

/// Turns the Hub to the page behind, or the one ahead.
fn travel(world: &mut World, way: Way) {
    let Some(now) = here(world) else {
        return;
    };
    let next = loop {
        let mut visits = world.resource_mut::<Visits>();
        let taken = match way {
            Way::Back => visits.behind.pop(),
            Way::Forward => visits.ahead.pop(),
        };
        let Some(visit) = taken else {
            return;
        };
        if stands(world, &visit) {
            break visit;
        }
    };
    let mut visits = world.resource_mut::<Visits>();
    match way {
        Way::Back => visits.ahead.push(now),
        Way::Forward => visits.behind.push(now),
    }
    // Shown as it is, it is not kept again as a page left.
    visits.now = Some(next);
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

fn mouse_pressed(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
) -> bool {
    mouse.is_some_and(|mouse| {
        mouse.any_just_pressed([
            MouseButton::Back,
            MouseButton::Forward,
        ])
    })
}

fn mouse_travel(world: &mut World) {
    let mouse = world.resource::<ButtonInput<MouseButton>>();
    let way = if mouse.just_pressed(MouseButton::Back) {
        Way::Back
    } else {
        Way::Forward
    };
    travel(world, way);
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
        let ada = realm::named(app.world_mut(), "ada@home").unwrap();
        app.insert_resource(Showing::Home).init_resource::<Sheet>();
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
        assert_eq!(*app.world().resource::<Showing>(), Showing::Home);
        travel(app.world_mut(), Way::Forward);
        travel(app.world_mut(), Way::Forward);
        app.update();
        assert_eq!(*app.world().resource::<Sheet>(), Sheet::Details);
        assert_eq!(app.world().resource::<Convo>().agent, ada);
    }
}
