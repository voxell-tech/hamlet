//! The Needs You screen: everything that waits for the chief, oldest
//! first and in full, with the answers each one takes. Questions are
//! answered on their `asking` card.

use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label,
};
use bevy_fynix::{ScopedExt, ViewExt, each, resource};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::messages::{Answer, Veto};
use hamlet_client::waits::{Choice, ChoiceExt, Waiting};
use hamlet_protocol::client::{
    Change, Lasting, WaitDanger, WaitDetail,
};
use hamlet_realm::needs::{TagAnswered, TagOpened};

use crate::dock::{PanelAppExt, lit_panel, show_tab};
use crate::feel::{Lit, Mood};
use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, bad_action, beside, brass, buttons, caption,
    description, framed, grim, lamp, marked, note, scroll_area,
    title,
};
use crate::me::talk_at;
use crate::platform::MonoText;
use crate::{AnyView, Cx, PanelSystems, View, asking, chat, with_cx};

const EMPTY: &str = "Nothing waits for you. Petitions, objections, \
                     questions and faults come here, oldest first.";

/// Seconds between two looks at the time each has waited.
const TICK: f32 = 10.0;

pub fn plugin(app: &mut App) {
    app.add_panel(
        "needs",
        DockWindowKind::new("PETITIONS", || {
            lit_panel(
                screen(),
                Lit {
                    alarm: true,
                    flash: false,
                },
            )
        })
        .icon(Glyph::Bell.image(ICON)),
    )
    .init_resource::<Queue>()
    .add_observer(tag_answered)
    .add_observer(tag_opened)
    .add_systems(PreUpdate, keep_queue.in_set(PanelSystems::Bind))
    .add_systems(Update, show_new);
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
struct Need {
    entity: Entity,
    detail: WaitDetail,
    by: String,
    about: String,
    line: String,
    waited: String,
    asked: u64,
    answers: Vec<Choice>,
    change: Option<Change>,
    chat: Option<hamlet_protocol::client::ChatRef>,
}

impl Need {
    fn of(entity: Entity, waiting: &Waiting) -> Self {
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
            by: waiting.by.clone(),
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
            talk_at(world, &chat.with);
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

/// Brings this screen to the front when something new waits, so it
/// is not missed.
pub(crate) fn show_new(
    waiting: Query<(), With<Waiting>>,
    mut seen: Local<usize>,
    mut commands: Commands,
) {
    let now = waiting.iter().count();
    if now > *seen {
        commands.queue(|world: &mut World| show_tab(world, "needs"));
    }
    *seen = now;
}

/// A click on a tag's paper opens this screen, which has the full
/// text.
pub(crate) fn tag_opened(_: On<TagOpened>, mut commands: Commands) {
    commands.queue(|world: &mut World| show_tab(world, "needs"));
}

fn button(need: &Need, at: usize) -> AnyView {
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
    let (which, text, rules, mood): (_, _, fn(&mut Cx), _) =
        match choice {
            Choice::Allow => (Check, "ALLOW ONCE", lamp, Mood::Good),
            Choice::Always => (CheckAll, always, brass, Mood::Plain),
            Choice::Deny => (Cross, "DENY", grim, Mood::Bad),
            Choice::Go => (Check, "GO AHEAD", lamp, Mood::Good),
            Choice::Withdraw => {
                (Restore, "WITHDRAW", grim, Mood::Bad)
            }
            Choice::Reply => {
                (Talk, "REPLY IN CHAT", lamp, Mood::Good)
            }
            Choice::Dismiss => (Cross, "DISMISS", brass, Mood::Plain),
        };
    let need = need.clone();
    marked(which, text)
        .rules(rules)
        .tagged(mood)
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

fn strip(need: &Need) -> AnyView {
    let need = need.clone();
    AnyView::new(move |cx: &mut Cx| {
        let questions = match &need.detail {
            WaitDetail::Question { questions } => Some(questions),
            _ => None,
        };
        let about = match (questions, held(cx.world, &need)) {
            (Some(asked), _) => {
                asking::card(need.entity, asked.clone())
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
        };
        let asked = match need.asked {
            1 => String::new(),
            n => format!(", asked {n} times"),
        };
        let head = format!(
            "{what} FROM {}  (waited {}{asked})",
            need.by.to_uppercase(),
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

/// The Needs You panel's contents.
pub fn screen() -> impl View {
    with_cx(|cx| {
        let space = &cx.theme().space;
        column((
            title(resource::<Queue, _>(|queue| match queue.len() {
                0 => "NOTHING NEEDS YOU".to_string(),
                1 => "1 THING NEEDS YOU".to_string(),
                n => format!("{n} THINGS NEED YOU"),
            })),
            // On its own row, at its own width, so a narrow panel
            // never pushes it past its edge.
            beside((
                column(()).fills_width(),
                bad_action(Glyph::Stop, "STOP ALL", |world| {
                    world.trigger(Veto { agent: None });
                    world.flush();
                }),
            )),
            description(resource::<Queue, _>(|queue| {
                if queue.is_empty() {
                    EMPTY.to_string()
                } else {
                    String::new()
                }
            })),
            scroll_area((each(
                // The time each has waited is part of the row.
                resource::<Queue, _>(|queue| queue.0.clone()),
                |n: &Need| format!("{n:?}"),
                strip,
            )
            .within(column(()).gap(space.gap)),)),
        ))
        .fills_height()
        .gap(space.section)
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
    fn a_received_approval_offers_its_answers_and_stop_all() {
        let mut app = panel_app();
        let mut state = hamlet_testing::realm::state();
        state.waits.push(hamlet_testing::realm::wait("approve-pwd"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.add_plugins(plugin);
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            screen(),
        );
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text.starts_with("APPROVAL FROM FOUNDER")), "{shown:?}");
        for text in [
            "ALLOW ONCE",
            "FOR THIS SESSION",
            "DENY",
            "1 THING NEEDS YOU",
        ] {
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
        press(&mut app, "STOP ALL");
        let stopped =
            app.world().resource::<Outbox>().iter().any(|message| {
                matches!(message, protocol::ClientMessage::Stop(stop)
                if stop.agent.is_none())
            });
        assert!(stopped);
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
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            screen(),
        );
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "A received change count")
        );
    }
}
