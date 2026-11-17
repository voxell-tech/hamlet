//! The rows that are blocks of their own and open in place: the
//! summary that stands for the earlier messages, the conversation of
//! a hire merged in, and a worker the agent started.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Overflow, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, ghost, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::realm::{self as agents, Left};
use hamlet_protocol::client as protocol;
use hamlet_ui::markdown::document;
use hamlet_ui::styles::{
    LINE, action, command, hairline, note, small_rules,
};
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::entry::{OpenTools, toggle};
use super::lines::{
    INSET, MergedLines, close_merged, merged_earlier, merged_rows,
    open_merged,
};
use crate::convo::Convo;
use crate::markdown;

/// How far in from a block's edge the line down from its chevron is,
/// and how far from that line what it holds is.
const GUIDE: f32 = 4.0;

/// The card of the entry `id` of `chat`.
pub(super) fn view(chat: Entity, id: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let entry = cx
            .world
            .get::<Transcript>(chat)
            .and_then(|t| t.item(&id))
            .map(|item| item.entry.clone());
        match entry {
            Some(Entry::Summary(_)) => cx.build(summary(chat, id)),
            Some(Entry::Merged { from, name }) => {
                cx.build(merged(chat, id, from, name))
            }
            Some(Entry::Tool {
                call: protocol::ToolCall::Worker { task },
                ..
            }) => cx.build(worker(chat, id, task)),
            _ => cx.build(frame()),
        }
    })
}

/// A chevron that points down while `open` says the block is open.
fn chevron(
    open: impl Fn(&World) -> bool + Send + Sync + 'static,
) -> impl View {
    glyph(Glyph::Chevron, 10.0).tone(Tone::Dim).rotation(
        derived(
            move |world: &World| {
                if open(world) { 180.0 } else { 90.0 }
            },
        )
        .when(every_frame()),
    )
}

/// What a block holds while it is open, under a line down from its
/// chevron.
fn held(inside: impl View + 'static) -> impl View {
    with_cx(move |cx| {
        let guide = frame()
            .width(px(LINE))
            .shrink(0.0)
            .margin(UiRect::left(px(INSET + GUIDE)))
            .fill(cx.theme().palette.gold_6);
        row((
            guide,
            column((inside,)).gap(6.0).grow(1.0).min_width(px(0.0)),
        ))
        .gap(GUIDE)
        .align(AlignItems::Stretch)
        .width(percent(100.0))
        .min_width(px(0.0))
    })
}

/// Whether the card `key` is open.
fn is_open(world: &World, key: &(Entity, String)) -> bool {
    world
        .get_resource::<OpenTools>()
        .is_some_and(|open| open.0.contains(key))
}

/// A quiet line across the chat where the earlier messages were
/// summarized, which opens to the summary.
fn summary(chat: Entity, id: String) -> impl View {
    let key = (chat, id.clone());
    let (turned, pressed) = (key.clone(), key.clone());
    let head = button(
        row((
            chevron(move |world| is_open(world, &turned)),
            label("Earlier messages summarized")
                .wrap(false)
                .tone(Tone::Dim)
                .rules(small_rules),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(INSET), px(2.0)))
    .rules(ghost)
    .on_activate(move |world| toggle(world, pressed.clone()));
    let body = keyed(
        resource::<OpenTools, _>(move |open| open.0.contains(&key)),
        move |open| match open {
            true => held(document(
                markdown::entry(chat, id.clone()),
                Tone::Dim,
            ))
            .boxed(),
            false => frame().boxed(),
        },
    )
    .within(column(()).width(percent(100.0)).min_width(px(0.0)));
    with_cx(move |cx| {
        let theme = cx.theme();
        let (line, gap) =
            (theme.palette.gold_6, theme.space.gap / 2.0);
        column((
            row((
                hairline(line).grow(1.0),
                head,
                hairline(line).grow(1.0),
            ))
            .gap(gap)
            .align(AlignItems::Center)
            .width(percent(100.0)),
            body,
        ))
        .gap(gap)
        .width(percent(100.0))
        .min_width(px(0.0))
    })
}

/// The conversation of the hire `from`, named `name`, merged into
/// the chat's agent, which opens in place to its rows, read only.
fn merged(
    chat: Entity,
    id: String,
    from: Option<protocol::AgentId>,
    name: String,
) -> impl View {
    let (turned, pressed, shown) = (id.clone(), id.clone(), id);
    let open = move |world: &World, id: &str| {
        world
            .get_resource::<MergedLines>()
            .is_some_and(|merged| merged.is_open(chat, id))
    };
    let words = format!("Merged from {name}");
    let head = button(
        row((
            chevron(move |world| open(world, &turned)),
            glyph(Glyph::Merge, 12.0).tone(Tone::Dim),
            label(words)
                .wrap(false)
                .tone(Tone::Dim)
                .rules(small_rules),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(INSET), px(2.0)))
    .rules(ghost)
    .on_activate(move |world| {
        if open(world, &pressed) {
            close_merged(world);
            return;
        }
        // A conversation merged in is opened only from the chat, not
        // from one merged into it.
        let own = world.resource::<Convo>().chat == chat;
        if let (Some(from), true) = (from, own) {
            open_merged(
                world,
                chat,
                pressed.clone(),
                from,
                name.clone(),
            );
        }
    });
    let body = keyed(
        derived(move |world: &World| open(world, &shown))
            .when(every_frame()),
        |open| match open {
            true => held(column((earlier(), merged_rows())).gap(6.0))
                .boxed(),
            false => frame().boxed(),
        },
    )
    .within(column(()).width(percent(100.0)).min_width(px(0.0)));
    with_cx(move |cx| {
        column((head, body))
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::FlexStart)
            .width(percent(100.0))
            .min_width(px(0.0))
    })
}

/// The way to the rows of the merged conversation before those
/// built.
fn earlier() -> impl View {
    keyed(
        resource::<MergedLines, _>(MergedLines::has_earlier),
        |earlier| match earlier {
            true => command("Earlier", merged_earlier).boxed(),
            false => frame().boxed(),
        },
    )
    .within(column(()).align(AlignItems::FlexStart))
}

/// How a worker an agent started stands.
#[derive(Clone, PartialEq)]
enum Work {
    Running,
    /// Done, with the entry its answer came back in, once it has.
    Done(Option<String>),
    Failed,
}

/// The worker the call `id` of `chat` started while it is in the
/// realm, and how it stands.
fn work(
    world: &World,
    chat: Entity,
    id: &str,
) -> (Option<protocol::AgentId>, Work) {
    let Some(transcript) = world.get::<Transcript>(chat) else {
        return (None, Work::Running);
    };
    let Some(Entry::Tool { exit, agent, .. }) =
        transcript.item(id).map(|item| &item.entry)
    else {
        return (None, Work::Running);
    };
    let worker = *agent;
    if exit.is_some_and(|code| code != 0) {
        return (worker, Work::Failed);
    }
    // Its answer comes back later, as a message from it.
    let answer = worker.and_then(|worker| {
        transcript
            .items()
            .iter()
            .rev()
            .take_while(|item| item.id != id)
            .find(|item| {
                matches!(
                    &item.entry,
                    Entry::Agent { from: Some(from), .. } if *from == worker
                )
            })
            .map(|item| item.id.clone())
    });
    let living = worker
        .and_then(|worker| agents::agent(world, worker))
        .is_some_and(|worker| world.get::<Left>(worker).is_none());
    let work = match (answer, exit) {
        (Some(answer), _) => Work::Done(Some(answer)),
        (None, None) => Work::Running,
        (None, Some(_)) if living => Work::Running,
        (None, Some(_)) => Work::Done(None),
    };
    (worker.filter(|_| living), work)
}

/// A worker the agent started on `task`: how it stands, which opens
/// to the whole task, its answer once it has come, and the way to the
/// worker's own chat.
fn worker(chat: Entity, id: String, task: String) -> impl View {
    let key = (chat, id.clone());
    let (turned, pressed) = (key.clone(), key.clone());
    let stands = {
        let id = id.clone();
        derived(move |world: &World| work(world, chat, &id))
            .when(every_frame())
    };
    let mark = keyed(stands, |(_, work)| {
        let (which, tone, words) = match work {
            Work::Running => (Glyph::Clock, Tone::Warning, "Working"),
            Work::Done(_) => (Glyph::Check, Tone::Success, "Done"),
            Work::Failed => (Glyph::Cross, Tone::Critical, "Failed"),
        };
        with_cx(move |cx| {
            row((
                glyph(which, 12.0).tone(tone),
                label(words)
                    .wrap(false)
                    .tone(Tone::Faint)
                    .rules(small_rules),
            ))
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::Center)
        })
        .boxed()
    })
    .within(row(()));
    let line = row((
        chevron(move |world| is_open(world, &turned)),
        glyph(Glyph::Worker, 12.0).tone(Tone::Dim),
        label("Worker").bold(true).wrap(false).rules(small_rules),
        column((one_line(label(task.clone()).tone(Tone::Dim))
            .rules(small_rules),))
        .grow(1.0)
        .min_width(px(0.0))
        .overflow(Overflow::clip_x()),
        mark,
    ));
    let head = with_cx(move |cx| {
        button(
            line.gap(cx.theme().space.gap)
                .align(AlignItems::Center)
                .width(percent(100.0))
                .min_width(px(0.0)),
        )
        .width(percent(100.0))
        .min_width(px(0.0))
        .padding(UiRect::axes(px(INSET), px(2.0)))
        .rules(ghost)
        .on_activate(move |world| toggle(world, pressed.clone()))
    });
    let body = keyed(
        derived(move |world: &World| {
            is_open(world, &key).then(|| work(world, chat, &key.1))
        })
        .when(every_frame()),
        move |stands| {
            let Some((worker, work)) = stands.clone() else {
                return frame().boxed();
            };
            let mut inside = vec![note(task.clone()).boxed()];
            if let Work::Done(Some(answer)) = work {
                inside.push(
                    document(
                        markdown::entry(chat, answer),
                        Tone::Body,
                    )
                    .boxed(),
                );
            }
            if let Some(worker) = worker {
                inside.push(
                    row((action(
                        Glyph::Talk,
                        "Open Its Chat",
                        move |world| {
                            if let Some(agent) =
                                agents::agent(world, worker)
                            {
                                crate::convo::talk_to(world, agent);
                            }
                        },
                    ),))
                    .boxed(),
                );
            }
            held(column(inside).gap(6.0)).boxed()
        },
    )
    .within(column(()).width(percent(100.0)).min_width(px(0.0)));
    with_cx(move |cx| {
        column((head, body))
            .gap(cx.theme().space.gap / 2.0)
            .width(percent(100.0))
            .min_width(px(0.0))
    })
}
