//! The one place to type, under the chat: the card of questions the
//! agent waits on, what the next message replies to, and the field.

use agents_ui::{Focused, Waits, ask, card};
use hamlet_engine_sdk::api::agents::{AgentsRequest, Need};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Size, Space, Tone,
    UiExt, ViewExt, World, column, derived, field, icon, keyed,
    label, pressable, resource, row,
};

use crate::reply::{Answers, REPLY_CHARS, replied};
use crate::timeline::{notice, refresh};

/// What a line that is a command is told.
const NO_COMMANDS: &str = "Commands are not typed here. Use the \
                           menus, or right-click an agent or a \
                           hamlet.";

/// The most lines the field grows to.
const LINES: u32 = 8;

/// What is typed in the field and not yet sent.
#[derive(Default)]
pub struct Draft(pub String);

/// The message the next one sent answers.
#[derive(Default)]
pub struct Replying(pub Option<Answers>);

/// Has the next message reply to `text`, said by `by`.
pub fn reply_to(world: &mut World, by: &str, text: &str) {
    world.resource_mut::<Replying>().0 =
        Some(Answers::of(by, text, REPLY_CHARS));
}

/// Sends `typed` to the open conversation. A command is not sent:
/// the chat says why.
fn send(world: &mut World, typed: String) {
    let typed = typed.trim().to_string();
    if typed.is_empty() {
        return;
    }
    let Some(to) = world
        .resource::<Focused>()
        .0
        .as_ref()
        .map(|focus| focus.with.clone())
    else {
        return;
    };
    world.resource_mut::<Draft>().0.clear();
    if typed.starts_with('/') {
        notice(world, NO_COMMANDS);
        refresh(world);
        return;
    }
    let text = match world.resource_mut::<Replying>().0.take() {
        Some(answers) => replied(&answers, &typed),
        None => typed,
    };
    ask(world, AgentsRequest::Send { to, text });
}

/// The oldest card of questions the open conversation's agent, or a
/// turn of its own session, waits on.
fn waiting(world: &World) -> Option<Need> {
    let focus = world.resource::<Focused>().0.as_ref()?;
    world
        .resource::<Waits>()
        .0
        .iter()
        .find(|need| {
            need.questions.is_some()
                && (need.session.as_ref() == Some(&focus.open.name)
                    || focus.open.agent.as_ref() == Some(&need.by))
        })
        .cloned()
}

/// The card the agent of the open conversation waits on.
fn questions() -> AnyView {
    let mut waits = resource::<Waits, _>(|_| ());
    let mut focused = resource::<Focused, _>(|_| ());
    // Both are asked, so that neither is left unread.
    let moved = move |world: &World| {
        waits.changed(world) | focused.changed(world)
    };
    keyed(derived(waiting).when(moved), |need| match need {
        Some(need) => column((
            row((
                icon("bell").tone(Tone::Accent),
                label("Waiting for your answer")
                    .size(Size::Small)
                    .tone(Tone::Accent),
            ))
            .space(Space::Half),
            card(need),
        ))
        .space(Space::Half)
        .boxed(),
        None => column(()).boxed(),
    })
    .boxed()
}

/// What the next message replies to, with the way not to.
fn preview() -> AnyView {
    keyed(
        resource::<Replying, _>(|replying| replying.0.clone()),
        |answers| match answers {
            Some(answers) => row((
                label(format!("Replying to {}", answers.by))
                    .size(Size::Small)
                    .tone(Tone::Dim),
                pressable((icon("cross"),))
                    .style(ButtonStyle::Quiet)
                    .on_click(|world| {
                        world.resource_mut::<Replying>().0 = None
                    })
                    .tip("Do not reply"),
            ))
            .space(Space::Gap)
            .boxed(),
            None => column(()).boxed(),
        },
    )
    .boxed()
}

/// The questions card, the reply's preview and the field.
pub fn message_bar() -> AnyView {
    column((
        questions(),
        preview(),
        field(resource::<Draft, _>(|draft| draft.0.clone()))
            .placeholder("Message")
            .lines(LINES)
            .on_commit(|world, text| {
                world.resource_mut::<Draft>().0 = text
            })
            .on_submit(send),
    ))
    .space(Space::Half)
    .boxed()
}
