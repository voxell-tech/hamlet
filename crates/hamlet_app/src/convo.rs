//! The agent each Hub's chat is with, and the ways to point it at
//! another.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Added, Changed, Or, With};
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use hamlet_client::chats::{Chat, Status, Transcript};
use hamlet_client::realm::{
    self as agents, Chief, Identity, Kind, Left,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing, bring_into_view, select};

use crate::hub::{self, Showing};

/// The chat a Hub shows and sends to.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Convo {
    pub chat: Entity,
    pub agent: Entity,
    /// The agent's name, `None` for the Hand.
    pub with: Option<String>,
}

impl Default for Convo {
    fn default() -> Self {
        Convo {
            chat: Entity::PLACEHOLDER,
            agent: Entity::PLACEHOLDER,
            with: None,
        }
    }
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

/// `hub`'s chat.
pub(crate) fn of(world: &World, hub: Entity) -> Option<&Convo> {
    world.get::<Convo>(hub)
}

/// The chat `hub` shows, else none.
pub(crate) fn chat_of(world: &World, hub: Entity) -> Entity {
    of(world, hub).map_or(Entity::PLACEHOLDER, |convo| convo.chat)
}

/// The agent `hub`'s chat is with, else none.
pub(crate) fn agent_of(world: &World, hub: Entity) -> Entity {
    of(world, hub).map_or(Entity::PLACEHOLDER, |convo| convo.agent)
}

/// The name the chief signs in with.
pub(crate) fn chief_name(world: &World) -> String {
    world
        .get_resource::<Chief>()
        .map(|chief| chief.0.clone())
        .unwrap_or_default()
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
    // Kept from now on, though a merged conversation brought it.
    if let Some(kept) = kept {
        crate::chat::keep_merged(world, kept);
    }
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

/// Points `hub`'s chat at `agent`.
pub(crate) fn seat(
    world: &mut World,
    hub: Entity,
    agent: Entity,
) -> Seating {
    let Some((to, with)) = id_and_shown(world, agent) else {
        return Seating::Unaddressed;
    };
    let chat = watch(world, to);
    let now = Convo { chat, agent, with };
    match world.get::<Convo>(hub) {
        Some(was) if *was == now => {}
        Some(was) => {
            if was.chat != chat {
                crate::chat::close_merged(world, hub);
            }
            world.entity_mut(hub).insert(now);
        }
        None => {
            world.entity_mut(hub).insert(now);
        }
    }
    Seating::Seated
}

/// `agent`'s house: the hall for the first lead.
pub(crate) fn home_of(world: &World, agent: Entity) -> Thing {
    if agents::first_lead(world) == Some(agent) {
        Thing::Cottage
    } else {
        Thing::House(agent)
    }
}

/// Points the following Hub's chat at `agent` and selects it in the
/// world.
pub(crate) fn talk_to(world: &mut World, agent: Entity) {
    // The tab in front stays for an agent with no address.
    if id_and_shown(world, agent).is_none() {
        return;
    }
    let Some(hub) = hub::bring_following(world) else {
        return;
    };
    if seat(world, hub, agent) == Seating::Unaddressed {
        return;
    }
    let hall = agents::first_lead(world) == Some(agent);
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
    hub::show(world, hub, Showing::Thread);
}

/// Points the following Hub's chat back at the Hand and brings its
/// tower into view.
pub(crate) fn talk_to_hand(world: &mut World) {
    let Some(hand) = agents::hand(world) else {
        return;
    };
    let Some(hub) = hub::bring_following(world) else {
        return;
    };
    seat(world, hub, hand);
    if world.contains_resource::<Selected>() {
        select(world, Some(Thing::Hand));
    }
    hub::show(world, hub, Showing::Thread);
    bring_into_view(world, Thing::Hand);
}

/// Points the following Hub's chat at the agent `to`.
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

/// Keeps each Hub's [`Convo`] on its agent, or on the Hand once it is
/// gone.
pub(crate) fn reseat(world: &mut World) {
    let hubs = world
        .query_filtered::<Entity, With<hub::Hub>>()
        .iter(world)
        .collect::<Vec<_>>();
    for hub in hubs {
        let agent = agent_of(world, hub);
        match id_and_shown(world, agent) {
            Some((_, with)) => {
                if let Some(mut convo) = world.get_mut::<Convo>(hub)
                    && convo.with != with
                {
                    convo.with = with;
                }
            }
            None => {
                if let Some(hand) = agents::hand(world) {
                    seat(world, hub, hand);
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
mod tests {
    use bevy::app::PreUpdate;
    use bevy::ecs::schedule::IntoScheduleConfigs;

    use super::*;
    use crate::PanelSystems;
    use crate::testing::{convo, panel_app};

    #[test]
    fn each_hub_keeps_the_agent_its_chat_is_with() {
        let mut app = panel_app();
        let first = crate::testing::hub(&mut app);
        let second = hub::spawn(
            app.world_mut(),
            "hub-2".into(),
            hub::RailState::Expanded,
        );
        let ada = agents::named(app.world_mut(), "agent#2").unwrap();
        talk_to(app.world_mut(), ada);
        let hand = agents::hand(app.world()).unwrap();
        assert_eq!(agent_of(app.world(), first), ada);
        assert_eq!(agent_of(app.world(), second), hand);
        seat(app.world_mut(), second, ada);
        seat(app.world_mut(), first, hand);
        assert_eq!(agent_of(app.world(), first), hand);
        assert_eq!(agent_of(app.world(), second), ada);
    }

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
        let scout =
            agents::named(app.world_mut(), "agent#3").unwrap();
        talk_to(app.world_mut(), scout);
        assert_eq!(convo(&app).with.as_deref(), Some("agent#3"));
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
        let now = convo(&app);
        assert_eq!(Some(now.agent), agents::hand(app.world()));
        assert_eq!(now.with, None);
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
            agents::named(app.world_mut(), "agent#5").unwrap();
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
