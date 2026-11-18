//! The rows that are cards of their own and open in place.

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
    LINE, action, command, note, small_rules, text_button,
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

/// How far in the line under a chevron is, and what it holds past it.
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
            Some(Entry::Event(happened)) => cx.build(event(happened)),
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

/// What a block holds while it is open.
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

fn is_open(world: &World, key: &(Entity, String)) -> bool {
    world
        .get_resource::<OpenTools>()
        .is_some_and(|open| open.0.contains(key))
}

/// A centred line between two rules across the chat, over `body`.
fn between(head: AnyView, body: AnyView) -> impl View {
    with_cx(move |cx| {
        let theme = cx.theme();
        let (line, gap) =
            (theme.palette.gold_6, theme.space.gap / 2.0);
        let rule = || {
            frame()
                .height(px(theme.space.border))
                .fill(line)
                .grow(1.0)
                .min_width(px(gap))
        };
        column((
            row((rule(), head, rule()))
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

/// The words of a centred line, after its icon.
fn line_words(mark: Glyph, words: String) -> impl View {
    row((
        glyph(mark, 12.0).tone(Tone::Dim),
        column((
            one_line(label(words).tone(Tone::Dim)).rules(small_rules),
        ))
        .min_width(px(0.0))
        .overflow(Overflow::clip_x()),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .min_width(px(0.0))
}

/// "Context compacted", which opens to the summary.
fn summary(chat: Entity, id: String) -> impl View {
    let key = (chat, id.clone());
    let (turned, pressed) = (key.clone(), key.clone());
    let head = button(
        row((
            chevron(move |world| is_open(world, &turned)),
            line_words(Glyph::History, "Context compacted".into()),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(INSET), px(2.0)))
    .rules(text_button)
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
    between(head.boxed(), body.boxed())
}

/// The model and effort of `model`, "Default" for the realm's.
fn model_words(world: &World, model: &protocol::ItemModel) -> String {
    let effort = model
        .effort
        .map_or("Default", crate::world_menu::model::words);
    format!(
        "{} {effort}",
        crate::models::model_title(world, &model.model)
    )
}

/// `from` and `to` with their shared first words said once.
fn changed(from: &str, to: &str) -> String {
    let (from, to) = (
        from.split(' ').collect::<Vec<_>>(),
        to.split(' ').collect::<Vec<_>>(),
    );
    let shared = from
        .iter()
        .zip(&to)
        .take_while(|(a, b)| a == b)
        .count()
        .min(from.len().saturating_sub(1))
        .min(to.len().saturating_sub(1));
    let mut words = from[..shared].join(" ");
    if !words.is_empty() {
        words.push(' ');
    }
    format!(
        "{words}{} → {}",
        from[shared..].join(" "),
        to[shared..].join(" ")
    )
}

/// Who made a change: "you" for the chief at this window.
fn who(world: &World, by: &str) -> String {
    match by == world.resource::<Convo>().name {
        true => "you".to_string(),
        false => by.to_string(),
    }
}

/// The words and icon of a change to the agent between its turns.
fn event_words(
    world: &World,
    event: &protocol::ItemEvent,
) -> (Glyph, String) {
    use protocol::ItemEvent as Event;
    match event {
        Event::Model { by, from, to } => {
            let words = changed(
                &model_words(world, from),
                &model_words(world, to),
            );
            let by = who(world, by);
            match by.as_str() {
                "you" | "" => (Glyph::Chip, words),
                by => (Glyph::Chip, format!("{words} · by {by}")),
            }
        }
        Event::Stopped { by, waiting } => {
            let by = match who(world, by) {
                by if by.is_empty() => String::new(),
                by => format!(" by {by}"),
            };
            match waiting {
                0 => (Glyph::Stop, format!("Stopped{by}")),
                n => (
                    Glyph::Stop,
                    format!("Stopped{by} · {n} waiting"),
                ),
            }
        }
        Event::Resumed { by } => match who(world, by).as_str() {
            "you" | "" => (Glyph::Play, "Resumed".into()),
            by => (Glyph::Play, format!("Resumed by {by}")),
        },
        Event::Dreamed { summary } if summary.is_empty() => {
            (Glyph::Moon, "Dreamed".into())
        }
        Event::Dreamed { summary } => {
            (Glyph::Moon, format!("Dreamed · {summary}"))
        }
        Event::Profile { .. } => {
            (Glyph::Profile, "Became a profile of the city".into())
        }
        Event::Renamed { name } => {
            (Glyph::Rename, format!("Renamed to {name}"))
        }
        Event::Fast { on: true } => {
            (Glyph::Fast, "Fast tier on".into())
        }
        Event::Fast { on: false } => {
            (Glyph::Fast, "Fast tier off".into())
        }
    }
}

/// A change to the agent between its turns, as a centred line.
fn event(event: protocol::ItemEvent) -> impl View {
    with_cx(move |cx| {
        let (mark, words) = event_words(cx.world, &event);
        between(
            row((line_words(mark, words),))
                .padding(UiRect::axes(px(INSET), px(2.0)))
                .min_width(px(0.0))
                .boxed(),
            frame().boxed(),
        )
    })
}

/// A merged agent's line, which opens to its conversation.
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
    let words = format!("Merged {name} into this chat");
    let head = button(
        row((
            chevron(move |world| open(world, &turned)),
            line_words(Glyph::Merge, words),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(INSET), px(2.0)))
    .rules(text_button)
    .on_activate(move |world| {
        if open(world, &pressed) {
            close_merged(world);
            return;
        }
        // Opened only from the chat itself.
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
    between(head.boxed(), body.boxed())
}

/// The way to earlier rows of the merged conversation.
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

/// The worker the call `id` of `chat` started, and how it stands.
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

/// A worker started on `task`, which opens to the task and answer.
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
