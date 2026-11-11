//! Toasts: what newly waits for the chief, and what happened that
//! needs no answer, stacked over the realm view.

use std::collections::{HashMap, HashSet};

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
    AlignItems, ComputedNode, Display, JustifyContent, Overflow,
    PositionType, UiRect, Val, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, row,
};
use bevy_fynix::{
    ReducedMotion, ScopedExt, Signal, ViewExt, Visual, each,
    resource, style,
};
use hamlet_client::messages::{Answer, Response, ServerFault};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::{
    Departure, Faulted, Kind, Left, LivesIn, Scholar, Seen, Shown,
    name_of,
};
use hamlet_client::waits::{RaisedBy, Waiting};
use hamlet_protocol::client::{
    AgentKind, Arrival, ClientMessage, LeftHow, ReplyKind, ScholarId,
    ServerMessage, WaitDetail,
};
use hamlet_ui::styles::{
    ICON, action, buttons, card_name, close_button, note, quoted,
    status_mark,
};
use hamlet_ui::theme::{HamletTheme, StatusKind};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx, View, kinds, rules, with_cx};

use crate::convo::Convo;
use crate::hub::Showing;
use crate::screens::petition::{Need, answers};
use crate::standing::Standing;
use crate::updates::{self, Offer, Ready};
use crate::{PanelSystems, chat};

/// The most toasts kept at once.
const MOST: usize = 5;

/// The older toasts whose edges show behind the newest while the
/// stack is closed.
const FOLDED: usize = 2;

/// The strip of a folded toast that shows above the one in front of
/// it, and its narrowing on each side.
const EDGE: f32 = 6.0;

/// Seconds a toast stays before it begins to fade.
const STAYS: f32 = 8.0;

/// Seconds a toast takes to fade out.
const FADES: f32 = 1.2;

/// The width of the stack, in logical pixels.
const WIDTH: f32 = 430.0;

/// How the server names the Hand as the one who decided an arrival.
const HAND: &str = "hand";

pub fn plugin(app: &mut App) {
    app.init_resource::<Toasts>()
        .init_resource::<Decreed>()
        .add_observer(answered)
        .add_observer(failed)
        .add_observer(refused)
        .add_systems(
            PreUpdate,
            (raise, judged, keep, settle)
                .chain()
                .in_set(PanelSystems::Bind),
        );
}

/// The toasts on show, oldest first.
#[derive(Resource, Default, PartialEq)]
pub struct Toasts {
    shown: Vec<Toast>,
    made: u64,
    /// The flag for the pointer being on the stack, which lays every
    /// toast out in full.
    open: bool,
}

/// A toast's place in the stack.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Place {
    /// In full: the newest, or any while the stack is open.
    Whole,
    /// Folded behind the newest, this many toasts back, only its
    /// edge showing.
    Folded(usize),
    /// Too far back to show while the stack is closed.
    Hidden,
}

/// A toast's progress towards its [`Place`], which it eases along.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Pose {
    /// 0 in full, 1 folded to its edge.
    fold: f32,
    /// The number of toasts in front of it, each narrowing it.
    back: f32,
    /// 1 on show, 0 gone: a new toast comes in from 0, and one too
    /// far back goes to it.
    shown: f32,
}

impl Pose {
    /// The pose a toast at `place` comes to rest in.
    fn of(place: Place) -> Self {
        match place {
            Place::Whole => Self {
                fold: 0.0,
                back: 0.0,
                shown: 1.0,
            },
            Place::Folded(back) => Self {
                fold: 1.0,
                back: back as f32,
                shown: 1.0,
            },
            Place::Hidden => Self {
                fold: 1.0,
                back: FOLDED as f32,
                shown: 0.0,
            },
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
struct Toast {
    id: u64,
    what: What,
    /// The time, in seconds of the clock, it begins to fade.
    until: f32,
    /// How seen it is, 1 to 0, while it fades.
    opacity: f32,
    pose: Pose,
    /// The height of what it says, in logical pixels, once laid out.
    tall: Option<f32>,
}

#[derive(Clone, PartialEq, Debug)]
enum What {
    /// A new wait, with the answers it takes.
    Petition(Entity),
    /// Something that happened, which nothing answers.
    Notice {
        kind: StatusKind,
        head: String,
        line: String,
    },
    /// The Hand let a scholar in or turned it away, which a chief
    /// can undo.
    Verdict {
        scholar: ScholarId,
        name: String,
        housed: bool,
    },
    /// A newer Hamlet, which stays until it is taken or closed.
    Update(Offer),
}

/// The stack's node, whose hover opens it and keeps its toasts.
#[derive(Component)]
struct Pile;

/// The node of what toast `id` says, measured for its height.
#[derive(Component)]
struct Inside(u64);

impl Toasts {
    fn push(&mut self, what: What, now: f32) {
        self.made += 1;
        self.shown.push(Toast {
            id: self.made,
            what,
            until: now + STAYS,
            opacity: 1.0,
            pose: Pose {
                shown: 0.0,
                ..Pose::of(Place::Whole)
            },
            tall: None,
        });
        // An update stays until it is taken or closed, so the oldest
        // of the others makes way first.
        while self.shown.len() > MOST {
            let oldest = self
                .shown
                .iter()
                .position(|toast| {
                    !matches!(toast.what, What::Update(_))
                })
                .unwrap_or(0);
            self.shown.remove(oldest);
        }
    }

    /// Something that happened, which nothing answers.
    pub(crate) fn note(
        &mut self,
        kind: StatusKind,
        head: String,
        line: String,
        now: f32,
    ) {
        self.push(What::Notice { kind, head, line }, now);
    }

    /// Offers `offer` in a toast that does not fade, unless one is on
    /// show already.
    pub(crate) fn offer(&mut self, offer: Offer, now: f32) {
        let what = What::Update(offer);
        if self.shown.iter().any(|toast| toast.what == what) {
            return;
        }
        self.push(what, now);
        if let Some(toast) = self.shown.last_mut() {
            toast.until = f32::INFINITY;
        }
    }

    /// The [`Place`] of toast `id` in the stack.
    fn place(&self, id: u64) -> Place {
        let Some(at) =
            self.shown.iter().position(|toast| toast.id == id)
        else {
            return Place::Hidden;
        };
        match self.shown.len() - 1 - at {
            0 => Place::Whole,
            _ if self.open => Place::Whole,
            back if back <= FOLDED => Place::Folded(back),
            _ => Place::Hidden,
        }
    }

    fn get(&self, id: u64) -> Option<&Toast> {
        self.shown.iter().find(|toast| toast.id == id)
    }
}

/// Raises a toast for each new wait, each worker that answered and
/// each agent that failed. What was there when the first state came
/// in is not new.
fn raise(
    time: Res<Time>,
    seen: Option<Res<Seen>>,
    waits: Query<Entity, Added<Waiting>>,
    answered: Query<
        (&Name, Option<&Shown>, &Kind, &Departure),
        Added<Left>,
    >,
    failing: Query<(Entity, &Name, Option<&Shown>), Added<Faulted>>,
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
    for (name, shown, kind, departure) in &answered {
        let name = shown.map_or(name.as_str(), |shown| &shown.0);
        if kind.0 == AgentKind::Worker
            && departure.0.how == LeftHow::Answered
        {
            toasts.push(
                What::Notice {
                    kind: StatusKind::Success,
                    head: "A worker answered".into(),
                    line: format!("{name} is done."),
                },
                now,
            );
        }
    }
    for (agent, name, shown) in &failing {
        let name = shown.map_or(name.as_str(), |shown| &shown.0);
        // A fault the chief is asked about is a petition already.
        let asked = waiting.iter().any(|(wait, by)| {
            by.is_some_and(|by| by.0 == agent)
                && matches!(wait.detail, WaitDetail::Fault {})
        });
        if !asked {
            toasts.push(
                What::Notice {
                    kind: StatusKind::Danger,
                    head: "Fault".into(),
                    line: format!("{name} failed."),
                },
                now,
            );
        }
    }
}

/// Raises a toast when the Hand lets a scholar in or turns it away:
/// a scholar that went from waiting to housed or turned, by the Hand.
/// What was there at the first state is not new.
fn judged(
    time: Res<Time>,
    seen: Option<Res<Seen>>,
    scholars: Query<&Scholar>,
    mut toasts: ResMut<Toasts>,
    mut before: Local<HashMap<ScholarId, Arrival>>,
    mut primed: Local<bool>,
) {
    if seen.is_none() {
        return;
    }
    let now = time.elapsed_secs();
    for scholar in &scholars {
        let scholar = &scholar.0;
        let was = before.insert(scholar.id, scholar.arrival);
        let by_hand = scholar.arrival_by.as_deref() == Some(HAND);
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
                scholar: scholar.id,
                name: scholar.name.clone(),
                housed,
            },
            now,
        );
    }
    *primed = true;
}

/// The command that takes back the Hand's verdict on `scholar`.
fn undo_of(scholar: ScholarId, housed: bool) -> String {
    let address = hamlet_client::realm::scholar_address(scholar);
    match housed {
        true => format!("/turn-away {address}"),
        false => format!("/welcome {address}"),
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
            kind: StatusKind::Danger,
            head: "Fault".into(),
            line: fault.text.clone(),
        },
        time.elapsed_secs(),
    );
}

/// The commands the window made, from the world, a form or a button
/// rather than typed in a chat, whose refusal would otherwise be said
/// only in the chat they went to.
#[derive(Resource, Default)]
pub(crate) struct Decreed(HashSet<RequestId>);

/// Watches the command just queued, the last thing sent, so that its
/// refusal is toasted.
pub(crate) fn watch_last(world: &mut World) {
    let last = world.resource::<Outbox>().iter().last().and_then(
        |message| match message {
            ClientMessage::Say(say) => Some(say.id.clone()),
            _ => None,
        },
    );
    // A window drawn without toasts watches nothing.
    if let Some(id) = last
        && let Some(mut decreed) = world.get_resource_mut::<Decreed>()
    {
        decreed.0.insert(id);
    }
}

/// A command given from the world that the server refused.
fn refused(
    response: On<Response>,
    time: Res<Time>,
    mut decreed: ResMut<Decreed>,
    mut toasts: ResMut<Toasts>,
) {
    if !decreed.0.remove(&response.to) {
        return;
    }
    let text = match &response.message {
        ServerMessage::Reply(reply)
            if reply.kind == ReplyKind::Error =>
        {
            reply.text.clone()
        }
        ServerMessage::Fault(fault) => fault.text.clone(),
        _ => return,
    };
    toasts.push(
        What::Notice {
            kind: StatusKind::Danger,
            head: "Not done".into(),
            line: text,
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

/// Opens the stack and keeps its toasts while the pointer is on it,
/// fades them otherwise, and takes away the ones that are over or
/// whose wait is closed.
fn keep(
    time: Res<Time>,
    pile: Query<&Hovered, With<Pile>>,
    waiting: Query<Option<&RaisedBy>, With<Waiting>>,
    showing: Option<Res<Showing>>,
    convo: Option<Res<Convo>>,
    mut toasts: ResMut<Toasts>,
) {
    let open = pile.iter().any(Hovered::get);
    if toasts.open != open {
        toasts.open = open;
    }
    if toasts.shown.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    let chatting = convo
        .filter(|_| showing.is_some_and(|s| *s == Showing::Thread))
        .map(|convo| convo.agent);
    let mut next = toasts.shown.clone();
    for toast in &mut next {
        if open {
            toast.until = toast.until.max(now + STAYS);
        }
        toast.opacity =
            ((toast.until + FADES - now) / FADES).clamp(0.0, 1.0);
    }
    next.retain(|toast| {
        toast.opacity > 0.0
            && match &toast.what {
                // Its chat, open, shows it in full.
                What::Petition(wait) => {
                    waiting.get(*wait).is_ok_and(|by| {
                        by.is_none_or(|by| Some(by.0) != chatting)
                    })
                }
                What::Notice { .. }
                | What::Verdict { .. }
                | What::Update(_) => true,
            }
    });
    if next != toasts.shown {
        toasts.shown = next;
    }
}

/// Eases each toast towards its place in the stack, and notes how
/// tall what it says was laid out.
fn settle(
    time: Res<Time>,
    theme: Option<Res<bevy_fynix::Theme<HamletTheme>>>,
    reduced: Option<Res<ReducedMotion>>,
    insides: Query<(&Inside, &ComputedNode)>,
    mut toasts: ResMut<Toasts>,
) {
    if toasts.shown.is_empty() {
        return;
    }
    let expand = theme.map_or(0.22, |theme| theme.0.motion.expand);
    // Most of the way there in the time the theme gives what opens
    // or folds away. A theme with no such time moves them at once.
    let rate = 3.0 / expand;
    let at_once =
        reduced.is_some_and(|reduced| reduced.0) || expand <= 0.0;
    let dt = time.delta_secs();
    let mut next = toasts.shown.clone();
    for toast in &mut next {
        let want = Pose::of(toasts.place(toast.id));
        let pose = &mut toast.pose;
        if at_once {
            *pose = want;
        } else {
            pose.fold = ease(pose.fold, want.fold, rate, dt);
            pose.back = ease(pose.back, want.back, rate, dt);
            pose.shown = ease(pose.shown, want.shown, rate, dt);
        }
        if let Some((_, node)) =
            insides.iter().find(|(inside, _)| inside.0 == toast.id)
        {
            let tall = node.size().y * node.inverse_scale_factor();
            if tall > 0.0
                && toast
                    .tall
                    .is_none_or(|was| (was - tall).abs() > 0.5)
            {
                toast.tall = Some(tall);
            }
        }
    }
    if next != toasts.shown {
        toasts.shown = next;
    }
}

/// `now` a step of `dt` seconds closer to `want`, at `rate`, and at
/// `want` once it is too close to tell.
fn ease(now: f32, want: f32, rate: f32, dt: f32) -> f32 {
    let next = now + (want - now) * (1.0 - (-rate * dt).exp());
    if (want - next).abs() < 0.005 {
        want
    } else {
        next
    }
}

/// The stack of toasts, at the bottom left of the window over the
/// status bar, the newest nearest the corner and the older folded
/// behind it. The pointer on the stack opens it, every toast in full
/// and a way to close them all above them.
pub fn stack() -> impl View {
    with_cx(|cx| {
        let space = &cx.theme().space;
        column((
            closing_all(),
            each(
                resource::<Toasts, _>(|toasts| toasts.shown.clone()),
                |toast: &Toast| toast.id,
                card,
            )
            .within(column(()).gap(space.gap)),
        ))
        .gap(space.gap)
        .width(px(WIDTH))
        .position(PositionType::Absolute)
        .inset(UiRect {
            left: px(space.panel + space.gap),
            top: Val::Auto,
            right: Val::Auto,
            // The status bar is less high than the menu bar.
            bottom: px(space.bar + space.gap),
        })
        .with(Hovered::default())
        .with(Pile)
    })
}

/// "Close all", at the end of the line above an open stack of more
/// than one toast, filled so that it reads over the world.
fn closing_all() -> impl View {
    row((action(Glyph::Cross, "Close all", dismiss_all),))
        .justify(JustifyContent::FlexEnd)
        .display(resource::<Toasts, _>(|toasts| {
            if toasts.open && toasts.shown.len() > 1 {
                Display::Flex
            } else {
                Display::None
            }
        }))
}

/// A prop of toast `id` that follows where it is on its way to its
/// place, and how tall what it says is. A toast taken away is gone.
fn posed<T: Send + Sync + 'static>(
    id: u64,
    pick: impl Fn(Pose, Option<f32>) -> T + Send + Sync + 'static,
) -> Signal<T> {
    resource::<Toasts, _>(move |toasts| match toasts.get(id) {
        Some(toast) => pick(toast.pose, toast.tall),
        None => pick(Pose::of(Place::Hidden), None),
    })
}

/// The words of a toast's line without the name its wait starts
/// with.
fn said(need: &Need) -> &str {
    need.line
        .strip_prefix(need.shown.as_str())
        .unwrap_or(&need.line)
        .trim_start()
}

/// Where who raised `need` is and how long ago it asked:
/// `· mill · now`.
fn origin(world: &World, need: &Need, by: &str) -> String {
    let age = need.waited.clone();
    if let WaitDetail::Newcomer { .. } = need.detail {
        return format!("· harbor · {age}");
    }
    match hamlet_client::realm::named(world, by)
        .and_then(|agent| world.get::<LivesIn>(agent))
    {
        Some(lives) => {
            format!("· {} · {age}", name_of(world, lives.0))
        }
        None => format!("· {age}"),
    }
}

fn card(toast: &Toast) -> AnyView {
    let (id, what) = (toast.id, toast.what.clone());
    AnyView::new(move |cx: &mut Cx| {
        let (mark, head, from, kind, line, answers) = match &what {
            What::Petition(wait) => {
                let Some(waiting) = cx.world.get::<Waiting>(*wait)
                else {
                    return cx.build(column(()));
                };
                let need = Need::of(*wait, waiting);
                let kind = match need.detail {
                    WaitDetail::Fault {} => StatusKind::Danger,
                    _ => Standing::Waits.kind(),
                };
                let from = origin(cx.world, &need, &waiting.by);
                let answers = answers(&need);
                (
                    // The bell of NEEDS YOU.
                    Standing::Waits.glyph(),
                    need.shown.clone(),
                    from,
                    kind,
                    said(&need).to_string(),
                    answers,
                )
            }
            What::Notice { kind, head, line } => (
                match kind {
                    StatusKind::Success => Glyph::Check,
                    _ => Glyph::Stop,
                },
                head.clone(),
                String::new(),
                *kind,
                line.clone(),
                column(()).boxed(),
            ),
            What::Verdict {
                scholar,
                name,
                housed,
            } => {
                let undo = undo_of(*scholar, *housed);
                let (head, line, kind) = if *housed {
                    (
                        "The Hand let a scholar in",
                        format!("{name} has a house in the city."),
                        StatusKind::Success,
                    )
                } else {
                    (
                        "The Hand turned a scholar away",
                        format!("{name} waits at the harbor."),
                        StatusKind::Warning,
                    )
                };
                (
                    Glyph::Tower,
                    head.into(),
                    String::new(),
                    kind,
                    line,
                    buttons(vec![action(
                        Glyph::Restore,
                        "Undo",
                        move |world| {
                            chat::command(world, undo.clone());
                        },
                    )])
                    .boxed(),
                )
            }
            What::Update(offer) => {
                let version = offer.version;
                let (line, button) = match &offer.ready {
                    Ready::File(file) => {
                        let file = file.clone();
                        (
                            "Restart to update. Agents pause while \
                             Hamlet restarts, and carry on after."
                                .to_string(),
                            action(
                                Glyph::Restore,
                                "Restart",
                                move |world| {
                                    updates::restart(
                                        world,
                                        version,
                                        file.clone(),
                                    );
                                },
                            ),
                        )
                    }
                    Ready::Page(page) => {
                        let page = page.clone();
                        (
                            "This copy cannot update itself. \
                             Download the new one from its page."
                                .to_string(),
                            action(
                                Glyph::Globe,
                                "Download",
                                move |_| {
                                    hamlet_desktop::open::browse(
                                        &page,
                                    );
                                },
                            ),
                        )
                    }
                };
                (
                    Glyph::Hamlet,
                    format!("Hamlet {version} is out"),
                    String::new(),
                    StatusKind::Info,
                    line,
                    buttons(vec![button]).boxed(),
                )
            }
        };
        let head = row((
            status_mark(mark, kind, ICON + 6.0),
            card_name(head, Tone::Body),
            note(from),
        ))
        .gap(6.0)
        .align(AlignItems::Center);
        let dismissed = what.clone();
        // The close sits at the end of the head's line, centred on
        // it.
        let top = row((
            opening(what.clone(), head),
            close_button(move |world| dismiss(world, &dismissed)),
        ))
        .gap(6.0)
        .align(AlignItems::Center);
        let words =
            column((top, opening(what.clone(), quoted(&line))))
                .gap(2.0);
        let theme = cx.theme();
        let space = &theme.space;
        // Laid out in full even while the card is folded, so that its
        // height is known to unfold to; it fades as the card folds.
        let inside = column((words, answers))
            .gap(space.gap)
            .shrink(0.0)
            .with(Inside(id))
            .rules(fading(id));
        // Its shadow doubled, so it stands off the world and the
        // panels.
        let mut shadow = theme.shadow();
        shadow.0.extend(theme.shadow().0);
        // The corner, padding and edge `rules::floating` gives a
        // card.
        let (gap, round) = (space.gap, space.soft);
        let (border, padding_y) =
            (space.border, space.card_padding_y);
        // Folded, a card is a strip under the bottom of the one in
        // front of it, its round corners hidden there, narrower the
        // further back it is. A card is never shorter than its
        // padding and edge.
        let strip = (EDGE + round).max(2.0 * (padding_y + border));
        let card = kinds::card((inside,))
            .status(kind)
            .overflow(Overflow::clip())
            .opacity(resource::<Toasts, _>(move |toasts| {
                toasts.get(id).map_or(0.0, |toast| {
                    toast.opacity * toast.pose.shown
                })
            }))
            .display(posed(id, |pose, _| {
                if pose.shown > 0.0 {
                    Display::Flex
                } else {
                    Display::None
                }
            }))
            .height(posed(id, move |pose, tall| match tall {
                // In full it takes the height of what it says.
                _ if pose.fold == 0.0 => Val::Auto,
                Some(tall) => {
                    let whole = tall + 2.0 * (padding_y + border);
                    px(whole + (strip - whole) * pose.fold)
                }
                None => px(strip),
            }))
            .margin(posed(id, move |pose, _| {
                let aside = EDGE * pose.back * pose.fold;
                UiRect {
                    left: px(aside),
                    right: px(aside),
                    top: px(0.0),
                    // Only its edge takes room, and none once gone.
                    bottom: px(-(gap + strip - EDGE * pose.shown)
                        * pose.fold),
                }
            }))
            .tagged(shadow)
            .rules(rules::floating);
        cx.build(card)
    })
}

/// `content`, a part of the words of the toast of `what`, pressed to
/// open what it is about.
fn opening(what: What, content: impl View) -> impl View {
    button(row((content,)))
        .grow(1.0)
        .min_width(px(0.0))
        .rules(opener)
        .on_activate(move |world| {
            if let What::Petition(wait) = &what {
                crate::petition::open(world, *wait);
            }
            dismiss(world, &what);
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

/// The rules that fade every part of what toast `id` says with its
/// time, and as it folds.
fn fading(id: u64) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    move |cx| {
        cx.set::<Visual>(move |visual, _| {
            visual.opacity(resource::<Toasts, _>(move |toasts| {
                toasts.get(id).map_or(0.0, |toast| {
                    toast.opacity
                        * toast.pose.shown
                        * (1.0 - toast.pose.fold)
                })
            }))
        });
    }
}

/// Takes every toast away, as [`dismiss`] does each.
fn dismiss_all(world: &mut World) {
    let shown = world
        .resource::<Toasts>()
        .shown
        .iter()
        .map(|toast| toast.what.clone())
        .collect::<Vec<_>>();
    for what in &shown {
        dismiss(world, what);
    }
}

/// Takes `what`'s toast away. A petition stays among the waits, and
/// a closed update is not offered again while the window is open.
fn dismiss(world: &mut World, what: &What) {
    if let What::Update(offer) = what {
        updates::decline(world, offer.version);
    }
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
            ServerMessage::State(state.clone()),
        );
        app.update();
        state.revision += 1;
        state.waits.push(hamlet_testing::realm::wait("new"));
        hamlet_client::receive(
            app.world_mut(),
            ServerMessage::State(state),
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
        assert!(shown.iter().any(|text| text == "ada"), "{shown:?}");
        // The command on a chip of its own, with no backticks.
        assert!(shown.iter().any(|text| text == "wants to run"));
        assert!(shown.iter().any(|text| text == "pwd"));
        press(&mut app, "Allow once");
        let allowed =
            app.world().resource::<Outbox>().iter().any(|message| {
                matches!(message, ClientMessage::Answer(answer)
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
        state.settings.newcomers = protocol::Newcomers::Hand;
        let mut moss = hamlet_testing::realm::scholar(3, "moss", &[]);
        moss.arrival = Arrival::Waiting;
        moss.house = None;
        state.scholars.push(moss);
        hamlet_client::receive(
            app.world_mut(),
            ServerMessage::State(state.clone()),
        );
        app.update();
        state.revision += 1;
        let moss = state.scholars.last_mut().unwrap();
        moss.arrival = Arrival::Housed;
        moss.arrival_by = Some(HAND.into());
        hamlet_client::receive(
            app.world_mut(),
            ServerMessage::State(state),
        );
        app.update();
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), stack());
        app.update();
        let shown = words(&mut app);
        assert!(
            shown
                .iter()
                .any(|text| text == "The Hand let a scholar in"),
            "{shown:?}"
        );
        press(&mut app, "Undo");
        let undone =
            app.world().resource::<Outbox>().iter().any(|message| {
                matches!(message, ClientMessage::Say(say)
                    if say.text == "/turn-away scholar#3")
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
        let pile = pile(&mut app);
        app.world_mut().entity_mut(pile).insert(Hovered(true));
        app.update();
        assert_eq!(app.world().resource::<Toasts>().shown.len(), 1);
        app.world_mut().entity_mut(pile).insert(Hovered(false));
        app.update();
        assert!(app.world().resource::<Toasts>().shown.is_empty());
    }

    /// The stack's node.
    fn pile(app: &mut App) -> Entity {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Pile>>()
            .single(world)
            .unwrap()
    }

    /// Toasts with `count` faults, the newest last.
    fn faults(count: usize) -> Toasts {
        let mut toasts = Toasts::default();
        for n in 0..count {
            toasts.push(
                What::Notice {
                    kind: StatusKind::Danger,
                    head: "Fault".into(),
                    line: n.to_string(),
                },
                0.0,
            );
        }
        toasts
    }

    /// The window with `toasts` stacked, a frame `step` long.
    fn stacked(toasts: Toasts, step: Duration) -> App {
        let mut app = panel_app();
        app.add_plugins(plugin);
        app.insert_resource(toasts);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(step));
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(step);
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), stack());
        app.update();
        app
    }

    /// Each toast's fold, oldest first.
    fn folds(app: &App) -> Vec<f32> {
        let toasts = app.world().resource::<Toasts>();
        toasts.shown.iter().map(|toast| toast.pose.fold).collect()
    }

    #[test]
    fn older_toasts_fold_behind_the_newest_until_the_stack_opens() {
        let mut toasts = faults(4);
        let places = |toasts: &Toasts| -> Vec<Place> {
            toasts
                .shown
                .iter()
                .map(|toast| toasts.place(toast.id))
                .collect()
        };
        assert_eq!(
            places(&toasts),
            [
                Place::Hidden,
                Place::Folded(2),
                Place::Folded(1),
                Place::Whole
            ]
        );
        toasts.open = true;
        assert_eq!(places(&toasts), [Place::Whole; 4]);
    }

    #[test]
    fn toasts_ease_into_their_folds_and_out_of_them() {
        let mut app = stacked(faults(2), Duration::from_millis(16));
        app.update();
        let [older, newest] = folds(&app)[..] else {
            panic!("two toasts");
        };
        // On its way, not there at once.
        assert!(0.0 < older && older < 1.0, "{older}");
        assert_eq!(newest, 0.0);
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(folds(&app), [1.0, 0.0]);
        let pile = pile(&mut app);
        app.world_mut().entity_mut(pile).insert(Hovered(true));
        app.update();
        app.update();
        assert!(folds(&app)[0] < 1.0);
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(folds(&app), [0.0, 0.0]);
    }

    #[test]
    fn with_less_motion_toasts_fold_at_once() {
        let mut app = stacked(faults(2), Duration::from_millis(16));
        app.insert_resource(ReducedMotion(true));
        app.update();
        assert_eq!(folds(&app), [1.0, 0.0]);
    }

    #[test]
    fn an_open_stack_closes_all_its_toasts_at_once() {
        let mut app = stacked(faults(2), Duration::from_millis(16));
        let pile = pile(&mut app);
        app.world_mut().entity_mut(pile).insert(Hovered(true));
        app.update();
        assert!(app.world().resource::<Toasts>().open);
        press(&mut app, "Close all");
        assert!(app.world().resource::<Toasts>().shown.is_empty());
    }

    #[test]
    fn an_update_stays_on_show_until_it_is_taken_or_closed() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        let step = Duration::from_secs_f32(STAYS + FADES + 1.0);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(step));
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(step);
        let offer = Offer {
            version: hamlet_protocol::Version::new(0, 2, 0),
            ready: Ready::Page("https://example.com".into()),
        };
        let mut toasts = app.world_mut().resource_mut::<Toasts>();
        toasts.offer(offer.clone(), 0.0);
        toasts.offer(offer.clone(), 0.0);
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Toasts>().shown.len(), 1);
        dismiss(app.world_mut(), &What::Update(offer));
        assert!(app.world().resource::<Toasts>().shown.is_empty());
    }

    #[test]
    fn newer_toasts_make_way_around_an_update() {
        let offer = Offer {
            version: hamlet_protocol::Version::new(0, 2, 0),
            ready: Ready::Page("https://example.com".into()),
        };
        let mut toasts = Toasts::default();
        toasts.offer(offer.clone(), 0.0);
        for n in 0..MOST {
            toasts.note(
                StatusKind::Danger,
                "Fault".into(),
                n.to_string(),
                0.0,
            );
        }
        assert_eq!(toasts.shown.len(), MOST);
        assert_eq!(toasts.shown[0].what, What::Update(offer));
        assert!(matches!(
            &toasts.shown[1].what,
            What::Notice { line, .. } if line == "1"
        ));
    }

    #[test]
    fn a_command_from_the_world_that_is_refused_toasts() {
        let mut app = panel_app();
        app.add_plugins(plugin).add_observer(chat::decreed);
        app.world_mut().trigger(hamlet_realm::orders::Decree {
            hamlet: None,
            text: "/move-house ada 1 1".into(),
        });
        app.world_mut().flush();
        let said = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                ClientMessage::Say(say) => Some(say),
                _ => None,
            })
            .collect::<Vec<_>>();
        let [say] = &said[..] else {
            panic!("one command: {said:?}");
        };
        let refusal = serde_json::from_value(serde_json::json!({
            "type": "reply",
            "to": say.id,
            "kind": "error",
            "text": "the house would stand on another",
        }))
        .unwrap();
        hamlet_client::receive(app.world_mut(), refusal);
        let shown = &app.world().resource::<Toasts>().shown;
        assert!(matches!(
            &shown[..],
            [Toast { what: What::Notice { line, .. }, .. }]
                if line == "the house would stand on another"
        ));
    }
}
