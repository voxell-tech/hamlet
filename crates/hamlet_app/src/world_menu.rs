//! The menu of a thing in the world, asked for with the right button,
//! and the question before anything that ends an agent or closes a
//! hamlet.

mod deed;
mod founding;
mod hire;
mod items;
mod model;
mod new_scholar;
mod road;

use bevy::app::{App, Update};
use bevy::ecs::component::{Component, Mutable};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::ui::px;
use bevy::window::Window;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, menu_item,
    overlay, popup,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::gifts::GiftReports;
use hamlet_client::messages::Veto;
use hamlet_client::realm::{self as agents, Deeds};
use hamlet_realm::orders;
use hamlet_realm::pick::{Asked, AskedGround, Thing};
use hamlet_ui::{AnyView, Cx, View, with_cx};

pub(crate) use self::deed::{Deed, question};
pub(crate) use self::founding::Founding;
pub(crate) use self::model::chip;
pub use self::model::{
    Favorites, open as open_model, search as search_models,
};

pub fn plugin(app: &mut App) {
    app.add_plugins(model::plugin)
        .init_resource::<Open>()
        .init_resource::<Founding>()
        .init_resource::<new_scholar::Making>()
        .init_resource::<hire::Hiring>()
        .init_resource::<road::Roading>()
        .add_observer(ask_of_row)
        .add_systems(Update, answer_ask);
}

/// The popup open over the world, and its place in the window.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) enum Open {
    #[default]
    Nothing,
    Menu(Thing, Vec2),
    Question(Deed, Vec2),
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
    /// The skills an agent could be given.
    Skills(Entity, Vec2),
    /// The model an agent runs, its effort and its tier.
    Model(Entity, Vec2),
}

/// On a row of a panel: the thing it stands for, whose menu the
/// right button opens there as it does in the world.
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

/// Opens the menu of a thing that was asked about, or of the
/// ground.
pub(crate) fn answer_ask(
    mut asked: ResMut<Asked>,
    mut ground: ResMut<AskedGround>,
    mut open: ResMut<Open>,
) {
    let ground = ground.0.take();
    if let Some((thing, at)) = asked.0.take() {
        *open = Open::Menu(thing, at);
        return;
    }
    let Some(at) = ground else {
        return;
    };
    // The click that asked about a thing reaches the world's own
    // surface too, a moment later and at the same place. It is not
    // a second question about the ground under the thing.
    let same = match &*open {
        Open::Menu(_, was) | Open::Question(_, was) => {
            was.distance(at) < 2.0
        }
        _ => false,
    };
    if !same {
        *open = Open::Ground(at);
    }
}

fn close(world: &mut World) {
    *world.resource_mut::<Open>() = Open::Nothing;
}

/// One line of a popup's form: the question `what` over a field of
/// the text in resource `R` that `read` and `write` reach.
fn asks<R: Resource<Mutability = Mutable>>(
    what: &'static str,
    read: fn(&R) -> &String,
    write: fn(&mut R, String),
) -> impl View {
    hamlet_ui::styles::close((
        hamlet_ui::styles::section(what),
        bevy_fynix::views::text_field(
            bevy_fynix::resource::<R, _>(move |r| read(r).clone()),
            move |world, text| {
                write(&mut world.resource_mut::<R>(), text);
            },
        )
        .rules(hamlet_ui::styles::text_input),
    ))
}

/// Stops what `agent` is doing now.
pub(super) fn stop(world: &mut World, agent: Entity) {
    world.trigger(Veto { agent: Some(agent) });
    world.flush();
}

/// The place of the pointer in the window.
fn pointer(world: &mut World) -> Vec2 {
    world
        .query::<&Window>()
        .iter(world)
        .find_map(Window::cursor_position)
        .unwrap_or_default()
}

/// Opens the menu of `thing` at the pointer, as a right click on
/// it does.
pub(super) fn open_for(world: &mut World, thing: Thing) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Menu(thing, at);
}

/// Opens the menu of the skills `agent` could be given, at the
/// pointer.
pub(super) fn open_skills(world: &mut World, agent: Entity) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Skills(agent, at);
}

/// Opens the form a hamlet is founded with, at the pointer.
pub(super) fn ask_found(world: &mut World) {
    if !Deeds::is_allowed(world, "found") {
        return;
    }
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Founding(at);
}

/// Opens the form a scholar is made in the city with, at the
/// pointer.
pub(crate) fn ask_new_scholar(world: &mut World) {
    if !Deeds::is_allowed(world, "scholar") {
        return;
    }
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::NewScholar(at);
}

/// Opens the form a scholar is hired under `lead` with, at the
/// pointer.
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

/// Opens the popup of `agent`'s model, at the pointer.
pub(crate) fn ask_model(world: &mut World, agent: Entity) {
    let at = pointer(world);
    model::open(world, agent, at);
}

/// Asks before `deed` is done, at the pointer.
pub(crate) fn ask(world: &mut World, deed: Deed) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Question(deed, at);
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
    overlay((menus(), gift_reports())).boxed()
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
            Open::Question(deed, at) => question(deed, at).boxed(),
            Open::Ground(at) => AnyView::new(move |cx: &mut Cx| {
                if !Deeds::is_allowed(cx.world, "found") {
                    return cx.build(frame());
                }
                let found =
                    menu_item(label("Found a Hamlet...").wrap(false))
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
            Open::Model(agent, at) => {
                model::picker(agent, at).boxed()
            }
            Open::Skills(agent, at) => {
                AnyView::new(move |cx: &mut Cx| {
                    let items = items::skills(cx.world, agent);
                    cx.build(menu(at, items))
                })
            }
        },
    )
    .within(overlay(()))
    .boxed()
}

fn gift_reports() -> AnyView {
    keyed(
        derived(|world| {
            world
                .get_resource::<GiftReports>()
                .map(|reports| reports.0.clone())
                .unwrap_or_default()
        })
        .when(every_frame()),
        |reports| {
            if reports.is_empty() {
                return frame().boxed();
            }
            let lines = reports
                .iter()
                .map(|(text, failed)| {
                    label(text.clone())
                        .tone(if *failed {
                            bevy_fynix::tokens::Tone::Critical
                        } else {
                            bevy_fynix::tokens::Tone::Body
                        })
                        .boxed()
                })
                .collect::<Vec<_>>();
            with_cx(move |cx| {
                let space = &cx.theme().space;
                popup(
                    Rect::new(16.0, 52.0, 16.0, 52.0),
                    (column((
                        column(lines).gap(space.gap / 2.0),
                        hamlet_ui::styles::lettered("Dismiss")
                            .rules(hamlet_ui::styles::button)
                            .on_activate(|world| {
                                world
                                    .resource_mut::<GiftReports>()
                                    .0
                                    .clear();
                            }),
                    ))
                    .gap(space.section)
                    .width(px(360.0)),),
                )
                .padding(bevy::ui::UiRect::all(px(
                    space.overlay_padding,
                )))
            })
            .boxed()
        },
    )
    .boxed()
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_client::realm::HamletId;
    use hamlet_protocol::client as protocol;
    use hamlet_realm::pick::{Also, Selected};
    use hamlet_testing::realm;

    use super::*;
    use crate::testing::{panel_app, press, words};

    fn app(state: protocol::State) -> App {
        let mut app = panel_app();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.add_plugins((crate::chat::plugin, plugin))
            .init_resource::<Asked>()
            .init_resource::<AskedGround>()
            .init_resource::<Selected>()
            .init_resource::<Also>();
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            view(),
        );
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        app
    }

    fn open(app: &mut App, open: Open) {
        *app.world_mut().resource_mut::<Open>() = open;
        app.update();
    }

    fn sent(app: &mut App) -> Vec<protocol::ClientMessage> {
        app.world_mut().resource_mut::<Outbox>().drain().collect()
    }

    #[test]
    fn ground_and_town_offer_only_received_deeds() {
        let mut state = realm::state();
        state.deeds = realm::deeds(&["new", "stop"]);
        let mut app = app(state.clone());
        app.world_mut().resource_mut::<AskedGround>().0 =
            Some(Vec2::ZERO);
        app.update();
        assert!(
            !words(&mut app)
                .iter()
                .any(|text| text == "Found a Hamlet...")
        );
        let world = app.world_mut();
        let home = world.query_filtered::<Entity, bevy::ecs::query::With<HamletId>>()
            .single(world).unwrap();
        open(&mut app, Open::Menu(Thing::Common(home), Vec2::ZERO));
        assert!(
            !words(&mut app)
                .iter()
                .any(|text| text == "Move home...")
        );

        state.deeds = realm::deeds(&["found", "move-hamlet"]);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        open(&mut app, Open::Ground(Vec2::ZERO));
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "Found a Hamlet...")
        );
        open(&mut app, Open::Menu(Thing::Common(home), Vec2::ZERO));
        assert!(
            words(&mut app).iter().any(|text| text == "Move home...")
        );
    }
    #[test]
    fn roads_are_offered_only_when_supported_and_sent_to_the_mayor() {
        let mut state = realm::state();
        state.agents.extend([
            realm::agent(3, "home", 1),
            realm::agent(4, "home", 2),
        ]);
        let mut app = app(state.clone());
        let a = agents::named(app.world_mut(), "home#1").unwrap();
        let b = agents::named(app.world_mut(), "home#2").unwrap();
        app.world_mut().resource_mut::<Selected>().0 =
            Some(Thing::Agent(a));
        app.world_mut().resource_mut::<Also>().0 =
            Some(Thing::Agent(b));
        open(&mut app, Open::Menu(Thing::Agent(a), Vec2::ZERO));
        let shown = words(&mut app);
        for absent in [
            "End home#1...",
            "Make home#1 Mayor...",
            "Merge home#1 into home#2...",
        ] {
            assert!(
                !shown.iter().any(|text| text == absent),
                "unsupported: {absent}"
            );
        }
        state.deeds = realm::deeds(&["road"]);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        open(&mut app, Open::Nothing);
        open(&mut app, Open::Road(a, Vec2::ZERO));
        press(&mut app, "home#2");
        let messages = sent(&mut app);
        let [protocol::ClientMessage::Say(say)] = &messages[..]
        else {
            panic!("one road: {messages:?}");
        };
        assert_eq!(say.to, protocol::AgentId(2));
        assert_eq!(say.text, "/road home#1 home#2");
    }

    #[test]
    fn the_first_mayors_menu_keeps_new_and_stop() {
        let mut app = app(realm::state());

        open(&mut app, Open::Menu(Thing::Keeper, Vec2::ZERO));
        // It asks first, as the Hub does.
        press(&mut app, "New Session...");
        app.update();
        press(&mut app, "New session");
        // Its chat is opened, then the new session asked for there.
        let messages = sent(&mut app);
        let Some(protocol::ClientMessage::Say(say)) = messages.last()
        else {
            panic!("a new session last: {messages:?}");
        };
        assert_eq!(say.to, protocol::AgentId(2));
        assert_eq!(say.text, "/new");
        open(&mut app, Open::Menu(Thing::Keeper, Vec2::ZERO));
        press(&mut app, "Stop the Mayor");
        let messages = sent(&mut app);
        let [protocol::ClientMessage::Stop(stop)] = &messages[..]
        else {
            panic!("one stop: {messages:?}");
        };
        assert_eq!(stop.agent, Some(protocol::AgentId(2)));
    }

    #[test]
    fn the_first_mayors_skill_menu_sends_another_agents_skill() {
        let mut state = realm::state();
        state.deeds = realm::deeds(&["give"]);
        let mut scout = realm::agent(3, "home", 1);
        scout.skills = vec!["maps".into()];
        state.agents.push(scout);
        let mut app = app(state);
        open(&mut app, Open::Menu(Thing::Keeper, Vec2::ZERO));
        press(&mut app, "Give a skill...");
        app.update();
        press(&mut app, "maps (of home#1)");
        let messages = sent(&mut app);
        let [
            protocol::ClientMessage::Give(protocol::Give::Skill {
                skill,
                to,
                ..
            }),
        ] = &messages[..]
        else {
            panic!("one skill: {messages:?}");
        };
        assert_eq!(
            skill,
            &protocol::GiveSkillSource {
                from: protocol::Keeper::Agent(protocol::AgentId(3)),
                name: "maps".into(),
            }
        );
        assert_eq!(*to, protocol::ScholarId(2));
    }
}
