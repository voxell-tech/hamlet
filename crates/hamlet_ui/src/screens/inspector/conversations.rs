//! The selected agent's received letters and questions, watched
//! read-only.

use bevy::ecs::world::World;
use bevy_fynix::views::BehaviorExt;
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::chats::Chat;
use hamlet_client::realm::{PeerChats, address_of, name_of};
use hamlet_protocol::client as protocol;

use super::Section;
use crate::convo::Convo;
use crate::styles::{
    breadcrumb, close, filling, group, lettered, note, scroll_area,
};
use crate::{View, built};

pub(super) fn is_read_only(world: &World) -> bool {
    let convo = world.resource::<Convo>();
    world
        .get::<Chat>(convo.session)
        .is_some_and(|chat| chat.0.from.is_some())
}

fn open(world: &mut World, peer: protocol::ConversationView) {
    let convo = world.resource::<Convo>();
    let reference = protocol::ChatRef {
        with: convo.to.clone(),
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
    *world.resource_mut::<Section>() = Section::Conversation;
}

pub(super) fn chief(world: &mut World) {
    let agent = world.resource::<Convo>().agent;
    crate::convo::seat(world, agent);
    *world.resource_mut::<Section>() = Section::Conversation;
}

fn peers(world: &World) -> Vec<protocol::ConversationView> {
    let agent = world.resource::<Convo>().agent;
    world
        .get::<PeerChats>(agent)
        .map(|chats| chats.0.clone())
        .unwrap_or_default()
}

/// The name of the agent `with` is, or its place in its hamlet while
/// it is not known.
fn name_of_peer(world: &World, with: &protocol::AgentRef) -> String {
    hamlet_client::realm::agent(world, with)
        .filter(|agent| address_of(world, *agent).is_some())
        .map(|agent| name_of(world, agent))
        .unwrap_or_else(|| format!("{}#{}", with.hamlet, with.number))
}

/// Where the chat that is watched read-only is: back to the chief's
/// chat, then the letters or questions it is of.
pub(super) fn path(world: &World) -> impl View + use<> {
    let convo = world.resource::<Convo>();
    let current = world
        .get::<Chat>(convo.session)
        .and_then(|chat| {
            let with = chat.0.from.as_ref()?;
            let kind = match chat.0.kind {
                Some(protocol::ChatRefKind::Questions) => "QUESTIONS",
                _ => "LETTERS",
            };
            let name = name_of_peer(world, with);
            Some(format!("{kind} WITH {}", name.to_uppercase()))
        })
        .unwrap_or_default();
    breadcrumb(vec!["CHIEF CHAT".into()], current, |world, _| {
        chief(world);
    })
}

fn exchange(peer: protocol::ConversationView) -> impl View {
    built(move |cx| {
        let name = name_of_peer(cx.world, &peer.with);
        let kind = match peer.kind {
            protocol::ConversationViewKind::Letters => "LETTERS",
            protocol::ConversationViewKind::Questions => "QUESTIONS",
        };
        let selected = peer.clone();
        let button =
            lettered(format!("{kind} WITH {}", name.to_uppercase()))
                .rules(crate::styles::button)
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

pub(super) fn screen() -> impl View {
    keyed(derived(peers).when(every_frame()), |chats| {
        let mut rows = vec![
            breadcrumb(
                vec!["CHIEF CHAT".into()],
                "AGENT CHATS",
                |world, _| chief(world),
            )
            .boxed(),
        ];
        if chats.is_empty() {
            rows.push(
                note("No conversations with other agents yet.")
                    .boxed(),
            );
        }
        rows.extend(
            chats.iter().cloned().map(|peer| exchange(peer).boxed()),
        );
        scroll_area((group(rows),)).boxed()
    })
    .within(filling())
}
