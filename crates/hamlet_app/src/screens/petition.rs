//! What waits for the chief: each wait in full, with the answers it
//! takes, shown in the thread of the agent that asks and all of them
//! on the realm's page. Questions are answered on their `asking`
//! card.

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
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label,
};
use bevy_fynix::{ScopedExt, ViewExt, each, resource};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::messages::Answer;
use hamlet_client::waits::{Choice, ChoiceExt, Waiting};
use hamlet_protocol::client::{
    Change, Lasting, WaitDanger, WaitDetail,
};
use hamlet_realm::needs::{TagAnswered, TagOpened};
use hamlet_ui::styles::{
    self, buttons, caption, danger_button, framed, marked, note,
    primary_button,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::convo::talk_at;
use crate::dock::{HUB, show_tab};
use crate::hub::Showing;
use crate::{PanelSystems, asking, chat};

/// Seconds between two looks at the time each has waited.
const TICK: f32 = 10.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Queue>()
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Need {
    pub(crate) entity: Entity,
    pub(crate) detail: WaitDetail,
    /// What who raised it is shown by.
    pub(crate) shown: String,
    about: String,
    pub(crate) line: String,
    waited: String,
    asked: u64,
    pub(crate) answers: Vec<Choice>,
    change: Option<Change>,
    chat: Option<hamlet_protocol::client::ChatRef>,
}

impl Need {
    pub(crate) fn of(entity: Entity, waiting: &Waiting) -> Self {
        let seconds =
            waiting.waited(crate::status::now_ms()).as_secs();
        let waited = match seconds {
            0..=59 => "under a minute".into(),
            60..=3599 => format!("{} min", seconds / 60),
            _ => format!(
                "{} h {:02} min",
                seconds / 3600,
                seconds % 3600 / 60
            ),
        };
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
pub(crate) fn keep_queue(
    time: Res<Time>,
    waiting: Query<(Entity, &Waiting)>,
    moved: Query<(), Changed<Waiting>>,
    mut answered: RemovedComponents<Waiting>,
    mut queue: ResMut<Queue>,
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

/// The button for the answer at `at` of `need`, or nothing.
pub(crate) fn button(need: &Need, at: usize) -> AnyView {
    let Some(choice) = need.answers.get(at).cloned() else {
        return frame().boxed();
    };
    let always = match need.detail {
        WaitDetail::Approval {
            always: Some(Lasting::Session),
            ..
        } => "FOR THIS SESSION",
        _ => "ALWAYS ALLOW",
    };
    use Glyph::{Check, CheckAll, Cross, Restore, Talk};
    let (which, text, rules, variant): (_, _, fn(&mut Cx), _) =
        match choice {
            Choice::Allow => (
                Check,
                "ALLOW ONCE",
                primary_button,
                Variant::Primary,
            ),
            Choice::Always => {
                (CheckAll, always, styles::button, Variant::Default)
            }
            Choice::Deny => {
                (Cross, "DENY", danger_button, Variant::Danger)
            }
            Choice::Go => {
                (Check, "GO AHEAD", primary_button, Variant::Primary)
            }
            Choice::Withdraw => {
                (Restore, "WITHDRAW", danger_button, Variant::Danger)
            }
            Choice::Reply => (
                Talk,
                "REPLY IN CHAT",
                primary_button,
                Variant::Primary,
            ),
            // A newcomer dismissed waits on at the harbor.
            Choice::Dismiss => match need.detail {
                WaitDetail::Newcomer { .. } => (
                    Restore,
                    "LATER",
                    styles::button,
                    Variant::Default,
                ),
                _ => (
                    Cross,
                    "DISMISS",
                    styles::button,
                    Variant::Default,
                ),
            },
            Choice::Welcome => (
                Check,
                "WELCOME INTO THE CITY",
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
            (None, Some((session, id, change))) => {
                chat::change_view(session, id, change)
            }
            // The exact text, which a tag in the world may cut.
            (None, None) => {
                note(need.about.clone()).tagged(MonoText).boxed()
            }
        };
        // A form has its own answers.
        let answers = match questions {
            Some(_) => frame().boxed(),
            None => buttons(vec![
                button(&need, 0),
                button(&need, 1),
                button(&need, 2),
            ])
            .boxed(),
        };
        let what = match need.detail {
            WaitDetail::Approval { .. } => "APPROVAL",
            WaitDetail::Objection {} => "OBJECTION",
            WaitDetail::Question { .. } => "QUESTION",
            WaitDetail::Fault {} => "FAULT",
            WaitDetail::Newcomer { .. } => "NEWCOMER",
        };
        let asked = match need.asked {
            1 => String::new(),
            n => format!(", asked {n} times"),
        };
        let head = format!(
            "{what} FROM {}  (waited {}{asked})",
            need.shown.to_uppercase(),
            need.waited
        );
        let tone = match need.detail {
            WaitDetail::Fault {} => Tone::Critical,
            _ => Tone::Accent,
        };
        // A form says its questions itself.
        let line = match questions {
            Some(_) => frame().boxed(),
            None => label(need.line.clone())
                .size(cx.theme().text.heading)
                .boxed(),
        };
        cx.build(framed((caption(head, tone), line, about, answers)))
    })
}

/// Everything that waits for the chief, oldest first and in full,
/// with the answers each takes; nothing while nothing waits.
pub fn waiting() -> impl View {
    with_cx(|cx| {
        let space = &cx.theme().space;
        each(
            // The time each has waited is part of the row.
            resource::<Queue, _>(|queue| queue.0.clone()),
            |n: &Need| format!("{n:?}"),
            strip,
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
            shown
                .iter()
                .any(|text| text.starts_with("APPROVAL FROM ADA")),
            "{shown:?}"
        );
        for text in ["ALLOW ONCE", "FOR THIS SESSION", "DENY"] {
            assert!(
                shown.iter().any(|shown| shown == text),
                "missing {text}: {shown:?}"
            );
        }
        press(&mut app, "ALLOW ONCE");
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
