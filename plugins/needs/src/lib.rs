//! The Needs panel: everything the agents wait on the chief for,
//! oldest first and in full, with the answers each takes. It is apart
//! from the engine's Petitions, which ask about what a plugin may do.

use agents_ui::{
    Heard, Waits, ask, card, change_view, hear, init, start,
};
use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Choice, Need, NeedKind,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, FrameProps, Size, Space,
    Tone, UiExt, ViewExt, World, column, each, field, icon_button,
    label, resource, row, scroll,
};
use hamlet_engine_sdk::{Entry, Message, Plugin};

/// The name of the panel.
const NEEDS: &str = "needs";

const EMPTY: &str = "Nothing waits for you. Petitions, objections, \
                     questions and faults come here, oldest first.";

#[derive(Default)]
struct Needs;

/// The reason or reply the next answer carries.
#[derive(Default)]
struct Reason(String);

/// How many waited when the panel last looked.
#[derive(Default)]
struct Seen(usize);

fn prepare(world: &mut World) {
    init(world);
    world.init_resource::<Reason>();
    world.init_resource::<Seen>();
}

/// Sends the chief's answer to `need`, with the reason typed.
fn answer(world: &mut World, need: u64, choice: Choice) {
    let text = world.resource::<Reason>().0.trim().to_string();
    // A reply is its words.
    if choice == Choice::Reply && text.is_empty() {
        return;
    }
    ask(world, AgentsRequest::Answer { need, choice, text });
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
    column(parts).space(Space::Half).boxed()
}

impl Plugin for Needs {
    fn declare(&self, entry: &mut Entry) {
        entry.panel(NEEDS, "Needs");
        start(&[AgentsRequest::Needs]);
    }

    fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
        prepare(world);
        column((
            label(resource::<Waits, _>(|waits| {
                match waits.0.len() {
                    0 => "NOTHING NEEDS YOU".to_string(),
                    1 => "1 THING NEEDS YOU".to_string(),
                    n => format!("{n} THINGS NEED YOU"),
                }
            }))
            .size(Size::Small)
            .bold(true)
            .face(Face::Display)
            .tone(Tone::Faint),
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
                        ask(world, AgentsRequest::Veto(None))
                    }),
            ))
            .space(Space::Gap),
            label(resource::<Waits, _>(|waits| {
                if waits.0.is_empty() {
                    EMPTY.to_string()
                } else {
                    String::new()
                }
            }))
            .tone(Tone::Dim),
            scroll((each(
                resource::<Waits, _>(|waits| waits.0.clone()),
                |need: &Need| need.clone(),
                strip,
            )
            .within(column(()).space(Space::Gap)),)),
        ))
        .space(Space::Gap)
        .boxed()
    }

    fn receive(&mut self, message: Message, world: &mut World) {
        prepare(world);
        let Some(told) = hear(world, message) else {
            return;
        };
        if let Heard::Reply(_, AgentsReply::Needs(_))
        | Heard::Event(AgentsEvent::Needs(_)) = &told
        {
            // Something new that waits brings the panel to the front,
            // so it is not missed.
            let now = world.resource::<Waits>().0.len();
            if now > world.resource::<Seen>().0 {
                hamlet_engine_sdk::ui::open(NEEDS);
            }
            world.resource_mut::<Seen>().0 = now;
        }
        if let Heard::Failed(Some(request), why) = &told {
            hamlet_engine_sdk::warn(&format!("{request:?}: {why}"));
        }
    }
}

hamlet_engine_sdk::export!(Needs);
