//! The agents' section of the engine's Petitions panel: everything
//! the agents wait on the chief for, oldest first and in full, with
//! the answers each takes, and STOP ALL.

use agents_ui::{
    AgentsWorldExt, Heard, Waits, card, change_view, start,
};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Choice, Need, NeedKind,
};
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, Fill, FrameProps, Size,
    Space, Tone, UiExt, ViewExt, column, each, field, icon_button,
    label, resource, row, set_section,
};
use serde::{Deserialize, Serialize};

/// The engine's Petitions panel.
const PETITIONS: &str = "engine/petitions";

/// The name of the section.
const AGENTS: &str = "agents";

#[derive(Default)]
struct Petitions;

impl Plugin for Petitions {
    fn build(&self, app: &mut App) {
        app.add_plugins(agents_ui::plugin)
            // What waited before a reload is not new.
            .init_saved_resource::<Reason>()
            .init_saved_resource::<Seen>()
            .add_panel_section(PETITIONS, AGENTS, "Agents", section)
            .add_systems(Declare, start(&[AgentsRequest::Needs]))
            .add_systems(Update, receive);
    }
}

hamlet_engine_bevy::export!(Petitions);

/// The reason or reply the next answer carries.
#[derive(Resource, Default, Serialize, Deserialize)]
struct Reason(String);

/// How many waited when the panel last looked.
#[derive(Resource, Default, Serialize, Deserialize)]
struct Seen(usize);

/// Sends the chief's answer to `need`, with the reason typed.
fn answer(world: &mut World, need: u64, choice: Choice) {
    let text = world.resource::<Reason>().0.trim().to_string();
    // A reply is its words.
    if choice == Choice::Reply && text.is_empty() {
        return;
    }
    world.ask(AgentsRequest::Answer { need, choice, text });
    world.resource_mut::<Reason>().0.clear();
}

fn button(need: &Need, choice: Choice) -> AnyView {
    // An edit or a command of a hamlet in manual mode is let go for
    // the agent's session, where another approval lasts.
    let always = match need.kind {
        NeedKind::Approval(danger) if danger.for_session() => {
            "FOR THIS SESSION"
        }
        _ => "ALWAYS ALLOW",
    };
    let (glyph, words, style) = match choice {
        Choice::Allow => {
            ("check", "ALLOW ONCE", ButtonStyle::Primary)
        }
        Choice::Always => ("check_all", always, ButtonStyle::Plain),
        Choice::Deny => ("cross", "DENY", ButtonStyle::Critical),
        Choice::GoAhead => {
            ("check", "GO AHEAD", ButtonStyle::Primary)
        }
        Choice::Withdraw => {
            ("restore", "WITHDRAW", ButtonStyle::Critical)
        }
        Choice::Reply => ("speech", "REPLY", ButtonStyle::Primary),
        Choice::Dismiss => ("cross", "DISMISS", ButtonStyle::Plain),
    };
    let id = need.id;
    icon_button(glyph, words)
        .style(style)
        .on_click(move |world| answer(world, id, choice))
        .boxed()
}

/// One thing that waits: what it is and from whom, its line, what it
/// is about, and its answers.
fn strip(need: &Need) -> AnyView {
    let what = match need.kind {
        NeedKind::Approval(_) => "APPROVAL",
        NeedKind::Objection => "OBJECTION",
        NeedKind::Question => "QUESTION",
        NeedKind::Fault => "FAULT",
        NeedKind::Lapsed => "UNANSWERED",
    };
    let asked = match need.asked {
        1 => String::new(),
        n => format!(", asked {n} times"),
    };
    let tone = match need.kind {
        NeedKind::Fault => Tone::Critical,
        _ => Tone::Accent,
    };
    let mut parts = vec![
        label(format!(
            "{what} FROM {}  (waited {}{asked})",
            need.by.to_uppercase(),
            need.waited
        ))
        .size(Size::Small)
        .bold(true)
        .face(Face::Display)
        .tone(tone)
        .boxed(),
    ];
    match (&need.questions, &need.change) {
        // A form says its questions itself, and has its own answers.
        (Some(_), _) => parts.push(card(need)),
        (None, change) => {
            parts.push(label(need.line.clone()).boxed());
            parts.push(match change {
                Some(change) => change_view(
                    format!("need/{}", need.id),
                    change,
                    None,
                ),
                // The exact text, which the line may cut.
                None => label(need.about.clone())
                    .size(Size::Small)
                    .tone(Tone::Dim)
                    .boxed(),
            });
            let answers = need
                .answers
                .iter()
                .take(3)
                .map(|choice| button(need, *choice))
                .collect::<Vec<_>>();
            parts.push(row(answers).space(Space::Half).boxed());
        }
    }
    column(parts).space(Space::Half).fill(Fill::Card).boxed()
}

// The panel scrolls, and says how many wait.
fn section() -> AnyView {
    column((
        field(resource::<Reason, _>(|reason| reason.0.clone()))
            .placeholder("Reason or reply")
            .on_commit(|world, text| {
                world.resource_mut::<Reason>().0 = text
            }),
        // The stop sits under the field, so a narrow panel never
        // pushes it past its edge.
        row((
            label("Reason or reply, then answer")
                .size(Size::Small)
                .tone(Tone::Dim),
            icon_button("stop", "STOP ALL")
                .style(ButtonStyle::Critical)
                .on_click(|world| {
                    world.ask(AgentsRequest::Veto(None))
                }),
        ))
        .space(Space::Gap),
        each(
            resource::<Waits, _>(|waits| waits.0.clone()),
            |need: &Need| need.clone(),
            strip,
        )
        .within(column(()).space(Space::Gap)),
    ))
    .space(Space::Gap)
    .boxed()
}

fn receive(
    world: &mut World,
    mut cursor: Local<MessageCursor<Heard>>,
) {
    for told in world.heard(&mut cursor) {
        if let Heard::Reply(_, AgentsReply::Needs(_))
        | Heard::Event(AgentsEvent::Needs(_)) = &told
        {
            // Something new that waits brings the panel to the front,
            // so it is not missed.
            let now = world.resource::<Waits>().0.len();
            set_section(
                AGENTS,
                u32::try_from(now).unwrap_or(u32::MAX),
            );
            if now > world.resource::<Seen>().0 {
                hamlet_engine_bevy::ui::open(PETITIONS);
            }
            world.resource_mut::<Seen>().0 = now;
        }
        if let Heard::Failed(Some(request), why) = &told {
            hamlet_engine_bevy::warn(&format!("{request:?}: {why}"));
        }
    }
}
