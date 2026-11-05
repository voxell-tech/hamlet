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
use hamlet_realm::pick::{Selected, Thing, focus_on, select};

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
        session: Entity::PLACEHOLDER,
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

/// The chat session this window shows and sends to.
///
/// Until the realm names the Hand, the session and the agent are
/// placeholders.
#[derive(Resource)]
pub struct Convo {
    pub session: Entity,
    /// The name the chief signs in with.
    pub name: String,
    /// The agent the chat is with.
    pub agent: Entity,
    /// That agent's name. `None` for the Hand.
    pub with: Option<String>,
}

impl Convo {
    /// The id the chat's messages name its agent by, even once the
    /// agent has left.
    pub(crate) fn to(
        &self,
        world: &World,
    ) -> Option<protocol::AgentId> {
        Some(world.get::<Identity>(self.agent)?.0)
    }
}

/// The id and name of a living agent, the name `None` for the Hand.
pub(crate) fn address(
    world: &World,
    agent: Entity,
) -> Option<(protocol::AgentId, Option<String>)> {
    let id = agents::id_of(world, agent)?;
    let with = match world.get::<Kind>(agent) {
        Some(Kind(protocol::AgentKind::Hand)) => None,
        _ => Some(agents::name_of(world, agent)),
    };
    Some((id, with))
}

fn watch(world: &mut World, to: protocol::AgentId) -> Entity {
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
    to: protocol::AgentId,
    with: Option<String>,
) {
    let session = watch(world, to);
    let mut convo = world.resource_mut::<Convo>();
    convo.session = session;
    convo.agent = agent;
    convo.with = with;
}

/// Points the chat at one of the realm's agents, and selects it in
/// the world.
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
    if !same {
        let thing = if hall {
            Thing::Keeper
        } else {
            Thing::Agent(agent)
        };
        select(world, Some(thing));
    }
    hub::show(world, Showing::Thread);
    show_tab(world, HUB);
}

/// Points the chat back at the Hand, and selects its tower and
/// brings the camera to it. The Hub turns to its thread and comes to
/// the front.
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
    focus_on(world, Thing::Hand, false);
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

/// Keeps [`Convo`] true to the agent the chat is with, and to the
/// Hand once that agent is gone.
pub(crate) fn reseat(world: &mut World) {
    let agent = world.resource::<Convo>().agent;
    match address(world, agent) {
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
        session: Entity,
        agent: Entity,
        name: &str,
        with: Option<&str>,
    ) -> Convo {
        Convo {
            session,
            name: name.into(),
            agent,
            with: with.map(str::to_string),
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_fynix::dock::DockTree;

    use super::*;
    use crate::testing::{panel_app, press};

    #[test]
    fn the_first_chat_watches_the_received_hand_and_sends_to_it() {
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
        let convo = app.world().resource::<Convo>();
        assert_eq!(convo.name, "alice");
        let chat = app
            .world()
            .get::<Chat>(convo.session)
            .expect("a watched chat");
        assert_eq!(chat.0.with, protocol::AgentId(1));
        crate::chat::send(app.world_mut(), "Find the key".into());
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|message| serde_json::to_value(message).unwrap())
            .find(|message| message["type"] == "say")
            .unwrap();
        assert_eq!(sent["to"], serde_json::json!(1));
        assert_eq!(sent["text"], "Find the key");
    }

    #[test]
    fn a_menu_seats_the_agents_chat_and_the_hand_takes_it_back() {
        let mut app = panel_app();
        app.init_resource::<crate::world_menu::Open>();
        let ada = agents::named(app.world_mut(), "ada@home").unwrap();
        let hand = agents::hand(app.world()).unwrap();
        let first = app.world().resource::<Convo>().session;
        assert_eq!(app.world().resource::<Convo>().agent, hand);
        *app.world_mut().resource_mut::<crate::world_menu::Open>() =
            crate::world_menu::Open::Menu(
                Thing::Agent(ada),
                bevy::math::Vec2::ZERO,
            );
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            crate::world_menu::view(),
        );
        app.update();
        press(&mut app, "Talk to ada@home");
        let convo = app.world().resource::<Convo>();
        assert_eq!(convo.agent, ada);
        assert_eq!(convo.with.as_deref(), Some("ada@home"));
        assert_ne!(convo.session, first);
        assert_eq!(
            app.world().get::<Chat>(convo.session).unwrap().0.with,
            protocol::AgentId(2)
        );
        let chat = convo.session;
        talk_to_hand(app.world_mut());
        assert_eq!(app.world().resource::<Convo>().session, first);
        talk_to(app.world_mut(), ada);
        assert_eq!(app.world().resource::<Convo>().session, chat);
    }

    #[test]
    fn the_hand_button_seats_the_hand_and_brings_the_hub_forward() {
        use crate::dock::{PETITIONS, designed};

        let mut app = panel_app();
        app.insert_resource(Showing::Home);
        let ada = agents::named(app.world_mut(), "ada@home").unwrap();
        seat(app.world_mut(), ada);
        let mut tree = app.world_mut().resource_mut::<DockTree>();
        designed(&mut tree);
        let side = tree.find_leaf_with_window(HUB).unwrap();
        let petitions = tree
            .leaf(side)
            .unwrap()
            .tabs()
            .find(|(window, _)| *window == PETITIONS)
            .map(|(_, tab)| tab)
            .unwrap();
        tree.set_active(side, petitions);
        talk_to_hand(app.world_mut());
        let hand = agents::hand(app.world()).unwrap();
        assert_eq!(app.world().resource::<Convo>().agent, hand);
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Thread
        );
        let tree = app.world().resource::<DockTree>();
        let front = tree.active(side).unwrap();
        assert_eq!(
            tree.leaf(side).unwrap().tab_index(front),
            Some(0)
        );
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
        state.agents.push(realm::agent(3, "home", 1, "scout"));
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.update();
        let scout =
            agents::named(app.world_mut(), "scout@home").unwrap();
        talk_to(app.world_mut(), scout);
        assert_eq!(
            app.world().resource::<Convo>().with.as_deref(),
            Some("scout@home")
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
        state.agents.push(hamlet_testing::realm::agent(
            4, "mill", 0, "miller",
        ));
        state.agents.push(hamlet_testing::realm::agent(
            5, "mill", 1, "carder",
        ));
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
            agents::named(app.world_mut(), "carder@mill").unwrap();
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
            app.world().resource::<Convo>().name,
            "renamed-chief"
        );
    }
}
