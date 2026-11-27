//! The page of the prompt the agent the chat is with would be given.

use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{Overflow, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, frame, label, scroll};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_protocol::client as protocol;
use hamlet_ui::kinds::card;
use hamlet_ui::styles::{
    action, bare, card_name, hairline, scroll_area,
};
use hamlet_ui::widgets::edges::faded;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use super::super::{Showing, show};
use super::{Briefing, Under, aside};
use crate::convo::Convo;

/// Reads the prompt the agent the chat is with would be given now,
/// and turns the page to it.
pub(crate) fn show_prompt(world: &mut World) {
    let convo = world.resource::<Convo>();
    let agent = convo.agent;
    *world.resource_mut::<Briefing>() = Briefing {
        agent: Some(agent),
        ..Briefing::default()
    };
    super::super::papers::prompt(world);
    show(world, Showing::Prompt);
}

/// Leaves the prompt of an agent the chat is no longer with.
pub(crate) fn leave_prompt(
    convo: Res<Convo>,
    briefing: Res<Briefing>,
    mut showing: ResMut<Showing>,
) {
    if *showing == Showing::Prompt
        && briefing.agent != Some(convo.agent)
    {
        *showing = Showing::Thread;
    }
}

/// What heads each section of a prompt, by the server's names.
const SOURCES: [(&str, &str); 10] = [
    ("charter", "Charter"),
    ("machine", "This computer"),
    ("project", "The project"),
    ("character", "Character"),
    ("memory", "Memory"),
    ("skills", "Skills"),
    ("place", "Its place"),
    ("realm", "The realm"),
    ("profiles", "Profiles it may hire"),
    ("errand", "Errand"),
];

/// What heads the section `name`.
fn source(name: &str) -> &str {
    SOURCES
        .iter()
        .find(|(known, _)| *known == name)
        .map_or(name, |(_, heading)| heading)
}

/// Puts the whole prompt on the clipboard.
fn copy_prompt(world: &mut World) {
    let text = world.resource::<Briefing>().whole();
    let Some(mut clipboard) =
        world.get_resource_mut::<bevy::clipboard::Clipboard>()
    else {
        return;
    };
    match clipboard.set_text(text) {
        Ok(()) => world.resource_mut::<Briefing>().copied = true,
        Err(e) => {
            bevy::log::warn!("could not copy the prompt: {e:?}");
        }
    }
}

/// The prompt under a heading for each of its sections, or why it
/// could not be read.
fn prompt_sections(
    prompt: &[protocol::PromptSection],
    failed: Option<&str>,
) -> AnyView {
    if let Some(why) = failed {
        return label(format!("The prompt could not be read: {why}"))
            .tone(Tone::Critical)
            .boxed();
    }
    let sections = prompt
        .iter()
        .map(|section| {
            let name = source(&section.name).to_string();
            // Each line as the model is given it, where Markdown
            // would run them together.
            let text = hamlet_ui::markdown::page(
                &section.text.replace('\n', "  \n"),
                Tone::Body,
            )
            .boxed();
            column((card_name(name, Tone::Body), text))
                .gap(6.0)
                .boxed()
        })
        .collect::<Vec<_>>();
    // One read-only page, a line between each part and the next,
    // clear of the scroll bar, which is drawn over the right edge of
    // the area and dims the letters under it. A line too long for the
    // card scrolls sideways inside its border, which stays.
    with_cx(move |cx| {
        let line = cx.theme().palette.gold_6;
        let gap = cx.theme().space.panel;
        let mut parts = Vec::new();
        for section in sections {
            if !parts.is_empty() {
                parts.push(hairline(line).boxed());
            }
            parts.push(section);
        }
        let page = faded(
            scroll((column(parts).gap(gap),))
                .overflow(Overflow::scroll_x())
                .width(percent(100.0)),
        );
        column((card((page,)),))
            .padding(UiRect::right(px(12.0)))
            .min_width(px(0.0))
    })
    .boxed()
}

/// The way to copy the prompt, once it is read.
pub(super) fn copy() -> impl View {
    keyed(
        resource::<Briefing, _>(|b| (b.prompt.is_empty(), b.copied)),
        |(empty, copied)| match (empty, copied) {
            (true, _) => frame().boxed(),
            (false, true) => {
                action(Glyph::Check, "Copied", copy_prompt)
            }
            (false, false) => {
                action(Glyph::Notes, "Copy", copy_prompt)
            }
        },
    )
    .within(bare())
}

/// The whole of the agent's prompt, part by part.
pub(crate) fn prompt_page() -> impl View {
    let shown = keyed(
        resource::<Briefing, _>(|b| {
            (b.prompt.clone(), b.failed.clone())
        }),
        |(prompt, failed)| prompt_sections(prompt, failed.as_deref()),
    )
    .within(bare());
    aside(|_| vec![Under::Prompt], scroll_area((shown,)))
}
