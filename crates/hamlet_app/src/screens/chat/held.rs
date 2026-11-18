//! The line at the end of a stopped chat: inputs waiting, and Resume.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Display, JustifyContent};
use bevy_fynix::views::{FrameProps, row};
use bevy_fynix::{derived, every_frame};
use hamlet_client::chats::{Status, Transcript};
use hamlet_client::messages::Resume;
use hamlet_client::outbox::RequestId;
use hamlet_ui::styles::{action, note};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{View, with_cx};

use crate::convo;

/// How many inputs wait in `hub`'s chat, while a stop holds them.
fn held(world: &World, hub: Entity) -> Option<usize> {
    let chat = convo::of(world, hub)?.chat;
    if !world.get::<Status>(chat)?.held {
        return None;
    }
    let transcript = world.get::<Transcript>(chat)?;
    Some(transcript.items().iter().filter(|item| item.queued).count())
}

/// "Stopped · 3 waiting" and Resume, in `hub`.
pub(super) fn line(hub: Entity) -> impl View {
    with_cx(move |cx| {
        let words =
            derived(move |world: &World| match held(world, hub) {
                Some(0) | None => "Stopped".to_string(),
                Some(waiting) => {
                    format!("Stopped · {waiting} waiting")
                }
            })
            .when(every_frame());
        let shown =
            derived(move |world: &World| match held(world, hub) {
                Some(_) => Display::Flex,
                None => Display::None,
            })
            .when(every_frame());
        let resume = move |world: &mut World| {
            let agent = convo::agent_of(world, hub);
            if agent == Entity::PLACEHOLDER {
                return;
            }
            world.trigger(Resume {
                request: RequestId::new(),
                agent,
            });
        };
        row((note(words), action(Glyph::Play, "Resume", resume)))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
            .justify(JustifyContent::FlexEnd)
            .display(shown)
    })
}
