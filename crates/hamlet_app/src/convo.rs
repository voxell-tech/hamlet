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
    self as agents, Chief, Identity, Kind, Left,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing, bring_into_view, select};

use crate::PanelSystems;
use crate::dock::{HUB, show_tab};
use crate::hub::{self, Showing};

/// Opens the chat with the Hand as soon as the realm names it.
pub(crate) fn plugin(app: &mut App) {
    let name = app
        .world()
        .get_resource::<Chief>()
        .map(|chief| chief.0.clone())
        .unwrap_or_default();
    app.insert_resource(Convo {
        chat: Entity::PLACEHOLDER,
        name,
        agent: Entity::PLACEHOLDER,
        with: None,
    })
    .add_systems(PreUpdate, chief_name.in_set(PanelSystems::Follow));
    if let Some(hand) = agents::hand(app.world()) {
        seat(app.world_mut(), hand);
    }
}

pub(crate) fn chief_name(
    chief: Option<Res<Chief>>,
    mut convo: ResMut<Convo>,
) {
    if let Some(chief) = chief
        && convo.name != chief.0
    {
        convo.name.clone_from(&chief.0);
    }
}

/// The chat this window shows and sends to.
#[derive(Resource)]
pub struct Convo {
    pub chat: Entity,
    /// The name the chief signs in with.
    pub name: String,
    pub agent: Entity,
    /// The agent's name, `None` for the Hand.
    pub with: Option<String>,
}

impl Convo {
    /// The chat's agent id, kept after the agent leaves.
    pub(crate) fn to(
        &self,
        world: &World,
    ) -> Option<protocol::AgentId> {
        Some(world.get::<Identity>(self.agent)?.0)
    }
}

/// A living agent's id and shown name, `None` for the Hand.
pub(crate) fn id_and_shown(
    world: &World,
    agent: Entity,
) -> Option<(protocol::AgentId, Option<String>)> {
    let id = agents::id_of(world, agent)?;
    let with = match world.get::<Kind>(agent) {
        Some(Kind(protocol::AgentKind::Hand)) => None,
        _ => Some(agents::shown_of(world, agent)),
    };
    Some((id, with))
}

fn watch(world: &mut World, to: protocol::AgentId) -> Entity {
    let reference = protocol::ChatRef { with: to };
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

/// Points the chat at `agent`.
pub(crate) fn seat(world: &mut World, agent: Entity) -> Seating {
    let Some((to, with)) = id_and_shown(world, agent) else {
        return Seating::Unaddressed;
    };
    seat_as(world, agent, to, with);
    Seating::Seated
}

fn seat_as(
    world: &mut World,
    agent: Entity,
    to: protocol::AgentId,
    with: Option<String>,
) {
    let chat = watch(world, to);
    if world.resource::<Convo>().chat != chat
        && world.contains_resource::<crate::chat::MergedLines>()
    {
        crate::chat::close_merged(world);
    }
    let mut convo = world.resource_mut::<Convo>();
    convo.chat = chat;
    convo.agent = agent;
    convo.with = with;
}

/// `agent`'s house: the hall for the first mayor.
pub(crate) fn home_of(world: &World, agent: Entity) -> Thing {
    if agents::first_mayor(world) == Some(agent) {
        Thing::Cottage
    } else {
        Thing::House(agent)
    }
}

/// Points the chat at `agent` and selects it in the world.
pub(crate) fn talk_to(world: &mut World, agent: Entity) {
    if seat(world, agent) == Seating::Unaddressed {
        return;
    }
    let hall = agents::first_mayor(world) == Some(agent);
    let selected =
        world.get_resource::<Selected>().map(|selected| selected.0);
    let same = match selected {
        Some(Some(Thing::Keeper | Thing::Cottage)) => hall,
        Some(Some(Thing::Agent(of) | Thing::House(of))) => {
            of == agent
        }
        Some(_) => false,
        // Nothing can be selected where there is no world.
        None => true,
    };
    // The house is easier to see than the robot.
    if !same {
        select(world, Some(home_of(world, agent)));
    }
    hub::show(world, Showing::Thread);
    show_tab(world, HUB);
}

/// Points the chat back at the Hand and brings its tower into view.
pub(crate) fn talk_to_hand(world: &mut World) {
    let Some(hand) = agents::hand(world) else {
        return;
    };
    seat(world, hand);
    if world.contains_resource::<Selected>() {
        select(world, Some(Thing::Hand));
    }
    hub::show(world, Showing::Thread);
    show_tab(world, HUB);
    bring_into_view(world, Thing::Hand);
}

/// Points the chat at the agent `to`.
pub(crate) fn talk_at(world: &mut World, to: protocol::AgentId) {
    if let Some(agent) = agents::agent(world, to) {
        talk_to(world, agent);
    }
}

/// An agent went or left, or its name changed.
pub(crate) fn roster_moved(
    named: Query<
        (),
        (With<Identity>, Or<(Added<Left>, Changed<Name>)>),
    >,
    mut gone: RemovedComponents<Identity>,
) -> bool {
    gone.read().count() > 0 || !named.is_empty()
}

/// Keeps [`Convo`] on its agent, or on the Hand once it is gone.
pub(crate) fn reseat(world: &mut World) {
    let agent = world.resource::<Convo>().agent;
    match id_and_shown(world, agent) {
        Some((_, with)) => {
            if world.resource::<Convo>().with != with {
                world.resource_mut::<Convo>().with = with;
            }
        }
        None => {
            if let Some(hand) = agents::hand(world) {
                seat(world, hand);
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
impl Convo {
    pub(crate) fn of(
        chat: Entity,
        agent: Entity,
        name: &str,
        with: Option<&str>,
    ) -> Convo {
        Convo {
            chat,
            name: name.into(),
            agent,
            with: with.map(str::to_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::panel_app;

    #[test]
    fn the_chat_returns_to_the_hand_when_its_agent_goes() {
        use hamlet_client::realm::Seen;
        use hamlet_client::receive;
        use hamlet_testing::realm;

        let mut app = panel_app();
        app.add_systems(
            PreUpdate,
            reseat.run_if(roster_moved).in_set(PanelSystems::Follow),
        );
        let mut state = realm::state();
        state.agents.push(realm::agent(3, "home", false));
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.update();
        let scout = agents::named(app.world_mut(), "hire#3").unwrap();
        talk_to(app.world_mut(), scout);
        assert_eq!(
            app.world().resource::<Convo>().with.as_deref(),
            Some("hire#3")
        );
        let revision = app.world().resource::<Seen>().revision + 1;
        receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: state.epoch,
                revision,
                changes: vec![protocol::Update::AgentGone {
                    id: protocol::AgentId(3),
                }],
            }),
        );
        app.update();
        let convo = app.world().resource::<Convo>();
        assert_eq!(Some(convo.agent), agents::hand(app.world()));
        assert_eq!(convo.with, None);
    }

    #[test]
    fn seating_another_hamlets_agent_watches_only_that_agents_chat() {
        use hamlet_client::outbox::Outbox;

        let mut app = panel_app();
        let mut state = hamlet_testing::realm::state();
        state.hamlets.push(hamlet_testing::realm::hamlet("mill", 4));
        state
            .agents
            .push(hamlet_testing::realm::agent(4, "mill", true));
        state
            .agents
            .push(hamlet_testing::realm::agent(5, "mill", false));
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
            agents::named(app.world_mut(), "hire#5").unwrap();
        talk_to(app.world_mut(), carder);
        app.update();
        let opens = app
            .world()
            .resource::<Outbox>()
            .iter()
            .filter_map(|message| match message {
                protocol::ClientMessage::Open(open) => {
                    Some(open.chat.with)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(opens, [protocol::AgentId(5)]);
    }
}
