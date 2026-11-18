//! The right-click menu of a thing in the world, and its questions.

mod deed;
mod founding;
mod hire;
mod items;
pub(crate) mod model;
mod new_scholar;
mod road;

use bevy::app::{App, Update};
use bevy::ecs::component::{Component, Mutable};
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::{Rect, Vec2};
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, JustifyContent, UiRect, UiScale, px};
use bevy::window::Window;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, ShortcutAppExt,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, POPUP, button, column, frame,
    label, menu_item, overlay, popup, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::messages::Veto;
use hamlet_client::outbox::RequestId;
use hamlet_client::realm::{self as agents, Deeds};
use hamlet_realm::orders;
use hamlet_realm::pick::{Asked, AskedGround, Thing};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::keys::Keys;
use hamlet_ui::{AnyView, Cx, View, with_cx};

pub(crate) use self::deed::{Deed, question};
pub(crate) use self::founding::Founding;
pub(crate) use self::model::{ChipName, Hook, chip};
pub use self::model::{
    Favorites, LastSide, open as open_model, search as search_models,
};

/// The command that sends a popup's form or answers its question.
const SUBMIT: CommandId = CommandId("popup.submit");

pub fn plugin(app: &mut App) {
    let submit = CommandSpec {
        id: SUBMIT,
        label: "Send the form or answer yes",
        scope: POPUP,
        run: |world, _| submit(world),
        enabled: |world| world.resource::<Open>().submits(),
        repeat: false,
    };
    app.add_plugins(model::plugin)
        .init_resource::<Open>()
        .init_resource::<Founding>()
        .init_resource::<founding::FolderAsker>()
        .init_resource::<founding::Awaited>()
        .add_observer(founding::chosen)
        .init_resource::<new_scholar::Making>()
        .init_resource::<hire::Hiring>()
        .init_resource::<road::Roading>()
        .init_resource::<deed::Reason>()
        .add_command(submit, &SUBMIT_KEYS)
        .add_observer(ask_of_row)
        .add_systems(
            Update,
            (answer_ask, focus_new_fields, founding::open_founded),
        );
}

/// The keys that send a form: primary and Enter, and Control and
/// Enter.
pub(crate) const SUBMIT_KEYS: [Chord; 2] =
    [SEND_KEYS.chord(), Keys::new(KeyCode::Enter).ctrl().chord()];

/// The keys a form's sending button shows.
pub(crate) const SEND_KEYS: Keys =
    Keys::new(KeyCode::Enter).primary();

/// The keys that send a form, at the end of its buttons.
pub(crate) fn send_keys() -> AnyView {
    row((SEND_KEYS.view(),))
        .grow(1.0)
        .justify(JustifyContent::FlexEnd)
        .align(AlignItems::Center)
        .boxed()
}

/// The popup open over the world, and its place in the window.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) enum Open {
    #[default]
    Nothing,
    Menu(Thing, Vec2),
    /// The question before a deed, against what it was asked from.
    Question(Deed, Rect),
    /// The menu of the bare ground.
    Ground(Vec2),
    /// The form a hamlet is founded with.
    Founding(Vec2),
    /// The form a scholar is made in the city with.
    NewScholar(Vec2),
    /// The form a scholar is hired under a lead with.
    Hire(Entity, Vec2),
    /// The form a road from an agent is built with.
    Road(Entity, Vec2),
    /// The model an agent runs, its effort and its tier.
    Model(Entity),
}

impl Open {
    /// Whether it is a form or a question the keys can send.
    fn submits(&self) -> bool {
        matches!(
            self,
            Open::Question(..)
                | Open::Founding(_)
                | Open::NewScholar(_)
                | Open::Hire(..)
        )
    }
}

/// Does what the open popup's first button does.
fn submit(world: &mut World) {
    match world.resource::<Open>().clone() {
        Open::Question(deed, _) => deed::answer(world, &deed),
        Open::Founding(_) => founding::submit(world),
        Open::NewScholar(_) => new_scholar::submit(world),
        Open::Hire(lead, _) => hire::submit(world, lead),
        _ => {}
    }
}

/// On a form's field that takes the focus when shown.
#[derive(Component)]
pub(crate) struct TakesFocus;

fn focus_new_fields(
    added: Query<Entity, Added<TakesFocus>>,
    children: Query<&Children>,
    texts: Query<(), With<EditableText>>,
    mut focus: ResMut<InputFocus>,
) {
    for root in &added {
        let field = std::iter::once(root)
            .chain(children.iter_descendants(root))
            .find(|node| texts.contains(*node));
        if let Some(field) = field {
            focus.set(field, FocusCause::Auto);
        }
    }
}

/// A form's optional parts, folded under a line that opens them.
pub(crate) fn optional<R: Resource<Mutability = Mutable>>(
    open: fn(&R) -> bool,
    flip: fn(&mut R),
    body: impl Fn() -> AnyView + Send + Sync + 'static,
) -> impl View {
    // The line is built once and only its chevron turns.
    let line =
        row((button(
            row((
                glyph(Glyph::Chevron, 10.0)
                    .tone(Tone::Faint)
                    .rotation(resource::<R, _>(move |r| {
                        if open(r) { 180.0 } else { 90.0 }
                    })),
                label("More Options").tone(Tone::Dim).wrap(false),
            ))
            .gap(6.0)
            .align(AlignItems::Center),
        )
        .padding(UiRect::all(px(0.0)))
        .justify(JustifyContent::FlexStart)
        .rules(views::tint)
        .on_activate(move |world| {
            flip(&mut world.resource_mut::<R>());
        }),));
    let body = keyed(resource::<R, _>(open), move |open| {
        if *open { body() } else { frame().boxed() }
    })
    .within(column(()));
    with_cx(move |cx| {
        column((line, body)).gap(cx.theme().space.section)
    })
}

/// On a panel's row: the thing whose menu a right click opens.
#[derive(Component)]
pub struct Asks(pub Thing);

pub(crate) fn ask_of_row(
    press: On<PointerPress>,
    rows: Query<&Asks>,
    window: Query<&Window>,
    mut asked: ResMut<Asked>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let at = window
        .iter()
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    asked.0 = Some((row.0, at));
}

/// Opens the menu of the thing asked about, or of the ground.
pub(crate) fn answer_ask(
    mut asked: ResMut<Asked>,
    mut ground: ResMut<AskedGround>,
    mut open: ResMut<Open>,
    scale: Option<Res<UiScale>>,
) {
    // Asked at a place in the window, opened at one in the panels.
    let scale = scale.map_or(1.0, |scale| scale.0);
    let ground = ground.0.take().map(|at| at / scale);
    if let Some((thing, at)) = asked.0.take() {
        *open = Open::Menu(thing, at / scale);
        return;
    }
    let Some(at) = ground else {
        return;
    };
    // The same click also reaches the ground a moment later.
    let same = match &*open {
        Open::Menu(_, was) => was.distance(at) < 2.0,
        Open::Question(_, was) => was.min.distance(at) < 2.0,
        _ => false,
    };
    if !same {
        *open = Open::Ground(at);
    }
}

fn close(world: &mut World) {
    *world.resource_mut::<Open>() = Open::Nothing;
}

/// One line of a popup's form: `what` over its field.
fn asks<R: Resource<Mutability = Mutable>>(
    what: &'static str,
    read: fn(&R) -> &String,
    write: fn(&mut R, String),
) -> impl View {
    hamlet_ui::styles::close((
        hamlet_ui::styles::section(what),
        field::<R>(read, write),
    ))
}

/// [`asks`] whose field takes the focus when the form opens.
fn asks_first<R: Resource<Mutability = Mutable>>(
    what: &'static str,
    read: fn(&R) -> &String,
    write: fn(&mut R, String),
) -> impl View {
    hamlet_ui::styles::close((
        hamlet_ui::styles::section(what),
        field::<R>(read, write).tagged(TakesFocus),
    ))
}

/// A field of the text in resource `R` that `read` and `write` reach.
fn field<R: Resource<Mutability = Mutable>>(
    read: fn(&R) -> &String,
    write: fn(&mut R, String),
) -> impl View {
    views::text_field(
        resource::<R, _>(move |r| read(r).clone()),
        move |world, text| {
            write(&mut world.resource_mut::<R>(), text);
        },
    )
    .rules(hamlet_ui::styles::text_input)
}

/// Stops what `agent` is doing now, and gives the id of the request.
pub(super) fn stop(world: &mut World, agent: Entity) -> RequestId {
    let request = RequestId::new();
    world.trigger(Veto {
        request: request.clone(),
        agent: Some(agent),
    });
    world.flush();
    request
}

/// The place of the pointer, in the panels' pixels.
fn pointer(world: &mut World) -> Vec2 {
    let at = world
        .query::<&Window>()
        .iter(world)
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    at / world.get_resource::<UiScale>().map_or(1.0, |scale| scale.0)
}

/// Opens the menu of `thing` at the pointer.
pub(crate) fn open_for(world: &mut World, thing: Thing) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Menu(thing, at);
}

/// Opens the menu of `agent` at `at`, the mayor's if it is the first.
pub fn open_agent(world: &mut World, agent: Entity, at: Vec2) {
    let thing =
        if hamlet_client::realm::first_mayor(world) == Some(agent) {
            Thing::Keeper
        } else {
            Thing::Agent(agent)
        };
    *world.resource_mut::<Open>() = Open::Menu(thing, at);
}

/// Whether the menu of `thing` holds anything now.
pub(crate) fn has_items(world: &World, thing: Thing) -> bool {
    items::has_items(world, thing)
}

/// Opens the form a hamlet is founded with, at the pointer.
pub(crate) fn ask_found(world: &mut World) {
    if !Deeds::is_allowed(world, "create-hamlet") {
        return;
    }
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Founding(at);
}

/// Opens the new scholar form at the pointer.
pub(crate) fn ask_new_scholar(world: &mut World) {
    if !Deeds::is_allowed(world, "scholar") {
        return;
    }
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::NewScholar(at);
}

/// Opens the form that hires under `lead`, at the pointer.
pub(crate) fn ask_hire(world: &mut World, lead: Entity) {
    if !Deeds::is_allowed(world, "hire") {
        return;
    }
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Hire(lead, at);
}

/// Opens the form a road from `agent` is built with, at the pointer.
pub(crate) fn ask_road(world: &mut World, agent: Entity) {
    if !Deeds::is_allowed(world, "road") {
        return;
    }
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Road(agent, at);
}

/// Opens the popup of `agent`'s model, over its chip.
pub(crate) fn ask_model(world: &mut World, agent: Entity) {
    model::open(world, agent);
}

/// Asks before `deed` is done, at the pointer.
pub(crate) fn ask(world: &mut World, deed: Deed) {
    let at = pointer(world);
    ask_against(world, deed, Rect::from_corners(at, at));
}

/// Asks before `deed` is done, against `anchor` in the window.
pub(crate) fn ask_against(
    world: &mut World,
    deed: Deed,
    anchor: Rect,
) {
    // A reason typed for an earlier question is not this one's.
    if let Some(mut reason) = world.get_resource_mut::<deed::Reason>()
    {
        *reason = deed::Reason::default();
    }
    *world.resource_mut::<Open>() = Open::Question(deed, anchor);
}

/// Brings back `agent`, which was removed or merged away.
pub(super) fn restore(world: &mut World, agent: Entity) {
    if !Deeds::is_allowed(world, "restore") {
        return;
    }
    let text = format!("/restore {}", agents::name_of(world, agent));
    orders::decree(world, agent, text);
}

/// A menu of `items` at `at`.
fn menu(at: Vec2, items: Vec<AnyView>) -> impl View {
    with_cx(move |cx| {
        popup(Rect::from_corners(at, at), items)
            .on_dismiss(|world, _| close(world))
            .min_width(px(cx.theme().space.menu_width))
    })
}

/// The menu or the question that is open, over everything.
pub fn view() -> AnyView {
    overlay((menus(),)).boxed()
}

fn menus() -> AnyView {
    keyed(
        derived(|world| {
            (
                world.resource::<Open>().clone(),
                world
                    .get_resource::<Deeds>()
                    .map(|deeds| deeds.0.clone()),
            )
        })
        .when(every_frame()),
        |(open, _)| match open.clone() {
            Open::Nothing => frame().boxed(),
            Open::Menu(thing, at) => {
                AnyView::new(move |cx: &mut Cx| {
                    let items = items::items(cx.world, thing, at);
                    if items.is_empty() {
                        return cx.build(frame());
                    }
                    cx.build(menu(at, items))
                })
            }
            Open::Question(deed, anchor) => {
                question(deed, anchor).boxed()
            }
            Open::Ground(at) => AnyView::new(move |cx: &mut Cx| {
                if !Deeds::is_allowed(cx.world, "create-hamlet") {
                    return cx.build(frame());
                }
                let found =
                    menu_item(label("Create a Hamlet").wrap(false))
                        .on_activate(move |world| {
                            *world.resource_mut::<Open>() =
                                Open::Founding(at);
                        })
                        .boxed();
                cx.build(menu(at, vec![found]))
            }),
            Open::Founding(at) => founding::founding(at).boxed(),
            Open::NewScholar(at) => {
                new_scholar::new_scholar(at).boxed()
            }
            Open::Hire(lead, at) => hire::hire(lead, at).boxed(),
            Open::Road(agent, at) => road::road(agent, at).boxed(),
            Open::Model(agent) => model::picker(agent).boxed(),
        },
    )
    .within(overlay(()))
    .boxed()
}
