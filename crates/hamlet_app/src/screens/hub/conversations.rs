//! The selected agent's received letters and questions, watched
//! read-only.

use bevy::ecs::world::World;
use bevy_fynix::views::BehaviorExt;
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::chats::Chat;
use hamlet_client::realm::{PeerChats, id_of, name_of};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{close, filling, group, lettered, note};
use hamlet_ui::{View, built};

use super::{Showing, show};
use crate::convo::Convo;

pub(super) fn is_read_only(world: &World) -> bool {
    let convo = world.resource::<Convo>();
    world
        .get::<Chat>(convo.session)
        .is_some_and(|chat| chat.0.from.is_some())
}

/// Watches the letters or questions `peer` has with the agent the
/// Hub is seated at.
pub(super) fn open(
    world: &mut World,
    peer: protocol::ConversationView,
) {
    let convo = world.resource::<Convo>();
    let Some(with) = convo.to(world) else {
        return;
    };
    let reference = protocol::ChatRef {
        with,
        from: Some(peer.with),
        kind: Some(match peer.kind {
            protocol::ConversationViewKind::Letters => {
                protocol::ChatRefKind::Letters
            }
            protocol::ConversationViewKind::Questions => {
                protocol::ChatRefKind::Questions
            }
        }),
        past: None,
    };
    let kept = hamlet_client::chats::find(world, &reference);
    let session =
        kept.unwrap_or_else(|| world.spawn(Chat(reference)).id());
    world.resource_mut::<Convo>().session = session;
    show(world, Showing::Thread);
}

/// Goes back to the chief's own chat with the agent.
pub(super) fn chief(world: &mut World) {
    let agent = world.resource::<Convo>().agent;
    crate::convo::seat(world, agent);
    show(world, Showing::Thread);
}

fn peers(world: &World) -> Vec<protocol::ConversationView> {
    let agent = world.resource::<Convo>().agent;
    world
        .get::<PeerChats>(agent)
        .map(|chats| chats.0.clone())
        .unwrap_or_default()
}

/// The exchange the chat shown is of, when it is a watched one.
pub(super) fn peer_shown(
    world: &World,
) -> Option<protocol::ConversationView> {
    let convo = world.resource::<Convo>();
    let shown = world.get::<Chat>(convo.session)?.0.clone();
    let from = shown.from?;
    peers(world).into_iter().find(|peer| {
        peer.with == from
            && matches!(
                (&peer.kind, &shown.kind),
                (
                    protocol::ConversationViewKind::Letters,
                    Some(protocol::ChatRefKind::Letters)
                ) | (
                    protocol::ConversationViewKind::Questions,
                    Some(protocol::ChatRefKind::Questions)
                )
            )
    })
}

/// The name of the agent `with` is, or its id while it is not
/// known.
pub(super) fn name_of_peer(
    world: &World,
    with: protocol::AgentId,
) -> String {
    hamlet_client::realm::agent(world, with)
        .filter(|agent| id_of(world, *agent).is_some())
        .map(|agent| name_of(world, agent))
        .unwrap_or_else(|| format!("agent#{}", with.0))
}

fn exchange(peer: protocol::ConversationView) -> impl View {
    built(move |cx| {
        let name = name_of_peer(cx.world, peer.with);
        let kind = match peer.kind {
            protocol::ConversationViewKind::Letters => "LETTERS",
            protocol::ConversationViewKind::Questions => "QUESTIONS",
        };
        let selected = peer.clone();
        let button =
            lettered(format!("{kind} WITH {}", name.to_uppercase()))
                .rules(hamlet_ui::styles::button)
                .on_activate(move |world| {
                    open(world, selected.clone());
                });
        let last = peer
            .last
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|at| at.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_default();
        cx.build(close((
            button,
            note(format!("{} messages. Last: {last}", peer.count)),
        )))
    })
}

/// The letters and questions the agent has had with others, each
/// to be watched.
pub(super) fn list() -> impl View {
    keyed(derived(peers).when(every_frame()), |chats| {
        if chats.is_empty() {
            return note("No conversations with other agents yet.")
                .boxed();
        }
        group(
            chats
                .iter()
                .cloned()
                .map(|peer| exchange(peer).boxed())
                .collect::<Vec<_>>(),
        )
        .boxed()
    })
    .within(filling())
}
