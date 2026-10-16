//! The Needs You screen: everything that waits for the chief, oldest
//! first and in full, with the answers each one takes.

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
use hamlet_core::oversight::danger::Danger;
use hamlet_core::oversight::{Answer, Kind, Need, Veto, Waiting};
use hamlet_core::session::{Entry, Transcript};
use hamlet_core::tools::Change;
use hamlet_rules::chief::rulings::Choice;
use hamlet_world::needs::{TagAnswered, TagOpened};

use crate::dock::{PanelAppExt, lit_panel, show_tab};
use crate::feel::{Lit, Mood};
use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, bad_action, brass, grim, lamp, marked, note, well,
};
use crate::me::Me;
use crate::platform::{DisplayText, MonoText, Wraps};
use crate::{AnyView, Cx, View, chat};

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
    rows.sort_by_key(|(_, w)| w.order);
    let needs = rows
        .into_iter()
        .map(|(entity, waiting)| Need::of(entity, waiting))
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
    let chief = world.resource::<Me>().user.clone();
    world.trigger(Answer {
        entity,
        choice,
        text,
        chief,
    });
    world.flush();
    world.resource_mut::<Reason>().0.clear();
}

/// A check or a cross on a tag in the world answers like a button
/// here.
pub(crate) fn tag_answered(
    answered: On<TagAnswered>,
    waiting: Query<&Waiting>,
    me: Res<Me>,
    mut commands: Commands,
) {
    let TagAnswered { item, agree } = *answered.event();
    let choice = waiting
        .get(item)
        .ok()
        .and_then(|need| Choice::quick(&need.answers, agree));
    if let Some(choice) = choice {
        commands.trigger(Answer {
            entity: item,
            choice,
            text: String::new(),
            chief: me.user.clone(),
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
    let Some(choice) = need.answers.get(at).copied() else {
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
            Choice::GoAhead => (Check, "GO AHEAD", lamp, Mood::Good),
            Choice::Withdraw => {
                (Restore, "WITHDRAW", grim, Mood::Bad)
            }
            Choice::Reply => (Talk, "REPLY", lamp, Mood::Good),
            Choice::Dismiss => (Cross, "DISMISS", brass, Mood::Plain),
        };
    marked(which, text)
        .rules(rules)
        .tagged(mood)
        .on_activate(move |world| answer(world, entity, choice))
        .boxed()
}

/// The edit that waits as `need`: its entry in the conversation
/// whose turn it holds, and its change.
fn held(world: &World, need: &Need) -> Option<(Entity, u64, Change)> {
    if need.kind != Kind::Approval(Danger::Edit) {
        return None;
    }
    let session = world.get::<Waiting>(need.entity)?.session?;
    let open = world
        .get::<Transcript>(session)?
        .items()
        .iter()
        .rev()
        .find(|item| {
            matches!(item.entry, Entry::Tool { exit: None, .. })
        })?;
    let Entry::Tool {
        change: Some(change),
        ..
    } = &open.entry
    else {
        return None;
    };
    let path = need.about.lines().next()?;
    path.starts_with(&change.path)
        .then(|| (session, open.id, change.clone()))
}

fn strip(need: &Need) -> AnyView {
    let need = need.clone();
    AnyView::new(move |cx: &mut Cx| {
        let about = match held(cx.world, &need) {
            Some((session, id, change)) => {
                chat::change_view(session, id, change)
            }
            // The exact text, which a tag in the world may cut.
            None => note(need.about.clone()).tagged(MonoText).boxed(),
        };
        let what = match need.kind {
            Kind::Approval(_) => "APPROVAL",
            Kind::Objection(_) => "OBJECTION",
            Kind::Question => "QUESTION",
            Kind::Fault => "FAULT",
            Kind::Lapsed => "UNANSWERED",
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
        cx.build(
            column((
                label(head)
                    .size(11.0)
                    .bold(true)
                    .tone(tone)
                    .tagged(DisplayText),
                label(need.line(80)).size(13.0),
                about,
                row((
                    button(&need, 0),
                    button(&need, 1),
                    button(&need, 2),
                ))
                .gap(8.0)
                .tagged(Wraps),
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
                let chief = world.resource::<Me>().user.clone();
                world.trigger(Veto { agent: None, chief });
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
            |n: &Need| {
                format!(
                    "{:?}|{}|{}|{}",
                    n.entity,
                    n.asked,
                    n.waited,
                    n.about.len()
                )
            },
            strip,
        )
        .within(column(()).gap(8.0)),))
        .fills_height(),
    ))
    .fills_height()
    .gap(12.0)
}
