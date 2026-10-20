//! The Needs You screen: everything that waits for the chief, oldest
//! first and in full, with the answers each one takes. Questions are
//! answered on their `asking` card.

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row, scroll,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, each, resource};
use hamlet_client::chats::{Chat, Entry, Transcript};
use hamlet_client::messages::{Answer, Veto};
use hamlet_client::waits::{
    Asks, Choice, ChoiceExt, Danger, Kind, Waiting,
};
use hamlet_protocol::asking::Questions;
use hamlet_protocol::v0::Change;
use hamlet_realm::needs::{TagAnswered, TagOpened};

use crate::dock::{PanelAppExt, lit_panel, show_tab};
use crate::feel::{Lit, Mood};
use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, bad_action, brass, grim, lamp, marked, note, well,
};
use crate::platform::{DisplayText, MonoText, Wraps};
use crate::{AnyView, Cx, View, asking, chat};

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
    .init_resource::<Reason>()
    .init_resource::<Queue>()
    .add_observer(tag_answered)
    .add_observer(tag_opened)
    .add_systems(Update, (show_new, keep_queue));
}

/// The reason or reply the next answer carries.
#[derive(Resource, Default)]
pub struct Reason(String);

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
    kind: Kind,
    by: String,
    about: String,
    line: String,
    waited: String,
    asked: u64,
    answers: Vec<Choice>,
    change: Option<Change>,
    chat: Option<hamlet_protocol::v0::ChatRef>,
    questions: Option<Questions>,
}

impl Need {
    fn of(
        entity: Entity,
        waiting: &Waiting,
        asks: Option<&Asks>,
    ) -> Self {
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
            kind: waiting.kind,
            by: waiting.by.clone(),
            about: waiting.about.clone(),
            line: waiting.line(80),
            waited,
            asked: waiting.asked,
            answers: waiting.answers.clone(),
            change: waiting.change.clone(),
            chat: waiting.chat.clone(),
            questions: asks.map(|asks| asks.0.clone()),
        }
    }
}

/// Keeps the [`Queue`] in step with the waits and the time each has
/// waited.
pub(crate) fn keep_queue(
    time: Res<Time>,
    waiting: Query<(Entity, &Waiting, Option<&Asks>)>,
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
    rows.sort_by_key(|(_, w, _)| (w.since, w.id.as_str()));
    let needs = rows
        .into_iter()
        .map(|(entity, waiting, asks)| {
            Need::of(entity, waiting, asks)
        })
        .collect::<Vec<_>>();
    if queue.0 != needs {
        queue.0 = needs;
    }
}

/// Sends the chief's answer to `entity`, with the typed reason.
fn answer(world: &mut World, entity: Entity, choice: Choice) {
    let text = world.resource::<Reason>().0.trim().to_string();
    // A reply is its words.
    if choice == Choice::Reply && text.is_empty() {
        return;
    }
    world.trigger(Answer {
        wait: entity,
        choice,
        text: (!text.is_empty()).then_some(text),
    });
    world.flush();
    world.resource_mut::<Reason>().0.clear();
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
    mut seen: bevy::ecs::system::Local<usize>,
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
    let entity = need.entity;
    // An edit or a command of a hamlet in manual mode is let go for
    // the agent's session, where another approval lasts.
    let always = match need.kind {
        Kind::Approval(danger) if danger.for_session() => {
            "FOR THIS SESSION"
        }
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
            Choice::Reply => (Talk, "REPLY", lamp, Mood::Good),
            Choice::Dismiss => (Cross, "DISMISS", brass, Mood::Plain),
        };
    marked(which, text)
        .rules(rules)
        .tagged(mood)
        .on_activate(move |world| {
            answer(world, entity, choice.clone())
        })
        .boxed()
}

/// The edit that waits as `need`: its entry in the conversation
/// whose turn it holds, and its change.
fn held(
    world: &World,
    need: &Need,
) -> Option<(Entity, String, Change)> {
    if need.kind != Kind::Approval(Danger::Edit) {
        return None;
    }
    let waiting = world.get::<Waiting>(need.entity)?;
    let session = need.chat.as_ref().and_then(|reference| {
        world
            .try_query::<(Entity, &Chat)>()?
            .iter(world)
            .find(|(_, chat)| &chat.0 == reference)
            .map(|(entity, _)| entity)
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
        let about = match (&need.questions, held(cx.world, &need)) {
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
        let answers = match need.questions {
            Some(_) => frame().boxed(),
            None => row((
                button(&need, 0),
                button(&need, 1),
                button(&need, 2),
            ))
            .gap(8.0)
            .tagged(Wraps)
            .boxed(),
        };
        let what = match need.kind {
            Kind::Approval(_) => "APPROVAL",
            Kind::Objection => "OBJECTION",
            Kind::Question => "QUESTION",
            Kind::Fault => "FAULT",
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
        let tone = match need.kind {
            Kind::Fault => Tone::Critical,
            _ => Tone::Accent,
        };
        // A form says its questions itself.
        let line = match need.questions {
            Some(_) => frame().boxed(),
            None => label(need.line.clone()).size(13.0).boxed(),
        };
        cx.build(
            column((
                label(head)
                    .size(11.0)
                    .bold(true)
                    .tone(tone)
                    .tagged(DisplayText),
                line,
                about,
                answers,
            ))
            .gap(6.0)
            .padding(UiRect::axes(px(14.0), px(10.0)))
            .border(1.0)
            .border_color(cx.theme().palette.brass_6)
            .radius(cx.theme().space.round)
            .fill(cx.theme().palette.card),
        )
    })
}

/// The Needs You panel's contents.
pub fn screen() -> impl View {
    column((
        row((label(resource::<Queue, _>(|queue| {
            match queue.len() {
                0 => "NOTHING NEEDS YOU".to_string(),
                1 => "1 THING NEEDS YOU".to_string(),
                n => format!("{n} THINGS NEED YOU"),
            }
        }))
        .size(12.0)
        .bold(true)
        .tone(Tone::Faint)
        .tagged(DisplayText),))
        .align(AlignItems::Center),
        text_field(
            resource::<Reason, _>(|r| r.0.clone()),
            |world, text| world.resource_mut::<Reason>().0 = text,
        )
        .rules(well),
        // The stop sits under the field, so a narrow panel never
        // pushes it past its edge.
        row((
            column((note("Reason or reply, then answer"),))
                .fills_width(),
            bad_action(Glyph::Stop, "STOP ALL", |world| {
                world.trigger(Veto { agent: None });
                world.flush();
            }),
        ))
        .gap(12.0)
        .align(AlignItems::Center),
        label(resource::<Queue, _>(|queue| {
            if queue.is_empty() {
                EMPTY.to_string()
            } else {
                String::new()
            }
        }))
        .tone(Tone::Dim),
        scroll((each(
            // The time each has waited is part of the row.
            resource::<Queue, _>(|queue| queue.0.clone()),
            |n: &Need| format!("{n:?}"),
            strip,
        )
        .within(column(()).gap(8.0)),))
        .fills_height(),
    ))
    .fills_height()
    .gap(12.0)
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::v0;

    use super::*;
    use crate::me::tests::{panel_app, press, words};

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
            v0::ServerMessage::State(state),
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
            let [v0::ClientMessage::Answer(answer)] = &sent[..]
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
            v0::ServerMessage::State(state),
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
                matches!(message, v0::ClientMessage::Answer(answer)
                if answer.wait == "approve-pwd"
                    && answer.choice == Choice::Allow)
            });
        assert!(allowed);
        press(&mut app, "STOP ALL");
        let stopped =
            app.world().resource::<Outbox>().iter().any(|message| {
                matches!(message, v0::ClientMessage::Stop(stop)
                if stop.agent.is_none())
            });
        assert!(stopped);
        let mut state = hamlet_testing::realm::state();
        let mut wait = hamlet_testing::realm::wait("approve-pwd");
        if let v0::Wait::Approval { about, .. } = &mut wait {
            *about = "git".into();
        }
        state.waits.push(wait);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        for _ in 0..3 {
            app.update();
        }
        assert!(words(&mut app).iter().any(|text| text == "git"));
    }
    #[test]
    fn a_received_edits_change_is_shown_without_its_chat_transcript()
    {
        let mut app = panel_app();
        app.add_plugins((crate::chat::plugin, plugin));
        let mut state = hamlet_testing::realm::state();
        let mut wait = hamlet_testing::realm::wait("edit");
        if let v0::Wait::Approval {
            danger,
            change,
            about,
            ..
        } = &mut wait
        {
            *danger = v0::WaitDanger::Edit;
            *about = "orchard.md".into();
            *change = Some(v0::Change {
                path: "orchard.md".into(),
                created: true,
                lines: vec![],
                total: 0,
                added: 0,
                removed: 0,
                count: "A received change count".into(),
            });
        }
        state.waits.push(wait);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
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
