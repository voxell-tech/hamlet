//! The selected agent's received letters and questions, watched
//! read-only.

use bevy::ecs::world::World;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::chats::Chat;
use hamlet_client::realm::{PeerChats, address_of, name_of};
use hamlet_protocol::client as protocol;

use super::Section;
use crate::icons::Glyph;
use crate::looks::{Fills, action, filling, note};
use crate::me::Me;
use crate::{AnyView, View};

pub(super) fn is_read_only(world: &World) -> bool {
    let me = world.resource::<Me>();
    world
        .get::<Chat>(me.session)
        .is_some_and(|chat| chat.0.from.is_some())
}

fn open(world: &mut World, peer: protocol::ConversationView) {
    let me = world.resource::<Me>();
    let reference = protocol::ChatRef {
        with: me.to.clone(),
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
    world.resource_mut::<Me>().session = session;
    *world.resource_mut::<Section>() = Section::Conversation;
}

pub(super) fn chief(world: &mut World) {
    let agent = world.resource::<Me>().agent;
    crate::me::seat(world, agent);
    *world.resource_mut::<Section>() = Section::Conversation;
}

fn peers(world: &World) -> Vec<protocol::ConversationView> {
    let agent = world.resource::<Me>().agent;
    world
        .get::<PeerChats>(agent)
        .map(|chats| chats.0.clone())
        .unwrap_or_default()
}

fn exchange(peer: protocol::ConversationView) -> AnyView {
    AnyView::new(move |cx: &mut crate::Cx| {
        let name = hamlet_client::realm::agent(cx.world, &peer.with)
            .filter(|agent| address_of(cx.world, *agent).is_some())
            .map(|agent| name_of(cx.world, agent))
            .unwrap_or_else(|| {
                format!("{}#{}", peer.with.hamlet, peer.with.number)
            });
        let kind = match peer.kind {
            protocol::ConversationViewKind::Letters => "LETTERS",
            protocol::ConversationViewKind::Questions => "QUESTIONS",
        };
        let selected = peer.clone();
        let button = button(label(format!(
            "{kind} WITH {}",
            name.to_uppercase()
        )))
        .rules(crate::looks::brass)
        .on_activate(move |world| open(world, selected.clone()));
        let last = peer
            .last
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|at| at.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_default();
        cx.build(
            column((
                button,
                note(format!(
                    "{} messages. Last: {last}",
                    peer.count
                )),
            ))
            .gap(4.0),
        )
    })
}

pub(super) fn screen() -> impl View {
    keyed(derived(peers).when(every_frame()), |chats| {
        let mut rows =
            vec![action(Glyph::Back, "CHIEF CHAT", chief).boxed()];
        if chats.is_empty() {
            rows.push(
                note("No conversations with other agents yet.")
                    .boxed(),
            );
        }
        rows.extend(chats.iter().cloned().map(exchange));
        scroll((column(rows).gap(8.0),)).fills_height().boxed()
    })
    .within(filling())
}
