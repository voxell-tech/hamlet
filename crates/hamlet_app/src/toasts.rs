//! Toasts: what newly waits for the chief, and what happened that
//! needs no answer, stacked over the realm view.

use std::collections::HashMap;

use bevy::app::{App, PreUpdate};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::picking::hover::Hovered;
use bevy::time::Time;
use bevy::ui::{
    AlignItems, JustifyContent, PositionType, UiRect, Val, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, Visual, each, resource, style};
use hamlet_client::messages::{Answer, ServerFault};
use hamlet_client::realm::{
    Departure, Faulted, Kind, Left, Scholar, Seen, Settings,
};
use hamlet_client::waits::{RaisedBy, Waiting};
use hamlet_protocol::client::{
    AgentKind, Arrival, LeftHow, Newcomers, ScholarId, WaitDetail,
};
use hamlet_ui::styles::{action, buttons, caption, close_button};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx, View};

use crate::dock::{PETITIONS, show_tab};
use crate::screens::petition::{Need, button};
use crate::{PanelSystems, chat};

/// The most toasts on show at once.
const MOST: usize = 3;

/// Seconds a toast stays before it begins to fade.
const STAYS: f32 = 8.0;

/// Seconds a toast takes to fade out.
const FADES: f32 = 1.2;

/// The width of the stack, in logical pixels.
const WIDTH: f32 = 340.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Toasts>()
        .add_observer(answered)
        .add_observer(failed)
        .add_systems(
            PreUpdate,
            (raise, judged, keep).chain().in_set(PanelSystems::Bind),
        );
}

/// The toasts on show, oldest first.
#[derive(Resource, Default, PartialEq)]
pub struct Toasts {
    shown: Vec<Toast>,
    made: u64,
}

#[derive(Clone, PartialEq, Debug)]
struct Toast {
    id: u64,
    what: What,
    /// The time, in seconds of the clock, it begins to fade.
    until: f32,
    /// How seen it is, 1 to 0, while it fades.
    opacity: f32,
}

#[derive(Clone, PartialEq, Debug)]
enum What {
    /// A new wait, with the answers it takes.
    Petition(Entity),
    /// Something that happened, which nothing answers.
    Notice {
        tone: Tone,
        head: String,
        line: String,
    },
    /// The Hand let a scholar in or turned it away, which a chief
    /// can undo.
    Verdict { scholar: String, housed: bool },
}

/// The node of a toast, whose hover keeps it.
#[derive(Component)]
struct Body(u64);

impl Toasts {
    fn push(&mut self, what: What, now: f32) {
        self.made += 1;
        self.shown.push(Toast {
            id: self.made,
            what,
            until: now + STAYS,
            opacity: 1.0,
        });
        let over = self.shown.len().saturating_sub(MOST);
        self.shown.drain(..over);
    }
}

/// Raises a toast for each new wait, each worker that answered and
/// each agent that failed. What was there when the first state came
/// in is not new.
fn raise(
    time: Res<Time>,
    seen: Option<Res<Seen>>,
    waits: Query<Entity, Added<Waiting>>,
    answered: Query<(&Name, &Kind, &Departure), Added<Left>>,
    failing: Query<(Entity, &Name), Added<Faulted>>,
    waiting: Query<(&Waiting, Option<&RaisedBy>)>,
    mut toasts: ResMut<Toasts>,
    mut primed: Local<bool>,
) {
    if seen.is_none() {
        return;
    }
    if !*primed {
        *primed = true;
        return;
    }
    let now = time.elapsed_secs();
    for entity in &waits {
        toasts.push(What::Petition(entity), now);
    }
    for (name, kind, departure) in &answered {
        if kind.0 == AgentKind::Worker
            && departure.0.how == LeftHow::Answered
        {
            toasts.push(
                What::Notice {
                    tone: Tone::Accent,
                    head: "A WORKER ANSWERED".into(),
                    line: format!("{name} is done."),
                },
                now,
            );
        }
    }
    for (agent, name) in &failing {
        // A fault the chief is asked about is a petition already.
        let asked = waiting.iter().any(|(wait, by)| {
            by.is_some_and(|by| by.0 == agent)
                && matches!(wait.detail, WaitDetail::Fault {})
        });
        if !asked {
            toasts.push(
                What::Notice {
                    tone: Tone::Critical,
                    head: "FAULT".into(),
                    line: format!("{name} failed."),
                },
                now,
            );
        }
    }
}

/// Raises a toast when the Hand lets a scholar in or turns it away.
/// A scholar that went from waiting to housed or turned while the
/// Hand holds the door was decided by the Hand. What was there at
/// the first state is not new.
fn judged(
    time: Res<Time>,
    seen: Option<Res<Seen>>,
    settings: Res<Settings>,
    scholars: Query<&Scholar>,
    mut toasts: ResMut<Toasts>,
    mut before: Local<HashMap<ScholarId, Arrival>>,
    mut primed: Local<bool>,
) {
    if seen.is_none() {
        return;
    }
    let now = time.elapsed_secs();
    let by_hand = settings.0.newcomers == Newcomers::Hand;
    for scholar in &scholars {
        let scholar = &scholar.0;
        let was = before.insert(scholar.id, scholar.arrival);
        if !*primed || !by_hand || was != Some(Arrival::Waiting) {
            continue;
        }
        let housed = match scholar.arrival {
            Arrival::Housed => true,
            Arrival::Turned => false,
            Arrival::Waiting => continue,
        };
        toasts.push(
            What::Verdict {
                scholar: scholar.name.clone(),
                housed,
            },
            now,
        );
    }
    *primed = true;
}

/// The command that takes back the Hand's verdict on `scholar`.
fn undo_of(scholar: &str, housed: bool) -> String {
    match housed {
        true => format!("/turn-away {scholar}"),
        false => format!("/welcome {scholar}"),
    }
}

/// A failure the server could not address to a request.
fn failed(
    fault: On<ServerFault>,
    time: Res<Time>,
    mut toasts: ResMut<Toasts>,
) {
    toasts.push(
        What::Notice {
            tone: Tone::Critical,
            head: "FAULT".into(),
            line: fault.text.clone(),
        },
        time.elapsed_secs(),
    );
}

/// An answer from a toast or anywhere else takes its toast away.
fn answered(answer: On<Answer>, mut toasts: ResMut<Toasts>) {
    let wait = answer.wait;
    toasts
        .shown
        .retain(|toast| toast.what != What::Petition(wait));
}

/// Keeps a toast on show while the pointer is on it, fades it, and
/// takes away the ones that are over or whose wait is closed.
fn keep(
    time: Res<Time>,
    over: Query<(&Hovered, &Body)>,
    waiting: Query<(), With<Waiting>>,
    mut toasts: ResMut<Toasts>,
) {
    if toasts.shown.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    let mut next = toasts.shown.clone();
    for toast in &mut next {
        if over.iter().any(|(hovered, body)| {
            hovered.get() && body.0 == toast.id
        }) {
            toast.until = now + STAYS;
        }
        toast.opacity =
            ((toast.until + FADES - now) / FADES).clamp(0.0, 1.0);
    }
    next.retain(|toast| {
        toast.opacity > 0.0
            && match &toast.what {
                What::Petition(wait) => waiting.contains(*wait),
                What::Notice { .. } | What::Verdict { .. } => true,
            }
    });
    if next != toasts.shown {
        toasts.shown = next;
    }
}

/// The stack of toasts, at the top right of the view it is in.
pub fn stack() -> impl View {
    each(
        resource::<Toasts, _>(|toasts| {
            toasts.shown.iter().rev().cloned().collect()
        }),
        |toast: &Toast| toast.id,
        card,
    )
    .within(
        column(())
            .gap(8.0)
            .width(px(WIDTH))
            .position(PositionType::Absolute)
            .inset(UiRect {
                left: Val::Auto,
                top: px(12.0),
                right: px(12.0),
                bottom: Val::Auto,
            }),
    )
}

/// The words of a toast's line without the name its wait starts
/// with.
fn said(need: &Need) -> &str {
    need.line
        .strip_prefix(need.by.as_str())
        .unwrap_or(&need.line)
        .trim_start()
}

fn card(toast: &Toast) -> AnyView {
    let (id, what) = (toast.id, toast.what.clone());
    AnyView::new(move |cx: &mut Cx| {
        let (head, tone, line, answers) = match &what {
            What::Petition(wait) => {
                let Some(waiting) = cx.world.get::<Waiting>(*wait)
                else {
                    return cx.build(column(()));
                };
                let need = Need::of(*wait, waiting);
                let tone = match need.detail {
                    WaitDetail::Fault {} => Tone::Critical,
                    _ => Tone::Accent,
                };
                let answers = buttons(vec![
                    button(&need, 0),
                    button(&need, 1),
                    button(&need, 2),
                ])
                .boxed();
                (
                    need.by.to_uppercase(),
                    tone,
                    said(&need).to_string(),
                    answers,
                )
            }
            What::Notice { tone, head, line } => (
                head.clone(),
                *tone,
                line.clone(),
                column(()).boxed(),
            ),
            What::Verdict { scholar, housed } => {
                let undo = undo_of(scholar, *housed);
                let (head, line) = if *housed {
                    (
                        "THE HAND LET A SCHOLAR IN",
                        format!("{scholar} has a house in the city."),
                    )
                } else {
                    (
                        "THE HAND TURNED A SCHOLAR AWAY",
                        format!("{scholar} waits at the harbor."),
                    )
                };
                (
                    head.into(),
                    Tone::Accent,
                    line,
                    buttons(vec![action(
                        Glyph::Restore,
                        "UNDO",
                        move |world| {
                            chat::command(world, undo.clone());
                        },
                    )])
                    .boxed(),
                )
            }
        };
        let opens = matches!(what, What::Petition(_));
        let own = what.clone();
        let words = column((
            caption(head, tone),
            label(line).size(cx.theme().text.small),
        ))
        .gap(2.0)
        .grow(1.0)
        .min_width(px(0.0));
        let body = bevy_fynix::views::button(row((words,)))
            .grow(1.0)
            .min_width(px(0.0))
            .rules(opener)
            .on_activate(move |world| {
                if opens {
                    show_tab(world, PETITIONS);
                }
                dismiss(world, &own);
            });
        let dismissed = what.clone();
        let space = &cx.theme().space;
        let palette = &cx.theme().palette;
        // The answers are in the card, under its words.
        let card = column((
            row((
                body,
                close_button(move |world| dismiss(world, &dismissed)),
            ))
            .gap(6.0)
            .align(AlignItems::FlexStart),
            answers,
        ))
        .gap(space.gap)
        .fill(palette.card)
        .border(space.border)
        .border_color(palette.gold_6)
        .radius(space.round)
        .padding(UiRect::axes(
            px(space.card_padding_x),
            px(space.card_padding_y),
        ))
        .rules(fading(id));
        let node = cx.build(card);
        cx.world
            .entity_mut(node)
            .insert((Hovered::default(), Body(id)));
        node
    })
}

/// A toast's words, pressed to open what it is about: no fill of its
/// own, the card around it is the toast.
fn opener(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, _| {
            f.fill(Color::NONE)
                .padding(UiRect::all(px(0.0)))
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::FlexStart)
        })
        .hovered(|s| s.fill(|_| Color::NONE))
        .pressed(|s| s.fill(|_| Color::NONE))
        .apply(cx);
}

/// The rules that fade every part of toast `id` with its time.
fn fading(id: u64) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    move |cx| {
        cx.set::<Visual>(move |visual, _| {
            visual.opacity(resource::<Toasts, _>(move |toasts| {
                toasts
                    .shown
                    .iter()
                    .find(|toast| toast.id == id)
                    .map_or(0.0, |toast| toast.opacity)
            }))
        });
    }
}

/// Takes `what`'s toast away. A petition stays among the waits.
fn dismiss(world: &mut World, what: &What) {
    world
        .resource_mut::<Toasts>()
        .shown
        .retain(|toast| &toast.what != what);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::{TimeUpdateStrategy, Virtual};
    use hamlet_client::outbox::Outbox;
    use hamlet_client::waits::Choice;
    use hamlet_protocol::client as protocol;

    use super::*;
    use crate::testing::{panel_app, press, words};

    fn app_with_a_petition() -> App {
        let mut app = panel_app();
        app.add_plugins(plugin);
        // What waits at the first state is not new.
        let mut state = hamlet_testing::realm::state();
        state.waits.push(hamlet_testing::realm::wait("old"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.update();
        state.revision += 1;
        state.waits.push(hamlet_testing::realm::wait("new"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), stack());
        app.update();
        app
    }

    #[test]
    fn a_new_petition_toasts_and_is_answered_in_place() {
        let mut app = app_with_a_petition();
        assert_eq!(app.world().resource::<Toasts>().shown.len(), 1);
        let shown = words(&mut app);
        assert!(
            shown.iter().any(|text| text == "ADA@HOME"),
            "{shown:?}"
        );
        assert!(
            shown.iter().any(|text| text == "wants to run `pwd`")
        );
        press(&mut app, "ALLOW ONCE");
        let allowed =
            app.world().resource::<Outbox>().iter().any(|message| {
                matches!(message, protocol::ClientMessage::Answer(answer)
                    if answer.wait == "new" && answer.choice == Choice::Allow)
            });
        assert!(allowed);
        assert!(app.world().resource::<Toasts>().shown.is_empty());
    }

    #[test]
    fn the_hands_verdict_toasts_with_a_way_to_undo_it() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state.settings.newcomers = Newcomers::Hand;
        let mut moss = hamlet_testing::realm::scholar(3, "moss", &[]);
        moss.arrival = Arrival::Waiting;
        moss.house = None;
        state.scholars.push(moss);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        app.update();
        state.revision += 1;
        let moss = state.scholars.last_mut().unwrap();
        moss.arrival = Arrival::Housed;
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), stack());
        app.update();
        let shown = words(&mut app);
        assert!(
            shown
                .iter()
                .any(|text| text == "THE HAND LET A SCHOLAR IN"),
            "{shown:?}"
        );
        press(&mut app, "UNDO");
        let undone =
            app.world().resource::<Outbox>().iter().any(|message| {
                matches!(message, protocol::ClientMessage::Say(say)
                    if say.text == "/turn-away moss")
            });
        assert!(undone);
    }

    #[test]
    fn a_toast_stays_under_the_pointer_and_fades_otherwise() {
        let mut app = app_with_a_petition();
        let step = Duration::from_secs_f32(STAYS + 2.0);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(step));
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(step);
        let world = app.world_mut();
        let body = world
            .query::<(Entity, &Body)>()
            .iter(world)
            .next()
            .unwrap()
            .0;
        world.entity_mut(body).insert(Hovered(true));
        app.update();
        assert_eq!(app.world().resource::<Toasts>().shown.len(), 1);
        app.world_mut().entity_mut(body).insert(Hovered(false));
        app.update();
        assert!(app.world().resource::<Toasts>().shown.is_empty());
    }
}
