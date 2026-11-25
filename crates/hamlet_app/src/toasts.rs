//! Toasts stacked over the realm view.

use std::collections::HashMap;

use bevy::app::{App, PreUpdate};
use bevy::clipboard::Clipboard;
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
use bevy_fynix::dock::DockTree;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, row,
};
use bevy_fynix::{
    Prop, ReducedMotion, ScopedExt, Signal, ViewExt, Visual, derived,
    each, resource, style,
};
use hamlet_client::messages::{Answer, Response, ServerFault};
use hamlet_client::outbox::RequestId;
use hamlet_client::realm::{
    Departure, Faulted, Kind, Lead, Left, LivesIn, Profile, Seen,
    Shown, name_of,
};
use hamlet_client::waits::{Choice, RaisedBy, Waiting};
use hamlet_core::models::{Shelf, model_title};
use hamlet_core::panels::{PanelSystems, is_in_front};
use hamlet_core::standing::Standing;
use hamlet_core::toasts::WatchRequest;
use hamlet_core::welcome::OpenWelcome;
use hamlet_protocol::client::{
    AgentKind, Arrival, LeftHow, ProfileId, ReplyKind, ServerMessage,
    Setup, SetupCause, WaitDetail,
};
use hamlet_ui::styles::{
    ICON, action, buttons, card_name, close_button, note, quoted,
    status_mark,
};
use hamlet_ui::theme::{HamletTheme, StatusKind};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx, View, kinds, rules, with_cx};

use crate::chat;
use crate::hub::{Hub, Sheet, Showing};
use crate::screens::wait::{self, ChatWaits, Need, answers};
use crate::updates::{self, Offer, Ready};

/// The most toasts kept at once.
const MOST: usize = 5;

/// The toasts folded behind the newest while the stack is closed.
const FOLDED: usize = 2;

/// How much a folded toast narrows per toast in front.
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
        .add_observer(responded)
        .add_observer(watched)
        .add_observer(toasted)
        .add_systems(
            PreUpdate,
            (raise, judged, keep.after(wait::follow), settle)
                .chain()
                .in_set(PanelSystems::Bind),
        );
}

/// The toasts on show, oldest first.
#[derive(Resource, Default, PartialEq)]
pub struct Toasts {
    shown: Vec<Toast>,
    made: u64,
    /// Whether the pointer is on the stack.
    open: bool,
}

/// A toast's place in the stack.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Place {
    /// In full: the newest, or any while the stack is open.
    Whole,
    /// Folded behind the newest, this many toasts back.
    Folded(usize),
    /// Too far back to show while the stack is closed.
    Hidden,
}

/// A toast's progress towards its [`Place`], which it eases along.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Pose {
    /// 0 in full, 1 folded away.
    fold: f32,
    /// The number of toasts in front of it, each narrowing it.
    back: f32,
    /// 1 on show, 0 gone.
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
    Wait(Entity),
    /// The open faults of one setup cause, shown as one.
    Setup { setup: Setup, waits: Vec<Entity> },
    /// Something that happened, which nothing answers.
    Notice {
        kind: StatusKind,
        head: String,
        line: String,
    },
    /// The Hand let a profile in or turned it away.
    Verdict {
        profile: ProfileId,
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
        // An update stays, so the oldest other makes way.
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

    /// Shows the fault `wait` in the one toast of its setup cause.
    fn gather(&mut self, setup: Setup, wait: Entity, now: f32) {
        let told = self.shown.iter_mut().find(|toast| {
            matches!(&toast.what, What::Setup { setup: shown, .. }
                if *shown == setup)
        });
        match told {
            Some(toast) => {
                if let What::Setup { waits, .. } = &mut toast.what
                    && !waits.contains(&wait)
                {
                    waits.push(wait);
                }
                toast.until = toast.until.max(now + STAYS);
            }
            None => self.push(
                What::Setup {
                    setup,
                    waits: vec![wait],
                },
                now,
            ),
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

    /// Offers `offer` in a toast that does not fade.
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

    /// Takes away every update offered, as when another channel is
    /// followed.
    pub(crate) fn withdraw_offers(&mut self) {
        self.shown
            .retain(|toast| !matches!(toast.what, What::Update(_)));
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

/// Raises a toast for each new wait, answer and failure.
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
        match waiting
            .get(entity)
            .ok()
            .and_then(|(wait, _)| setup_of(wait))
        {
            Some(setup) => toasts.gather(setup.clone(), entity, now),
            None => toasts.push(What::Wait(entity), now),
        }
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
        // A fault the chief is asked about is a wait already.
        let asked = waiting.iter().any(|(wait, by)| {
            by.is_some_and(|by| by.0 == agent)
                && matches!(wait.detail, WaitDetail::Fault { .. })
        });
        if !asked {
            toasts.push(
                What::Notice {
                    kind: StatusKind::Danger,
                    head: "Something went wrong".into(),
                    line: format!("{name} ran into an error."),
                },
                now,
            );
        }
    }
}

/// The setup cause of `wait`, when it is a fault of one.
fn setup_of(wait: &Waiting) -> Option<&Setup> {
    match &wait.detail {
        WaitDetail::Fault { setup } => setup.as_ref(),
        _ => None,
    }
}

/// Raises a toast when the Hand lets a profile in or turns it away.
fn judged(
    time: Res<Time>,
    seen: Option<Res<Seen>>,
    profiles: Query<&Profile>,
    mut toasts: ResMut<Toasts>,
    mut before: Local<HashMap<ProfileId, Arrival>>,
    mut primed: Local<bool>,
) {
    if seen.is_none() {
        return;
    }
    let now = time.elapsed_secs();
    for profile in &profiles {
        let profile = &profile.0;
        let was = before.insert(profile.id, profile.arrival);
        let by_hand = profile.arrival_by.as_deref() == Some(HAND);
        if !*primed || !by_hand || was != Some(Arrival::Waiting) {
            continue;
        }
        let housed = match profile.arrival {
            Arrival::Housed => true,
            Arrival::Turned => false,
            Arrival::Waiting | Arrival::Apart => continue,
        };
        toasts.push(
            What::Verdict {
                profile: profile.id,
                name: profile.name.clone(),
                housed,
            },
            now,
        );
    }
    *primed = true;
}

/// The command that takes back the Hand's verdict on `profile`.
fn undo_of(profile: ProfileId, housed: bool) -> String {
    let address = hamlet_client::realm::profile_address(profile);
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
            head: "Something went wrong".into(),
            line: fault.text.clone(),
        },
        time.elapsed_secs(),
    );
}

/// The commands whose rejection, or response under a head, is
/// toasted.
#[derive(Resource, Default)]
pub(crate) struct Decreed(HashMap<RequestId, Option<String>>);

fn watched(watch: On<WatchRequest>, mut decreed: ResMut<Decreed>) {
    decreed.0.insert(watch.request.clone(), watch.head.clone());
}

fn toasted(
    toast: On<hamlet_core::toasts::Toast>,
    time: Res<Time>,
    mut toasts: ResMut<Toasts>,
) {
    toasts.note(
        toast.kind,
        toast.head.clone(),
        toast.line.clone(),
        time.elapsed_secs(),
    );
}

/// Toasts the response to a watched command.
fn responded(
    response: On<Response>,
    time: Res<Time>,
    mut decreed: ResMut<Decreed>,
    mut toasts: ResMut<Toasts>,
) {
    let Some(head) = decreed.0.remove(&response.to) else {
        return;
    };
    let (kind, head, line) = match (&response.message, head) {
        (ServerMessage::Reply(reply), _)
            if reply.kind == ReplyKind::Error =>
        {
            (
                StatusKind::Danger,
                "Not done".into(),
                reply.text.clone(),
            )
        }
        (ServerMessage::Fault(fault), _) => (
            StatusKind::Danger,
            "Not done".into(),
            fault.text.clone(),
        ),
        (ServerMessage::Reply(reply), Some(head)) => {
            (StatusKind::Success, head, reply.text.clone())
        }
        _ => return,
    };
    toasts
        .push(What::Notice { kind, head, line }, time.elapsed_secs());
}

/// An answer from a toast or anywhere else takes its toast away.
fn answered(answer: On<Answer>, mut toasts: ResMut<Toasts>) {
    let wait = answer.wait;
    toasts.shown.retain(|toast| toast.what != What::Wait(wait));
}

/// Opens the stack on hover, else fades the toasts and drops the
/// done.
fn keep(
    time: Res<Time>,
    pile: Query<&Hovered, With<Pile>>,
    waiting: Query<(), With<Waiting>>,
    hubs: Query<(&Hub, &Showing, Option<&Sheet>, &ChatWaits)>,
    tree: Option<Res<DockTree>>,
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
    // A Hub's chat, open in a tab in front, shows the waits it holds
    // in full; its Details sheet hides them.
    let in_full = |wait: Entity| {
        hubs.iter().any(|(hub, showing, sheet, waits)| {
            *showing == Showing::Thread
                && sheet != Some(&Sheet::Details)
                && waits.holds(wait)
                && tree
                    .as_ref()
                    .is_none_or(|tree| is_in_front(tree, &hub.0))
        })
    };
    let mut next = toasts.shown.clone();
    for toast in &mut next {
        if open {
            toast.until = toast.until.max(now + STAYS);
        }
        toast.opacity =
            ((toast.until + FADES - now) / FADES).clamp(0.0, 1.0);
        if let What::Setup { waits, .. } = &mut toast.what {
            waits.retain(|wait| waiting.contains(*wait));
        }
    }
    next.retain(|toast| {
        toast.opacity > 0.0
            && match &toast.what {
                // Its chat, open, shows it in full.
                What::Wait(wait) => {
                    waiting.contains(*wait) && !in_full(*wait)
                }
                What::Setup { waits, .. } => !waits.is_empty(),
                What::Notice { .. }
                | What::Verdict { .. }
                | What::Update(_) => true,
            }
    });
    if next != toasts.shown {
        toasts.shown = next;
    }
}

/// Eases each toast towards its place and notes its height.
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
    // Most of the way in the theme's fold time; at once without one.
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

/// `now` a step of `dt` seconds closer to `want`, at `rate`.
fn ease(now: f32, want: f32, rate: f32, dt: f32) -> f32 {
    let next = now + (want - now) * (1.0 - (-rate * dt).exp());
    if (want - next).abs() < 0.005 {
        want
    } else {
        next
    }
}

/// The stack of toasts at the bottom left of the window.
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
            .within(column(())),
        ))
        .gap(space.gap)
        .width(px(WIDTH))
        .position(PositionType::Absolute)
        .inset(UiRect {
            left: px(space.panel + space.gap),
            top: Val::Auto,
            right: Val::Auto,
            bottom: px(space.bar),
        })
        .with(Hovered::default())
        .with(Pile)
    })
}

/// "Close All", above an open stack of more than one toast.
fn closing_all() -> impl View {
    row((action(Glyph::Cross, "Close All", dismiss_all),))
        .justify(JustifyContent::FlexEnd)
        .display(resource::<Toasts, _>(|toasts| {
            if toasts.open && toasts.shown.len() > 1 {
                Display::Flex
            } else {
                Display::None
            }
        }))
}

/// A prop of toast `id` that follows its pose and height.
fn posed<T: Send + Sync + 'static>(
    id: u64,
    pick: impl Fn(Pose, Option<f32>) -> T + Send + Sync + 'static,
) -> Signal<T> {
    resource::<Toasts, _>(move |toasts| match toasts.get(id) {
        Some(toast) => pick(toast.pose, toast.tall),
        None => pick(Pose::of(Place::Hidden), None),
    })
}

/// A toast's line without the name its wait starts with.
fn said(need: &Need) -> &str {
    need.line
        .strip_prefix(need.shown.as_str())
        .unwrap_or(&need.line)
        .trim_start()
}

/// Where who raised `need` is and how long ago it asked.
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
            What::Wait(wait) => {
                let Some(waiting) = cx.world.get::<Waiting>(*wait)
                else {
                    return cx.build(column(()));
                };
                let need = Need::of(*wait, waiting);
                let kind = match need.detail {
                    WaitDetail::Fault { .. } => StatusKind::Danger,
                    _ => Standing::Waits.kind(),
                };
                let from = origin(cx.world, &need, &waiting.by);
                let answers = answers(None, &need);
                (
                    // The bell of NEEDS YOU.
                    Standing::Waits.glyph(),
                    Prop::from(need.shown.clone()),
                    from,
                    kind,
                    said(&need).to_string(),
                    answers,
                )
            }
            What::Setup { setup, .. } => {
                let head = derived(move |world: &World| {
                    setup_head(world, id)
                })
                .when(head_moved(id));
                let line = match (setup.cause, setup.model.is_empty())
                {
                    (SetupCause::Model, true) => {
                        "No model is set up yet.".to_string()
                    }
                    (SetupCause::Model, false) => {
                        "It is not set up, or not available. Set it \
                         up once for all of them."
                            .to_string()
                    }
                    (SetupCause::Credentials, _) => {
                        "Its provider refused the sign-in. Sign in \
                         again once for all of them."
                            .to_string()
                    }
                };
                let buttons = buttons(vec![
                    action(Glyph::Chip, "Set Up Models", |world| {
                        world.trigger(OpenWelcome::Again);
                    }),
                    action(
                        Glyph::Cross,
                        "Dismiss All",
                        move |world| {
                            dismiss_group(world, id);
                        },
                    ),
                ]);
                (
                    Glyph::Chip,
                    head.into(),
                    String::new(),
                    StatusKind::Danger,
                    line,
                    buttons.boxed(),
                )
            }
            What::Notice { kind, head, line } => (
                match kind {
                    StatusKind::Success => Glyph::Check,
                    _ => Glyph::Stop,
                },
                Prop::from(head.clone()),
                String::new(),
                *kind,
                line.clone(),
                column(()).boxed(),
            ),
            What::Verdict {
                profile,
                name,
                housed,
            } => {
                let undo = undo_of(*profile, *housed);
                let (head, line, kind) = if *housed {
                    (
                        "The Hand let a profile in",
                        format!("{name} has a house in the city."),
                        StatusKind::Success,
                    )
                } else {
                    (
                        "The Hand turned a profile away",
                        format!("{name} waits at the harbor."),
                        StatusKind::Warning,
                    )
                };
                (
                    Glyph::Tower,
                    Prop::from(head.to_string()),
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
                let version = &offer.version;
                let (line, which, word) = match &offer.ready {
                    Ready::File(_) => (
                        "Restart to update. Agents pause while \
                         Hamlet restarts, and carry on after.",
                        Glyph::Restore,
                        "Restart",
                    ),
                    Ready::Page(_) => (
                        "This copy cannot update itself. Download \
                         the new one from its page.",
                        Glyph::Globe,
                        "Download",
                    ),
                };
                let (line, offer) = (line.to_string(), offer.clone());
                let button = action(which, word, move |world| {
                    updates::take(world, &offer);
                });
                (
                    Glyph::Hamlet,
                    Prop::from(format!("Hamlet {version} is out")),
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
        let top = row((
            opening(what.clone(), head),
            note(resource::<Toasts, _>(move |toasts| {
                behind(toasts, id)
            })),
            close_button(move |world| dismiss(world, id)),
        ))
        .gap(6.0)
        .align(AlignItems::Center);
        let theme = cx.theme();
        let space = &theme.space;
        let words =
            column((top, opening(what.clone(), quoted(&line))))
                .gap(space.gap / 4.0);
        // Laid out in full while folded, so its height is known.
        let inside = column((words, answers))
            .gap(space.gap)
            .shrink(0.0)
            .with(Inside(id))
            .rules(fading(id));
        let mut shadow = theme.shadow();
        shadow.0.extend(theme.shadow().0);
        let (gap, round) = (space.gap, space.soft);
        let (border, padding_y) =
            (space.border, space.card_padding_y);
        // Folded, a card is a strip behind the one in front.
        let strip = (EDGE + round).max(2.0 * (padding_y + border));
        let whole = move |tall: Option<f32>| {
            tall.map_or(strip, |tall| {
                tall + 2.0 * (padding_y + border)
            })
        };
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
            .height(posed(id, move |pose, tall| {
                // In full it takes the height of what it says.
                if pose.fold == 0.0 {
                    return Val::Auto;
                }
                let whole = whole(tall);
                px(whole + (strip - whole) * pose.fold)
            }))
            .margin(posed(id, |pose, _| {
                UiRect::horizontal(px(EDGE * pose.back * pose.fold))
            }))
            .shrink(0.0)
            .tagged(shadow)
            .rules(rules::floating);
        // A negative margin would not shrink a column.
        let room = column((card,))
            .shrink(0.0)
            .height(posed(id, move |pose, tall| {
                if pose.fold == 0.0 {
                    return Val::Auto;
                }
                px((whole(tall) + gap) * (1.0 - pose.fold))
            }))
            .padding(posed(id, move |pose, _| UiRect {
                bottom: px(gap * (1.0 - pose.fold)),
                ..UiRect::ZERO
            }));
        cx.build(room)
    })
}

/// How many toasts are folded behind toast `id`, as `+2`.
fn behind(toasts: &Toasts, id: u64) -> String {
    let more = toasts.shown.len().saturating_sub(1);
    let newest =
        toasts.shown.last().is_some_and(|toast| toast.id == id);
    if toasts.open || !newest || more == 0 {
        String::new()
    } else {
        format!("+{more}")
    }
}

/// A toast's `content`, pressed to open what it is about.
fn opening(what: What, content: impl View) -> impl View {
    button(row((content,)))
        .grow(1.0)
        .min_width(px(0.0))
        .rules(opener)
        .on_activate(move |world| {
            if let What::Wait(wait) = &what {
                wait::open(world, *wait);
            }
        })
}

/// A toast's words, with no fill of their own.
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

/// Fades toast `id` with its time and as it folds.
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

/// The head of the setup toast `id`.
fn setup_head(world: &World, id: u64) -> String {
    let Some(What::Setup { setup, waits }) = world
        .get_resource::<Toasts>()
        .and_then(|toasts| toasts.get(id))
        .map(|toast| &toast.what)
    else {
        return String::new();
    };
    let agents = waits
        .iter()
        .filter_map(|wait| world.get::<RaisedBy>(*wait))
        .map(|by| by.0)
        .collect::<Vec<_>>();
    let who = match agents.as_slice() {
        [one] => hamlet_client::realm::shown_of(world, *one),
        all if all
            .iter()
            .all(|agent| world.get::<Lead>(*agent).is_some()) =>
        {
            format!("{} leads", all.len())
        }
        all => format!("{} agents", all.len()),
    };
    if setup.model.is_empty() {
        let has = if agents.len() == 1 { "has" } else { "have" };
        return format!("{who} {has} no model");
    }
    format!("{who} can't use {}", model_title(world, &setup.model))
}

/// Whether the head of the setup toast `id` may have changed.
fn head_moved(
    id: u64,
) -> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut seen = None;
    move |world: &World| {
        let waits = match world
            .get_resource::<Toasts>()
            .and_then(|toasts| toasts.get(id))
            .map(|toast| &toast.what)
        {
            Some(What::Setup { waits, .. }) => waits.len(),
            _ => 0,
        };
        let realm = world
            .get_resource_change_ticks::<Seen>()
            .map(|ticks| ticks.changed);
        let models = world
            .get_resource_change_ticks::<Shelf>()
            .map(|ticks| ticks.changed);
        // A write with no system run since gets the tick seen.
        let now = world.read_change_tick();
        let open = realm == Some(now) || models == Some(now);
        let moved = open || seen != Some((waits, realm, models));
        seen = Some((waits, realm, models));
        moved
    }
}

/// Dismisses every fault of the setup toast `id`, and the toast.
fn dismiss_group(world: &mut World, id: u64) {
    let waits = match world.resource::<Toasts>().get(id) {
        Some(Toast {
            what: What::Setup { waits, .. },
            ..
        }) => waits.clone(),
        _ => Vec::new(),
    };
    for wait in waits {
        world.trigger(Answer {
            wait,
            request: RequestId::new(),
            choice: Choice::Dismiss,
            text: None,
        });
    }
    dismiss(world, id);
}

/// Takes every toast away.
fn dismiss_all(world: &mut World) {
    let update =
        world.resource::<Toasts>().shown.iter().find_map(|toast| {
            match &toast.what {
                What::Update(offer) => Some(offer.version.clone()),
                _ => None,
            }
        });
    if let Some(version) = update {
        updates::decline(world, version);
    }
    world.resource_mut::<Toasts>().shown.clear();
}

/// Says that the page at `address` did not open.
pub(crate) fn unopened(world: &mut World, address: &str) {
    let copied = world.get_resource_mut::<Clipboard>().is_some_and(
        |mut clipboard| {
            clipboard.set_text(address.to_owned()).is_ok()
        },
    );
    let line = if copied {
        format!("Its address is copied: {address}")
    } else {
        format!("Open {address} in a browser.")
    };
    let now = world.resource::<Time>().elapsed_secs();
    world.resource_mut::<Toasts>().note(
        StatusKind::Danger,
        "The page did not open".into(),
        line,
        now,
    );
}

fn dismiss(world: &mut World, id: u64) {
    let version = world
        .resource::<Toasts>()
        .shown
        .iter()
        .find(|toast| toast.id == id)
        .and_then(|toast| match &toast.what {
            What::Update(offer) => Some(offer.version.clone()),
            _ => None,
        });
    if let Some(version) = version {
        updates::decline(world, version);
    }
    world
        .resource_mut::<Toasts>()
        .shown
        .retain(|toast| toast.id != id);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::{TimeUpdateStrategy, Virtual};

    use super::*;
    use crate::testing::panel_app;

    fn app_with_a_wait() -> App {
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
    fn a_toast_stays_under_the_pointer_and_fades_otherwise() {
        let mut app = app_with_a_wait();
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
}
