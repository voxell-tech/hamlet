//! The Hamlets stall's purchases: a hamlet founded from its template
//! with all it comes with, then selected and shown in the world.

use bevy::app::{App, PreUpdate};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::time::Time;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Bylaws, HOME, HamletId};
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Thing, focus_on, select};

use crate::PanelSystems;
use crate::bazaar::samples::hamlets::Template;
use crate::bazaar::shelf::{Said, Stock};
use crate::bazaar::{BazaarSettings, traded};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Founding>()
        .add_observer(answered)
        .add_systems(
            PreUpdate,
            seek.run_if(resource_exists::<Seeking>)
                .in_set(PanelSystems::Follow),
        );
}

/// The hamlet being founded, by its name, and the request founding
/// it.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct Founding(pub Option<(String, protocol::RequestId)>);

impl Founding {
    /// True while `name` is being founded.
    pub fn is(&self, name: &str) -> bool {
        self.0
            .as_ref()
            .is_some_and(|(founding, _)| founding == name)
    }
}

/// A hamlet founded whose town the camera waits for, and the time it
/// stops waiting, in seconds since the app began.
#[derive(Resource)]
struct Seeking {
    hamlet: String,
    until: f64,
}

/// True when the realm has the hamlet `name`.
pub(crate) fn owns(world: &World, name: &str) -> bool {
    hamlet_client::realm::hamlet(world, name).is_some()
}

/// The reason no more hamlets can be founded, when the realm holds as
/// many as the home's limits allow.
pub(crate) fn full(world: &World) -> Option<String> {
    let home = hamlet_client::realm::hamlet(world, HOME)?;
    let most = world.get::<Bylaws>(home)?.0.hamlets.most;
    let count =
        world.try_query::<&HamletId>()?.iter(world).count() as i64;
    (count >= most).then(|| {
        format!(
            "Your realm holds as many hamlets as your limits allow, \
             {count} of {most}. Raise Most hamlets in Settings, \
             under Realm, or close a hamlet to make room."
        )
    })
}

/// Says `words` at the Hamlets stall.
fn say(world: &mut World, words: impl Into<String>, wrong: bool) {
    world.resource_mut::<Said<Template>>().say(words, wrong);
}

/// Founds the hamlet `name` from its template.
pub(crate) fn buy(world: &mut World, name: &str) {
    if world.resource::<Founding>().0.is_some() || owns(world, name) {
        return;
    }
    if let Some(why) = full(world) {
        return say(world, why, true);
    }
    let Some(template) = world
        .resource::<Stock<Template>>()
        .get(&name.to_string())
        .copied()
    else {
        return say(world, "That hamlet is no longer offered.", true);
    };
    let request = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::FoundFrom(protocol::FoundFrom {
            id,
            template: template.founded(),
        })
    });
    world.resource_mut::<Founding>().0 =
        Some((name.to_string(), request));
    world.resource_mut::<Said<Template>>().0 = None;
}

/// Selects the hamlet `name`, and brings its town into view once it
/// stands.
pub(crate) fn visit(world: &mut World, name: &str) {
    if let Some(hamlet) = hamlet_client::realm::hamlet(world, name) {
        select(world, Some(Thing::Common(hamlet)));
    }
    let waits = f64::from(world.resource::<BazaarSettings>().seek);
    let now = world
        .get_resource::<Time>()
        .map_or(0.0, Time::elapsed_secs_f64);
    world.insert_resource(Seeking {
        hamlet: name.to_string(),
        until: now + waits,
    });
}

/// Brings the town of the hamlet sought into view once the world has
/// raised it, and stops waiting once it is or the time is up.
fn seek(world: &mut World) {
    let Some(Seeking { hamlet, until }) =
        world.get_resource::<Seeking>()
    else {
        return;
    };
    let now = world
        .get_resource::<Time>()
        .map_or(f64::INFINITY, Time::elapsed_secs_f64);
    let found = hamlet_client::realm::hamlet(world, hamlet);
    let late = now > *until;
    let Some(hamlet) = found else {
        if late {
            world.remove_resource::<Seeking>();
        }
        return;
    };
    let common = Thing::Common(hamlet);
    let stands =
        world.try_query::<&Thing>().is_some_and(|mut things| {
            things.iter(world).any(|t| *t == common)
        });
    if stands {
        if world
            .resource::<hamlet_realm::pick::Selected>()
            .0
            .is_none()
        {
            select(world, Some(common));
        }
        focus_on(world, common, false);
    }
    if stands || late {
        world.remove_resource::<Seeking>();
    }
}

/// Takes in the server's answer to a hamlet founded.
fn answered(response: On<Response>, mut commands: Commands) {
    let response = response.event().clone();
    commands.queue(move |world: &mut World| {
        let Some((name, request)) =
            world.resource::<Founding>().0.clone()
        else {
            return;
        };
        if request != response.to {
            return;
        }
        world.resource_mut::<Founding>().0 = None;
        let Some(template) =
            world.resource::<Stock<Template>>().get(&name).copied()
        else {
            return;
        };
        let refused = match &response.message {
            protocol::ServerMessage::Fault(fault) => {
                Some(fault.text.clone())
            }
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                Some(reply.text.clone())
            }
            _ => None,
        };
        if let Some(why) = refused {
            let words = format!(
                "{} could not be founded: {why}",
                template.title
            );
            return say(world, words, true);
        }
        traded(world);
        let agents = match template.agents.len() {
            1 => "one agent".to_string(),
            count => format!("{count} agents"),
        };
        let words = format!(
            "{} stands in your realm, chief. {} keeps it, with \
             {agents} and all they know.",
            template.title, template.mayor.name,
        );
        say(world, words, false);
        visit(world, &name);
    });
}
