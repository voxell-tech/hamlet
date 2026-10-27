//! The agent the chat is with, and the ways to point it at another.

use bevy::app::{App, PreUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Added, Changed, Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use hamlet_client::chats::{Chat, Status, Transcript};
use hamlet_client::realm::{
    self as agents, AgentNumber, Chief, HOME, MAYOR, Mayor,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing, select};

use crate::PanelSystems;
use crate::dock::show_tab;

/// Opens the chat with the first mayor.
pub(crate) fn plugin(app: &mut App) {
    let to = protocol::Address {
        hamlet: HOME.into(),
        agent: MAYOR.into(),
    };
    let session = watch(app.world_mut(), to.clone());
    let name = app
        .world()
        .get_resource::<Chief>()
        .map(|chief| chief.0.clone())
        .unwrap_or_default();
    let mayor =
        agents::mayor(app.world_mut()).unwrap_or(Entity::PLACEHOLDER);
    app.insert_resource(Me {
        session,
        to,
        name,
        agent: mayor,
        with: None,
    })
    .add_systems(PreUpdate, chief_name.in_set(PanelSystems::Follow));
}

pub(crate) fn chief_name(
    chief: Option<Res<Chief>>,
    mut me: ResMut<Me>,
) {
    if let Some(chief) = chief
        && me.name != chief.0
    {
        me.name.clone_from(&chief.0);
    }
}

/// The chat session this window shows and sends to.
#[derive(Resource)]
pub struct Me {
    pub session: Entity,
    pub to: protocol::Address,
    /// The name the chief signs in with.
    pub name: String,
    /// The agent the chat is with.
    pub agent: Entity,
    /// That agent's name. `None` for the mayor.
    pub with: Option<String>,
}

/// The address and name of a living agent.
pub(crate) fn address(
    world: &World,
    agent: Entity,
) -> Option<(protocol::Address, Option<String>)> {
    let mut to = agents::address_of(world, agent)?;
    let with = if world.get::<Mayor>(agent).is_some() {
        to.agent = MAYOR.into();
        None
    } else {
        Some(to.agent.clone())
    };
    Some((to, with))
}

fn watch(world: &mut World, to: protocol::Address) -> Entity {
    let reference = protocol::ChatRef {
        kind: None,
        from: None,
        with: to,
        past: None,
    };
    let kept = hamlet_client::chats::find(world, &reference);
    kept.unwrap_or_else(|| {
        world
            .spawn((
                Chat(reference),
                Transcript::default(),
                Status::default(),
            ))
            .id()
    })
}

/// Whether the chat could be pointed at an agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Seating {
    Seated,
    /// It has no address: it left, or the world does not know it.
    Unaddressed,
}

/// The selected agent's watched chat.
pub(crate) fn seat(world: &mut World, agent: Entity) -> Seating {
    let Some((to, with)) = address(world, agent) else {
        return Seating::Unaddressed;
    };
    seat_as(world, agent, to, with);
    Seating::Seated
}

fn seat_as(
    world: &mut World,
    agent: Entity,
    to: protocol::Address,
    with: Option<String>,
) {
    let session = watch(world, to.clone());
    let mut me = world.resource_mut::<Me>();
    me.session = session;
    me.to = to;
    me.agent = agent;
    me.with = with;
}

/// Points the chat at one of the hamlet's agents, and selects it in
/// the world.
pub(crate) fn talk_to(world: &mut World, agent: Entity) {
    if seat(world, agent) == Seating::Unaddressed {
        return;
    }
    let mayor = agents::mayor(world) == Some(agent);
    let selected =
        world.get_resource::<Selected>().map(|selected| selected.0);
    let same = match selected {
        Some(Some(Thing::Keeper | Thing::Cottage)) => mayor,
        Some(Some(Thing::Agent(of) | Thing::House(of))) => {
            of == agent
        }
        Some(_) => false,
        // Nothing can be selected where there is no world.
        None => true,
    };
    if !same {
        let thing = if mayor {
            Thing::Keeper
        } else {
            Thing::Agent(agent)
        };
        select(world, Some(thing));
    }
    show_tab(world, "agent");
}

/// Points the chat back at the mayor.
pub(crate) fn talk_to_mayor(world: &mut World) {
    talk_to_mayor_of(world, HOME);
}

/// The watched chat with the hamlet's office.
pub(crate) fn talk_to_mayor_of(world: &mut World, hamlet: &str) {
    if let Some(mayor) = agents::mayor_of(world, hamlet) {
        seat_as(
            world,
            mayor,
            protocol::Address {
                hamlet: hamlet.into(),
                agent: MAYOR.into(),
            },
            None,
        );
        if world.contains_resource::<Selected>() {
            let thing = if hamlet == HOME {
                Thing::Keeper
            } else {
                Thing::Agent(mayor)
            };
            select(world, Some(thing));
        }
        show_tab(world, "agent");
    }
}

/// Points the chat at the agent at `to`, or at its hamlet's office.
pub(crate) fn talk_at(world: &mut World, to: &protocol::Address) {
    if to.agent == MAYOR {
        talk_to_mayor_of(world, &to.hamlet);
    } else if let Some(agent) = agents::named(world, &to.agent) {
        talk_to(world, agent);
    }
}

/// An agent went, an office changed hands or a name changed.
pub(crate) fn roster_moved(
    named: Query<
        (),
        (With<AgentNumber>, Or<(Added<Mayor>, Changed<Name>)>),
    >,
    mut gone: RemovedComponents<AgentNumber>,
    mut unseated: RemovedComponents<Mayor>,
) -> bool {
    let removed = gone.read().count() + unseated.read().count();
    removed > 0 || !named.is_empty()
}

/// Keeps [`Me`] true to the agent the chat is with, and to the first
/// mayor once that agent is gone.
pub(crate) fn reseat(world: &mut World) {
    let me = world.resource::<Me>();
    let (agent, to) = (me.agent, me.to.clone());
    if to.agent == MAYOR {
        if let Some(mayor) = agents::mayor_of(world, &to.hamlet)
            && mayor != agent
        {
            seat_as(world, mayor, to, None);
        }
    } else {
        match address(world, agent) {
            Some((to, with)) => {
                let me = world.resource::<Me>();
                if me.to != to || me.with != with {
                    seat_as(world, agent, to, with);
                }
            }
            None => {
                if let Some(mayor) = agents::mayor(world) {
                    seat_as(
                        world,
                        mayor,
                        protocol::Address {
                            hamlet: HOME.into(),
                            agent: MAYOR.into(),
                        },
                        None,
                    );
                }
            }
        }
    }
    // An agent that is gone from the world cannot stay selected.
    let gone = match world.get_resource::<Selected>().map(|s| s.0) {
        Some(Some(
            Thing::Agent(agent)
            | Thing::House(agent)
            | Thing::Ruin(agent),
        )) => world.get_entity(agent).is_err(),
        _ => false,
    };
    if gone {
        select(world, None);
    }
}

#[cfg(test)]
impl Me {
    pub(crate) fn of(
        session: Entity,
        agent: Entity,
        name: &str,
        with: Option<&str>,
    ) -> Me {
        Me {
            session,
            to: protocol::Address {
                hamlet: HOME.into(),
                agent: with.unwrap_or(MAYOR).into(),
            },
            name: name.into(),
            agent,
            with: with.map(str::to_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{panel_app, press};

    #[test]
    fn the_first_chat_watches_the_received_mayor_and_sends_to_it() {
        use hamlet_client::chats::Chat;
        use hamlet_client::outbox::Outbox;

        let mut app = App::new();
        app.add_plugins(hamlet_client::ClientPlugin { server: None });
        let mut state = hamlet_testing::realm::state();
        state.chief = "alice".into();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.add_plugins(plugin);
        let me = app.world().resource::<Me>();
        assert_eq!(me.name, "alice");
        let chat = app
            .world()
            .get::<Chat>(me.session)
            .expect("a watched chat");
        assert_eq!(chat.0.with.hamlet, "home");
        assert_eq!(chat.0.with.agent, "mayor");
        crate::chat::send(app.world_mut(), "Find the key".into());
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|message| serde_json::to_value(message).unwrap())
            .find(|message| message["type"] == "say")
            .unwrap();
        assert_eq!(
            sent["to"],
            serde_json::json!({"hamlet": "home", "agent": "mayor"})
        );
        assert_eq!(sent["text"], "Find the key");
    }

    #[test]
    fn the_founders_menu_seats_the_mayors_chat_and_reuses_it() {
        let mut app = panel_app();
        app.init_resource::<crate::world_menu::Open>();
        let founder = agents::mayor(app.world_mut()).unwrap();
        let first = app.world().resource::<Me>().session;
        *app.world_mut().resource_mut::<crate::world_menu::Open>() =
            crate::world_menu::Open::Menu(
                Thing::Agent(founder),
                bevy::math::Vec2::ZERO,
            );
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            crate::world_menu::view(),
        );
        app.update();
        press(&mut app, "Talk to founder");
        let me = app.world().resource::<Me>();
        assert_eq!(me.to.agent, MAYOR);
        assert_eq!(me.to.hamlet, HOME);
        assert_eq!(me.with, None);
        assert_eq!(me.session, first);
        assert_eq!(
            app.world().get::<Chat>(me.session).unwrap().0.with,
            me.to
        );
        let chat = me.session;
        talk_to_mayor(app.world_mut());
        talk_to(app.world_mut(), founder);
        assert_eq!(app.world().resource::<Me>().session, chat);
    }
    #[test]
    fn the_chat_stays_true_as_offices_change_and_agents_go() {
        use bevy::ecs::schedule::IntoScheduleConfigs;
        use hamlet_client::realm::Seen;
        use hamlet_client::receive;
        use hamlet_testing::realm;

        let mut app = panel_app();
        app.add_systems(
            PreUpdate,
            reseat.run_if(roster_moved).in_set(PanelSystems::Follow),
        );
        let mut state = realm::state();
        state.agents.push(realm::agent("home", 1, "scout"));
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.update();
        let scout = agents::named(app.world_mut(), "scout").unwrap();
        talk_to(app.world_mut(), scout);
        let named_chat = app.world().resource::<Me>().session;
        assert_eq!(
            app.world().resource::<Me>().with.as_deref(),
            Some("scout")
        );

        state.agents[0].mayor = false;
        state.agents[1].mayor = true;
        let revision = app.world().resource::<Seen>().revision + 1;
        receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: state.epoch,
                revision,
                changes: state
                    .agents
                    .iter()
                    .cloned()
                    .map(|agent| protocol::Update::Agent { agent })
                    .collect(),
            }),
        );
        app.update();
        let me = app.world().resource::<Me>();
        assert_eq!(me.agent, scout);
        assert_eq!(
            me.to,
            protocol::Address {
                hamlet: HOME.into(),
                agent: MAYOR.into()
            }
        );
        assert_eq!(me.with, None);
        assert_ne!(me.session, named_chat);

        let founder =
            agents::named(app.world_mut(), "founder").unwrap();
        talk_to(app.world_mut(), founder);
        assert_eq!(app.world().resource::<Me>().to.agent, "founder");
        receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: state.epoch,
                revision: revision + 1,
                changes: vec![protocol::Update::AgentGone {
                    agent: state.agents[0].agent.clone(),
                }],
            }),
        );
        app.update();
        let me = app.world().resource::<Me>();
        assert_eq!(me.agent, scout);
        assert_eq!(me.to.agent, MAYOR);
        assert_eq!(me.with, None);
    }

    #[test]
    fn seating_another_hamlets_agent_watches_only_that_agents_chat() {
        use hamlet_client::outbox::Outbox;

        let mut app = panel_app();
        let mut state = hamlet_testing::realm::state();
        state.hamlets.push(hamlet_testing::realm::hamlet("mill"));
        state
            .agents
            .push(hamlet_testing::realm::agent("mill", 0, "miller"));
        state
            .agents
            .push(hamlet_testing::realm::agent("mill", 1, "carder"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        let carder =
            agents::named(app.world_mut(), "carder").unwrap();
        talk_to(app.world_mut(), carder);
        app.update();
        let opens = app
            .world()
            .resource::<Outbox>()
            .iter()
            .filter_map(|message| match message {
                protocol::ClientMessage::Open(open) => {
                    Some(open.chat.with.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            opens,
            [protocol::Address {
                hamlet: "mill".into(),
                agent: "carder".into(),
            }]
        );
    }

    #[test]
    fn the_received_chiefs_name_stays_current() {
        let mut app = panel_app();
        app.update();
        let mut state = hamlet_testing::realm::state();
        state.chief = "renamed-chief".into();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert_eq!(
            app.world().resource::<Me>().name,
            "renamed-chief"
        );
    }
}
