//! What waits for the chief: each wait in full, with the answers it
//! takes, shown in the thread of the agent that asks and all of them
//! on the realm's page. Questions are answered on their `asking`
//! card.

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
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, each};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::messages::Answer;
use hamlet_client::waits::{Choice, ChoiceExt, Waiting};
use hamlet_protocol::client::{
    Change, Lasting, Questions, WaitDanger, WaitDetail,
};
use hamlet_realm::needs::{TagAnswered, TagOpened};
use hamlet_ui::styles::{
    self, buttons, caption, close, command_block, danger_button,
    framed, marked, note, primary_button,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::age::Age;
use crate::convo::talk_at;
use crate::dock::{HUB, show_tab};
use crate::hub::Showing;
use crate::roll::{either, moved};
use crate::{PanelSystems, asking, chat};

/// Seconds between two refreshes of the time each has waited.
const TICK: f32 = 10.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Queue>()
        .init_resource::<Unfolded>()
        .add_observer(tag_answered)
        .add_observer(tag_opened)
        .add_systems(
            PreUpdate,
            keep_queue.in_set(PanelSystems::Bind),
        );
}

/// Everything that waits for the chief, oldest first, as the panels
/// show it.
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

/// The questions opened in full on the realm's page.
#[derive(Resource, Default)]
struct Unfolded(HashSet<Entity>);

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

/// Keeps the [`Queue`] in step with the waits and the time each has
/// waited.
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

/// Sends the chief's answer to `need`. A reply is words, which are
/// written in the chat that asked, so it opens that chat instead.
fn answer(world: &mut World, need: &Need, choice: Choice) {
    if choice == Choice::Reply {
        if let Some(chat) = &need.chat {
            talk_at(world, chat.with);
        }
        return;
    }
    world.trigger(Answer {
        wait: need.entity,
        choice,
        text: None,
    });
    world.flush();
}

/// A check or a cross on a tag in the world answers like a button
/// here.
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

/// Turns the Hub to where `wait` is answered: a newcomer's scholar,
/// the thread of the agent that asks, else the realm's page, which
/// has every wait.
pub(crate) fn open(world: &mut World, wait: Entity) {
    let Some(waiting) = world.get::<Waiting>(wait) else {
        return;
    };
    let scholar = match &waiting.detail {
        WaitDetail::Newcomer { scholar } => {
            hamlet_client::realm::scholar(world, *scholar)
        }
        _ => None,
    };
    let asker = hamlet_client::realm::named(world, &waiting.by);
    show_tab(world, HUB);
    match (scholar, asker) {
        (Some(scholar), _) => {
            crate::hub::show(world, Showing::Scholar(scholar));
        }
        (None, Some(agent)) => crate::convo::talk_to(world, agent),
        (None, None) => crate::hub::show(world, Showing::Realm),
    }
}

/// The buttons of the answers `need` takes, with what the lasting
/// one lets go under them.
pub(crate) fn answers(need: &Need) -> AnyView {
    let scope = match scope(need) {
        Some(words) => note(words).boxed(),
        None => frame().boxed(),
    };
    close((
        buttons(vec![
            button(need, 0),
            button(need, 1),
            button(need, 2),
        ]),
        scope,
    ))
    .boxed()
}

/// The button for the answer at `at` of `need`, or nothing.
fn button(need: &Need, at: usize) -> AnyView {
    let Some(choice) = need.answers.get(at).cloned() else {
        return frame().boxed();
    };
    let always = lasting_words(need);
    use Glyph::{Check, CheckAll, Cross, Restore, Talk};
    let (which, text, rules, variant): (_, _, fn(&mut Cx), _) =
        match choice {
            Choice::Allow => (
                Check,
                "Allow once",
                primary_button,
                Variant::Primary,
            ),
            Choice::Always => {
                (CheckAll, always, styles::button, Variant::Default)
            }
            Choice::Deny => {
                (Cross, "Deny", danger_button, Variant::Danger)
            }
            Choice::Go => {
                (Check, "Go ahead", primary_button, Variant::Primary)
            }
            Choice::Withdraw => {
                (Restore, "Withdraw", danger_button, Variant::Danger)
            }
            Choice::Reply => (
                Talk,
                "Reply in chat",
                primary_button,
                Variant::Primary,
            ),
            // A newcomer dismissed waits on at the harbor.
            Choice::Dismiss => match need.detail {
                WaitDetail::Newcomer { .. } => (
                    Restore,
                    "Later",
                    styles::button,
                    Variant::Default,
                ),
                _ => (
                    Cross,
                    "Dismiss",
                    styles::button,
                    Variant::Default,
                ),
            },
            Choice::Welcome => (
                Check,
                "Welcome into the city",
                primary_button,
                Variant::Primary,
            ),
        };
    let need = need.clone();
    marked(which, text)
        .rules(rules)
        .tagged(variant)
        .on_activate(move |world| {
            answer(world, &need, choice.clone());
        })
        .boxed()
}

/// The button of the lasting answer of `need`.
fn lasting_words(need: &Need) -> &'static str {
    match need.detail {
        WaitDetail::Approval {
            always: Some(Lasting::Session),
            ..
        } => "Allow this session",
        _ => "Allow always",
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
        (Lasting::Session, WaitDanger::Edit) => {
            format!("{who}'s edits go ahead until it ends")
        }
        (Lasting::Session, _) => {
            format!("{who}'s commands go ahead until it ends")
        }
        (Lasting::Grant, WaitDanger::Network) => {
            format!("{who} keeps network access until you revoke it")
        }
        (Lasting::Grant, _) => {
            format!("{who} may leave the sandbox until you revoke it")
        }
    };
    Some(format!("{}: {lets}.", lasting_words(need)))
}

/// The edit that waits as `need`: its entry in the conversation
/// whose turn it holds, and its change.
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
    let session = need.chat.as_ref().and_then(|reference| {
        hamlet_client::chats::find(world, reference)
    });
    let open = session
        .and_then(|session| world.get::<Transcript>(session))
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
        session.unwrap_or(Entity::PLACEHOLDER),
        open.map_or_else(
            || waiting.id.clone(),
            |item| item.id.clone(),
        ),
        change.clone(),
    ))
}

/// The line over the card of `need`: what waits, in its tone, and
/// how long it has waited. A newcomer's is about `scholar`, the one
/// at the harbor.
fn head(need: &Need, scholar: &str) -> impl View + use<> {
    let what = match &need.detail {
        WaitDetail::Approval { says, .. } => {
            format!("{} {says}", need.shown)
        }
        WaitDetail::Question { .. } => {
            format!("{} has a question", need.shown)
        }
        WaitDetail::Objection {} => {
            format!("Objection from {}", need.shown)
        }
        WaitDetail::Fault {} => format!("Fault in {}", need.shown),
        WaitDetail::Newcomer { .. } => {
            format!("{scholar} wants to move into the city")
        }
    };
    let when = match need.asked {
        1 => need.waited.clone(),
        n => format!("{} · asked {n} times", need.waited),
    };
    let tone = match need.detail {
        WaitDetail::Fault {} => Tone::Critical,
        _ => Tone::Accent,
    };
    with_cx(move |cx| {
        row((caption(what, tone), frame().grow(1.0), note(when)))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
    })
}

/// The card of a question folded to its first question, with the
/// button that unfolds it in place.
fn brief(need: &Need, asked: &Questions) -> AnyView {
    let wait = need.entity;
    let first = asked
        .questions
        .first()
        .map(|one| one.question.clone())
        .unwrap_or_default();
    let answer = match asked.questions.len() {
        1 => "Answer".to_string(),
        n => format!("Answer {n} questions"),
    };
    let head = head(need, "");
    with_cx(move |cx| {
        let open = marked(Glyph::Talk, answer)
            .rules(primary_button)
            .tagged(Variant::Primary)
            .on_activate(move |world| {
                world.resource_mut::<Unfolded>().0.insert(wait);
            });
        framed((
            head,
            label(first).size(cx.theme().text.heading),
            row((open,)),
        ))
    })
    .boxed()
}

pub(crate) fn strip(need: &Need) -> AnyView {
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
            (None, Some((session, id, change))) => chat::change_view(
                session,
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
        let scholar = match &need.detail {
            WaitDetail::Newcomer { scholar } => {
                hamlet_client::realm::scholar(cx.world, *scholar)
                    .and_then(|scholar| {
                        cx.world.get::<hamlet_client::realm::Scholar>(
                            scholar,
                        )
                    })
                    .map_or_else(
                        || "A new scholar".to_string(),
                        |scholar| scholar.0.name.clone(),
                    )
            }
            _ => String::new(),
        };
        let head = head(&need, &scholar);
        // A form says its questions itself, and an approval's
        // subject is its head and, in full, its body.
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
        cx.build(framed((head, line, about, answers)))
    })
}

/// Everything that waits for the chief, oldest first, with the
/// answers each takes, a question folded until it is opened; nothing
/// while nothing waits.
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
                _ => strip(need),
            },
        )
        .within(column(()).gap(space.gap))
    })
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;

    use super::*;
    use crate::testing::{panel_app, press, words};

    #[test]
    fn the_tags_check_and_cross_answer_the_received_wait() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state
            .waits
            .push(hamlet_testing::realm::wait("tag-approval"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let world = app.world_mut();
        let wait = world
            .query::<(Entity, &Waiting)>()
            .iter(world)
            .find(|(_, wait)| wait.id == "tag-approval")
            .unwrap()
            .0;
        world.resource_mut::<Outbox>().drain().for_each(drop);
        for (agree, choice) in
            [(true, Choice::Allow), (false, Choice::Deny)]
        {
            world.trigger(TagAnswered { item: wait, agree });
            world.flush();
            let sent = world
                .resource_mut::<Outbox>()
                .drain()
                .collect::<Vec<_>>();
            let [protocol::ClientMessage::Answer(answer)] = &sent[..]
            else {
                panic!("one answer: {sent:?}");
            };
            assert_eq!(answer.wait, "tag-approval");
            assert_eq!(answer.choice, choice);
            assert_eq!(answer.text, None);
        }
    }

    #[test]
    fn a_received_approval_offers_its_answers() {
        let mut app = panel_app();
        crate::testing::seat_ada(&mut app);
        let mut state = hamlet_testing::realm::state();
        state.waits.push(hamlet_testing::realm::wait("approve-pwd"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.add_plugins(plugin);
        app.update();
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            waiting(),
        );
        app.update();
        let shown = words(&mut app);
        assert!(
            shown.iter().any(|text| text == "ada wants to run"),
            "{shown:?}"
        );
        for text in ["Allow once", "Allow this session", "Deny"] {
            assert!(
                shown.iter().any(|shown| shown == text),
                "missing {text}: {shown:?}"
            );
        }
        press(&mut app, "Allow once");
        let allowed =
            app.world().resource::<Outbox>().iter().any(|message| {
                matches!(message, protocol::ClientMessage::Answer(answer)
                if answer.wait == "approve-pwd"
                    && answer.choice == Choice::Allow)
            });
        assert!(allowed);
        let mut state = hamlet_testing::realm::state();
        let mut wait = hamlet_testing::realm::wait("approve-pwd");
        wait.about = "git".into();
        state.waits.push(wait);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert!(words(&mut app).iter().any(|text| text == "git"));
    }
    #[test]
    fn a_received_edits_change_is_shown_without_its_chat_transcript()
    {
        let mut app = panel_app();
        app.add_plugins((chat::plugin, plugin));
        let mut state = hamlet_testing::realm::state();
        let mut wait = hamlet_testing::realm::wait("edit");
        wait.detail = WaitDetail::Approval {
            danger: WaitDanger::Edit,
            says: "wants to edit".into(),
            always: Some(Lasting::Session),
        };
        wait.about = "orchard.md".into();
        wait.change = Some(Change {
            path: "orchard.md".into(),
            created: true,
            lines: vec![],
            total: 0,
            added: 0,
            removed: 0,
            count: "A received change count".into(),
        });
        state.waits.push(wait);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            waiting(),
        );
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "A received change count")
        );
    }
}
