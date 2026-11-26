//! The Hub's back and forward through the pages gone through.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use hamlet_core::fynix::shortcut::{
    CommandId, CommandSpec, Layer, ScopeId, ScopeSpec, ShortcutAppExt,
};
use hamlet_core::pick::{Selected, Thing, select};
use hamlet_core::ui::widgets::keys::Keys;
use hamlet_core::ui::widgets::trail::{
    Trail, Way, mouse_pressed, mouse_way,
};

use super::{Sheet, Showing, show};
use crate::convo::seat;

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
    let primary = |key| Keys::new(key).primary().chord();
    app.add_scope(ScopeSpec {
        id: KEYS,
        label: "Hub",
        layer: Layer::Panel,
        active: None,
    })
    .add_command(
        command("hub.back", "Back", |world, _| {
            travel_under_pointer(world, Way::Back);
        }),
        &[primary(KeyCode::BracketLeft)],
    )
    .add_command(
        command("hub.forward", "Forward", |world, _| {
            travel_under_pointer(world, Way::Forward);
        }),
        &[primary(KeyCode::BracketRight)],
    )
    .add_command(
        CommandSpec {
            enabled: |world| {
                super::under_pointer(world).is_some_and(|hub| {
                    super::profile::asking(world, hub)
                })
            },
            ..command(
                "hub.submit",
                "Do what the profile's form asks",
                |world, _| {
                    if let Some(hub) = super::under_pointer(world) {
                        super::profile::submit(world, hub);
                    }
                },
            )
        },
        &crate::world_menu::SUBMIT_KEYS,
    )
    .add_systems(
        Update,
        (mouse_travel.run_if(mouse_pressed), record).chain(),
    );
}

/// One page of a Hub.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Visit {
    showing: Showing,
    agent: Option<Entity>,
    sheet: Sheet,
}

/// The pages behind the one a Hub shows and those ahead of it.
#[derive(Component, Default)]
pub(crate) struct Visits(Trail<Visit>);

fn here(world: &World, hub: Entity) -> Option<Visit> {
    let showing = *world.get::<Showing>(hub)?;
    let agent = matches!(showing, Showing::Thread | Showing::Prompt)
        .then(|| crate::convo::of(world, hub).map(|c| c.agent))
        .flatten();
    let sheet = world.get::<Sheet>(hub).copied()?;
    Some(Visit {
        showing,
        agent,
        sheet,
    })
}

/// Keeps the page left behind whenever a Hub turns to another.
fn record(world: &mut World) {
    let Some(mut hubs) =
        world.try_query_filtered::<Entity, With<Visits>>()
    else {
        return;
    };
    for hub in hubs.iter(world).collect::<Vec<_>>() {
        if let Some(visit) = here(world, hub)
            && let Some(mut visits) = world.get_mut::<Visits>(hub)
        {
            visits.0.keep(visit);
        }
    }
}

/// Whether a page there still has what it shows, its chat's agent
/// one that can still be talked to.
fn stands(world: &World, visit: &Visit) -> bool {
    let alive = |entity: Entity| world.get_entity(entity).is_ok();
    let shown = match visit.showing {
        Showing::Profile(e)
        | Showing::Hamlet(e)
        | Showing::Gone(e)
        | Showing::Chief(e) => alive(e),
        _ => true,
    };
    shown
        && visit.agent.is_none_or(|agent| {
            crate::convo::id_and_shown(world, agent).is_some()
        })
}

/// What is selected in the world while `visit` is shown.
fn thing_of(world: &World, visit: &Visit) -> Option<Option<Thing>> {
    Some(match visit.showing {
        Showing::Realm => None,
        Showing::Thread => {
            Some(crate::convo::home_of(world, visit.agent?))
        }
        Showing::Profile(profile) => Some(Thing::Profile(profile)),
        Showing::Hamlet(hamlet) => Some(Thing::Common(hamlet)),
        Showing::Gone(agent) => Some(Thing::Ruin(agent)),
        Showing::Chief(chief) => Some(Thing::Chief(chief)),
        Showing::Library => Some(Thing::Library),
        Showing::Prompt => return None,
    })
}

/// Turns `hub` to the page behind, or the one ahead.
fn travel(world: &mut World, hub: Entity, way: Way) {
    let Some(now) = here(world, hub) else {
        return;
    };
    let Some(mut visits) = world.get_mut::<Visits>(hub) else {
        return;
    };
    let mut trail = core::mem::take(&mut visits.0);
    let next = trail.travel(way, now, |visit| stands(world, visit));
    if let Some(mut visits) = world.get_mut::<Visits>(hub) {
        visits.0 = trail;
    }
    let Some(next) = next else {
        return;
    };
    // Selecting the agent would turn a prompt page to its chat.
    if super::is_following(world, hub)
        && let Some(thing) = thing_of(world, &next)
        && world.resource::<Selected>().0 != thing
    {
        select(world, thing);
    }
    if let Some(agent) = next.agent {
        seat(world, hub, agent);
    }
    if let Some(mut sheet) = world.get_mut::<Sheet>(hub)
        && *sheet != next.sheet
    {
        *sheet = next.sheet;
    }
    // The prompt read last may be another agent's.
    if next.showing == Showing::Prompt {
        super::thread::brief(world, hub);
    }
    show(world, hub, next.showing);
}

fn travel_under_pointer(world: &mut World, way: Way) {
    if let Some(hub) = super::under_pointer(world) {
        travel(world, hub, way);
    }
}

/// Turns the Hub under the pointer by the mouse's back or forward
/// button.
fn mouse_travel(world: &mut World) {
    if let Some(way) = mouse_way(world) {
        travel_under_pointer(world, way);
    }
}

#[cfg(test)]
mod tests {
    use hamlet_core::client::realm;

    use super::*;
    use crate::testing::panel_app;

    #[test]
    fn back_and_forward_walk_the_pages_gone_through() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        let ada = realm::named(app.world_mut(), "agent#2").unwrap();
        app.init_resource::<Selected>()
            .init_resource::<hamlet_core::pick::Also>();
        let hub = crate::testing::hub(&mut app);
        app.update();
        crate::convo::talk_to(app.world_mut(), ada);
        show(app.world_mut(), hub, Showing::Thread);
        app.update();
        *app.world_mut().get_mut::<Sheet>(hub).unwrap() =
            Sheet::Details;
        app.update();
        let sheet =
            |app: &App| *app.world().get::<Sheet>(hub).unwrap();
        let showing =
            |app: &App| *app.world().get::<Showing>(hub).unwrap();
        travel(app.world_mut(), hub, Way::Back);
        app.update();
        assert_eq!(sheet(&app), Sheet::Chat);
        assert_eq!(showing(&app), Showing::Thread);
        travel(app.world_mut(), hub, Way::Back);
        app.update();
        assert_eq!(showing(&app), Showing::Realm);
        travel(app.world_mut(), hub, Way::Forward);
        travel(app.world_mut(), hub, Way::Forward);
        app.update();
        assert_eq!(sheet(&app), Sheet::Details);
        assert_eq!(crate::testing::convo(&app).agent, ada);
    }
}
