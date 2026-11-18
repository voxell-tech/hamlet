//! What waits for the chief, with the answers each takes.

use std::collections::HashSet;

use bevy::app::{App, PreUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::observer::On;
use bevy::ecs::query::Changed;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
use bevy::ui::{AlignItems, Display, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, frame, label, row,
    scroll,
};
use bevy_fynix::{ViewExt, derived, each, keyed, resource};
use hamlet_client::chats::{Chat, Entry, Transcript};
use hamlet_client::messages::Answer;
use hamlet_client::waits::{Choice, ChoiceExt, RaisedBy, Waiting};
use hamlet_protocol::client::{
    Change, Lasting, Questions, RequestId, WaitDanger, WaitDetail,
};
use hamlet_realm::needs::{TagAnswered, TagOpened};
use hamlet_ui::kinds::card;
use hamlet_ui::styles::{
    Looks, bare, buttons, command_block, marked, marked_wrapping,
    note, status_mark,
};
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::age::Age;
use crate::convo::{Convo, talk_at};
use crate::dock::{HUB, show_tab};
use crate::hub::{Sheet, Showing};
use crate::roll::{either, moved};
use crate::standing::Standing;
use crate::{PanelSystems, asking, chat};

/// Seconds between two refreshes of the time each has waited.
const TICK: f32 = 10.0;

/// The most of the chat's height, in percent, a wait takes before it
/// scrolls.
const TALLEST: f32 = 45.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Queue>()
        .init_resource::<Unfolded>()
        .init_resource::<ChatWaits>()
        .add_observer(tag_answered)
        .add_observer(tag_opened)
        .add_systems(
            PreUpdate,
            (keep_queue, follow).in_set(PanelSystems::Bind),
        );
}

/// Everything that waits for the chief, oldest first.
#[derive(Resource, Default, PartialEq)]
pub struct Queue(Vec<Need>);

impl Queue {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The requests opened in full on the realm's page.
#[derive(Resource, Default)]
struct Unfolded(HashSet<Entity>);

/// What the open chat's agent waits on from the chief, oldest first.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct ChatWaits {
    waits: Vec<Entity>,
    /// The one turned to, else the oldest is shown.
    shown: Option<Entity>,
}

impl ChatWaits {
    pub(crate) fn holds(&self, wait: Entity) -> bool {
        self.waits.contains(&wait)
    }

    /// The wait shown, its place among them and how many there are.
    fn on_show(&self) -> Option<(Entity, usize, usize)> {
        let at = self
            .shown
            .and_then(|shown| {
                self.waits.iter().position(|&wait| wait == shown)
            })
            .unwrap_or(0);
        Some((*self.waits.get(at)?, at, self.waits.len()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Need {
    pub(crate) entity: Entity,
    pub(crate) detail: WaitDetail,
    /// What who raised it is shown by.
    pub(crate) shown: String,
    about: String,
    pub(crate) line: String,
    /// How long it has waited, short: `now`, `5m`.
    pub(crate) waited: String,
    asked: u64,
    pub(crate) answers: Vec<Choice>,
    change: Option<Change>,
    chat: Option<hamlet_protocol::client::ChatRef>,
}

impl Need {
    pub(crate) fn of(entity: Entity, waiting: &Waiting) -> Self {
        let waited =
            Age::of(crate::status::now_ms(), waiting.since).short();
        Self {
            entity,
            detail: waiting.detail.clone(),
            shown: waiting.shown.clone(),
            about: waiting.about.clone(),
            line: waiting.line(80),
            waited,
            asked: waiting.asked,
            answers: waiting.answers.clone(),
            change: waiting.change.clone(),
            chat: waiting.chat.clone(),
        }
    }
}

/// Keeps the [`Queue`] in step with the waits.
fn keep_queue(
    time: Res<Time>,
    waiting: Query<(Entity, &Waiting)>,
    moved: Query<(), Changed<Waiting>>,
    mut answered: RemovedComponents<Waiting>,
    mut queue: ResMut<Queue>,
    mut unfolded: ResMut<Unfolded>,
    mut due: Local<f32>,
) {
    let now = time.elapsed_secs();
    let ticked = !queue.0.is_empty() && now >= *due;
    let lost = answered.read().count() > 0;
    if !lost && moved.is_empty() && !ticked {
        return;
    }
    *due = now + TICK;
    let mut rows = waiting.iter().collect::<Vec<_>>();
    rows.sort_by_key(|(_, w)| (w.since, w.id.as_str()));
    let needs = rows
        .into_iter()
        .map(|(entity, waiting)| Need::of(entity, waiting))
        .collect::<Vec<_>>();
    if queue.0 != needs {
        unfolded.0.retain(|wait| waiting.contains(*wait));
        queue.0 = needs;
    }
}

/// Keeps [`ChatWaits`] in step with the chat.
pub(crate) fn follow(
    convo: Res<Convo>,
    chats: Query<&Chat>,
    waiting: Query<(Entity, &Waiting, Option<&RaisedBy>)>,
    mut chat_waits: ResMut<ChatWaits>,
) {
    let chat = chats.get(convo.chat).ok().map(|chat| &chat.0);
    let mut found = waiting
        .iter()
        .filter(|(_, waits, by)| {
            (waits.chat.is_some() && waits.chat.as_ref() == chat)
                || by.is_some_and(|by| by.0 == convo.agent)
        })
        .map(|(wait, waits, _)| {
            (waits.since, waits.id.as_str(), wait)
        })
        .collect::<Vec<_>>();
    found.sort();
    let waits =
        found.into_iter().map(|(.., wait)| wait).collect::<Vec<_>>();
    let shown =
        chat_waits.shown.filter(|shown| waits.contains(shown));
    if chat_waits.waits != waits || chat_waits.shown != shown {
        *chat_waits = ChatWaits { waits, shown };
    }
}

/// Shows the wait at `at` over the message bar.
fn turn(world: &mut World, at: usize) {
    let mut chat_waits = world.resource_mut::<ChatWaits>();
    if let Some(&wait) = chat_waits.waits.get(at) {
        chat_waits.shown = Some(wait);
    }
}

/// Sends the chief's answer to `need`, or opens its chat for a reply.
fn answer(world: &mut World, need: &Need, choice: Choice) {
    if choice == Choice::Reply {
        if let Some(chat) = &need.chat {
            talk_at(world, chat.with);
        }
        return;
    }
    world.trigger(Answer {
        wait: need.entity,
        request: RequestId::new(),
        choice,
        text: None,
    });
    world.flush();
}

/// Answers a check or a cross on a tag in the world.
pub(crate) fn tag_answered(
    answered: On<TagAnswered>,
    waiting: Query<&Waiting>,
    mut commands: Commands,
) {
    let TagAnswered { item, agree } = *answered.event();
    let choice = waiting
        .get(item)
        .ok()
        .and_then(|need| Choice::quick(&need.answers, agree));
    if let Some(choice) = choice {
        commands.trigger(Answer {
            wait: item,
            request: RequestId::new(),
            choice,
            text: None,
        });
    }
}

/// A click on a tag's paper shows where it is answered in full.
pub(crate) fn tag_opened(
    opened: On<TagOpened>,
    mut commands: Commands,
) {
    let wait = opened.item;
    commands.queue(move |world: &mut World| open(world, wait));
}

/// Turns the Hub to where `wait` is answered.
pub(crate) fn open(world: &mut World, wait: Entity) {
    let Some(waiting) = world.get::<Waiting>(wait) else {
        return;
    };
    let profile = match &waiting.detail {
        WaitDetail::Newcomer { profile } => {
            hamlet_client::realm::profile(world, *profile)
        }
        _ => None,
    };
    let asker = hamlet_client::realm::named(world, &waiting.by);
    show_tab(world, HUB);
    match (profile, asker) {
        (Some(profile), _) => {
            crate::hub::show(world, Showing::Profile(profile));
        }
        (None, Some(agent)) => {
            crate::convo::talk_to(world, agent);
            world.insert_resource(Sheet::Chat);
            world.resource_mut::<ChatWaits>().shown = Some(wait);
        }
        (None, None) => crate::hub::show(world, Showing::Realm),
    }
}

/// The buttons of the answers `need` takes.
pub(crate) fn answers(need: &Need) -> AnyView {
    let (refusing, agreeing): (Vec<usize>, Vec<usize>) =
        (0..need.answers.len()).partition(|&at| {
            matches!(
                need.answers[at],
                Choice::Deny | Choice::Withdraw | Choice::Dismiss
            )
        });
    let mut parts: Vec<AnyView> =
        refusing.into_iter().map(|at| button(need, at)).collect();
    let agreeing: Vec<AnyView> =
        agreeing.into_iter().map(|at| button(need, at)).collect();
    parts.push(buttons(agreeing).boxed());
    buttons(parts).boxed()
}

/// The button for the answer at `at` of `need`, or nothing.
fn button(need: &Need, at: usize) -> AnyView {
    let Some(choice) = need.answers.get(at).cloned() else {
        return frame().boxed();
    };
    let always = lasting_words(need);
    use Glyph::{Check, CheckAll, Cross, Restore, Talk};
    let done = Variant::Tinted(StatusKind::Success);
    let (which, text, variant) = match choice {
        Choice::Allow => (Check, "Allow Once", done),
        Choice::Always => (CheckAll, always, Variant::Default),
        Choice::Deny => (Cross, "Deny", Variant::Danger),
        Choice::Go => (Check, "Go Ahead", done),
        Choice::Withdraw => (Restore, "Withdraw", Variant::Danger),
        Choice::Reply => (Talk, "Reply in Chat", Variant::Primary),
        // A newcomer dismissed waits on at the harbor.
        Choice::Dismiss => match need.detail {
            WaitDetail::Newcomer { .. } => {
                (Restore, "Later", Variant::Default)
            }
            _ => (Cross, "Dismiss", Variant::Default),
        },
        Choice::Welcome => (Check, "Welcome into the City", done),
    };
    let scope = matches!(choice, Choice::Always)
        .then(|| scope(need))
        .flatten();
    let need = need.clone();
    let button = marked_wrapping(which, text)
        .variant(variant)
        .on_activate(move |world| {
            answer(world, &need, choice.clone());
        });
    match scope {
        Some(words) => {
            button.tooltip(move || note(words.clone())).boxed()
        }
        None => button.boxed(),
    }
}

/// The button of the lasting answer of `need`.
fn lasting_words(need: &Need) -> &'static str {
    match need.detail {
        WaitDetail::Approval {
            always: Some(Lasting::Conversation),
            ..
        } => "Allow for This Conversation",
        _ => "Allow Always",
    }
}

/// What the lasting answer of `need` lets go, and for how long.
fn scope(need: &Need) -> Option<String> {
    let WaitDetail::Approval {
        always: Some(lasting),
        danger,
        ..
    } = &need.detail
    else {
        return None;
    };
    let who = &need.shown;
    let lets = match (lasting, danger) {
        (Lasting::Conversation, WaitDanger::Edit) => {
            format!("{who}'s edits go ahead until it ends")
        }
        (Lasting::Conversation, _) => {
            format!("{who}'s commands go ahead until it ends")
        }
        (Lasting::Grant, WaitDanger::Network) => {
            format!("{who} keeps network access until you revoke it")
        }
        (Lasting::Grant, _) => {
            format!("{who} may leave the sandbox until you revoke it")
        }
    };
    Some(format!("{lets}."))
}

/// The edit that waits as `need`, and its change.
fn held(
    world: &World,
    need: &Need,
) -> Option<(Entity, String, Change)> {
    if !matches!(
        need.detail,
        WaitDetail::Approval {
            danger: WaitDanger::Edit,
            ..
        }
    ) {
        return None;
    }
    let waiting = world.get::<Waiting>(need.entity)?;
    let chat = need.chat.as_ref().and_then(|reference| {
        hamlet_client::chats::find(world, reference)
    });
    let open = chat
        .and_then(|chat| world.get::<Transcript>(chat))
        .and_then(|transcript| {
            transcript.items().iter().rev().find(|item| {
                matches!(item.entry, Entry::Tool { exit: None, .. })
            })
        });
    let change =
        need.change.as_ref().or_else(|| match &open?.entry {
            Entry::Tool { change, .. } => change.as_ref(),
            _ => None,
        })?;
    Some((
        chat.unwrap_or(Entity::PLACEHOLDER),
        open.map_or_else(
            || waiting.id.clone(),
            |item| item.id.clone(),
        ),
        change.clone(),
    ))
}

/// The line over the card of `need`, `tail` at its end.
fn head(
    need: &Need,
    profile: &str,
    tail: AnyView,
) -> impl View + use<> {
    let what = match &need.detail {
        WaitDetail::Approval { says, .. } => {
            format!("{} {says}", need.shown)
        }
        WaitDetail::Question { questions } => {
            match questions.questions.len() {
                1 => format!("Request from {}", need.shown),
                n => format!("{n} requests from {}", need.shown),
            }
        }
        WaitDetail::Objection {} => {
            format!("{} objects to your instruction", need.shown)
        }
        WaitDetail::Fault { .. } => {
            format!("{} ran into an error", need.shown)
        }
        WaitDetail::Newcomer { .. } => {
            format!("{profile} wants to move into the city")
        }
    };
    let when = match need.asked {
        1 => need.waited.clone(),
        n => format!("{} · asked {n} times", need.waited),
    };
    let kind = match need.detail {
        WaitDetail::Fault { .. } => StatusKind::Danger,
        _ => Standing::Waits.kind(),
    };
    with_cx(move |cx| {
        row((
            status_mark(Standing::Waits.glyph(), kind, 22.0),
            label(what).size(cx.theme().text.small).bold(true),
            frame().grow(1.0),
            note(when),
            tail,
        ))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
    })
}

/// The card of a request folded to its first question.
fn brief(need: &Need, asked: &Questions) -> AnyView {
    let wait = need.entity;
    let first = asked
        .questions
        .first()
        .map(|one| one.question.clone())
        .unwrap_or_default();
    let more = match asked.questions.len() {
        0 | 1 => frame().boxed(),
        n => note(format!("+{} more", n - 1)).boxed(),
    };
    let head = head(need, "", frame().boxed());
    with_cx(move |cx| {
        let open = marked(Glyph::Talk, "Respond")
            .variant(Variant::Primary)
            .on_activate(move |world| {
                world.resource_mut::<Unfolded>().0.insert(wait);
            });
        card((
            head,
            row((
                row((label(first).size(cx.theme().text.heading),))
                    .min_width(px(0.0)),
                more,
            ))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Baseline),
            row((open,)),
        ))
    })
    .boxed()
}

/// The card of `need` in full, `tail` at the end of its head.
fn strip(need: &Need, tail: AnyView) -> AnyView {
    let need = need.clone();
    AnyView::new(move |cx: &mut Cx| {
        let questions = match &need.detail {
            WaitDetail::Question { questions } => Some(questions),
            _ => None,
        };
        let about = match (questions, held(cx.world, &need)) {
            (Some(asked), _) => {
                asking::card(need.entity, asked.clone()).boxed()
            }
            (None, Some((chat, id, change))) => chat::change_view(
                chat,
                id,
                change,
                chat::Underway::Waiting,
            ),
            // A newcomer's line says it all.
            (None, None)
                if matches!(
                    need.detail,
                    WaitDetail::Newcomer { .. }
                ) =>
            {
                frame().boxed()
            }
            // The exact command, which a tag in the world may cut.
            (None, None)
                if matches!(
                    need.detail,
                    WaitDetail::Approval { .. }
                ) =>
            {
                command_block(need.about.clone()).boxed()
            }
            (None, None) => {
                note(need.about.clone()).tagged(MonoText).boxed()
            }
        };
        // A form has its own answers.
        let answers = match questions {
            Some(_) => frame().boxed(),
            None => answers(&need),
        };
        let profile = match &need.detail {
            WaitDetail::Newcomer { profile } => {
                hamlet_client::realm::profile(cx.world, *profile)
                    .and_then(|profile| {
                        cx.world.get::<hamlet_client::realm::Profile>(
                            profile,
                        )
                    })
                    .map_or_else(
                        || "A new profile".to_string(),
                        |profile| profile.0.name.clone(),
                    )
            }
            _ => String::new(),
        };
        let head = head(&need, &profile, tail);
        // A form says its questions itself.
        let line = match &need.detail {
            WaitDetail::Question { .. }
            | WaitDetail::Approval { .. } => None,
            WaitDetail::Newcomer { .. } => Some(format!(
                "Made by {}, it waits at the harbor until you let \
                 it in.",
                need.shown
            )),
            _ => Some(need.line.clone()),
        };
        let line = match line {
            Some(line) => {
                label(line).size(cx.theme().text.heading).boxed()
            }
            None => frame().boxed(),
        };
        cx.build(card((head, line, about, answers)))
    })
}

/// Everything that waits for the chief, oldest first.
pub fn waiting() -> impl View {
    // The time each has waited is part of the row.
    let rows = derived(|world: &World| {
        let unfolded = world.get_resource::<Unfolded>();
        world
            .resource::<Queue>()
            .0
            .iter()
            .map(|need| {
                let open = unfolded.is_some_and(|open| {
                    open.0.contains(&need.entity)
                });
                (need.clone(), open)
            })
            .collect::<Vec<_>>()
    })
    .when(either(moved::<Queue>(), moved::<Unfolded>()));
    with_cx(move |cx| {
        let space = &cx.theme().space;
        each(
            rows,
            |(need, open): &(Need, bool)| format!("{need:?}{open}"),
            |(need, open)| match (&need.detail, open) {
                (WaitDetail::Question { questions }, false) => {
                    brief(need, questions)
                }
                _ => strip(need, frame().boxed()),
            },
        )
        .within(column(()).gap(space.gap))
    })
}

/// What the agent of the open chat waits on from the chief, in full.
pub fn in_chat() -> impl View {
    let shown =
        resource::<ChatWaits, _>(|chat_waits| {
            match chat_waits.waits.is_empty() {
                true => Display::None,
                false => Display::Flex,
            }
        });
    keyed(resource::<ChatWaits, _>(ChatWaits::on_show), |&on_show| {
        let Some((wait, at, count)) = on_show else {
            return frame().boxed();
        };
        AnyView::new(move |cx: &mut Cx| {
            let Some(waiting) = cx.world.get::<Waiting>(wait) else {
                return cx.build(frame());
            };
            let tail = match count {
                1 => frame().boxed(),
                _ => asking::pager(at, count, turn).boxed(),
            };
            let card = strip(&Need::of(wait, waiting), tail);
            cx.build(scroll((card,)))
        })
    })
    .within(
        bare()
            .shrink(1.0)
            .min_height(px(0.0))
            .max_height(percent(TALLEST))
            .display(shown),
    )
}
