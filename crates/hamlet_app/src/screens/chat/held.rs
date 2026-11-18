//! The line at the end of a stopped chat: inputs waiting, and Resume.

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

use crate::convo::Convo;

/// How many inputs wait in the shown chat, while a stop holds them.
fn held(world: &World) -> Option<usize> {
    let convo = world.get_resource::<Convo>()?;
    if !world.get::<Status>(convo.chat)?.held {
        return None;
    }
    let transcript = world.get::<Transcript>(convo.chat)?;
    Some(transcript.items().iter().filter(|item| item.queued).count())
}

/// "Stopped · 3 waiting" and Resume.
pub(super) fn line() -> impl View {
    with_cx(|cx| {
        let words = derived(|world: &World| match held(world) {
            Some(0) | None => "Stopped".to_string(),
            Some(waiting) => format!("Stopped · {waiting} waiting"),
        })
        .when(every_frame());
        let shown = derived(|world: &World| match held(world) {
            Some(_) => Display::Flex,
            None => Display::None,
        })
        .when(every_frame());
        row((note(words), action(Glyph::Play, "Resume", resume)))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
            .justify(JustifyContent::FlexEnd)
            .display(shown)
    })
}

fn resume(world: &mut World) {
    let Some(agent) = world.get_resource::<Convo>().map(|c| c.agent)
    else {
        return;
    };
    world.trigger(Resume {
        request: RequestId::new(),
        agent,
    });
}
