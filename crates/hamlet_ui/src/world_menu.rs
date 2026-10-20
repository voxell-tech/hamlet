//! The menu of a thing in the world, asked for with the right button,
//! and the question before anything that ends an agent or closes a
//! hamlet.

mod deed;
mod founding;
mod items;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::picking::events::{Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::ui::px;
use bevy::window::Window;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, menu_item,
    overlay, popup,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::messages::Veto;
use hamlet_client::realm::{self as agents, Deeds};
use hamlet_realm::orders;
use hamlet_realm::pick::{Asked, AskedGround, Thing};

pub(crate) use self::deed::{Deed, question};
pub(crate) use self::founding::Founding;
use crate::{AnyView, Cx, View};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Open>()
        .init_resource::<Founding>()
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
    /// The skills an agent could be given.
    Skills(Entity, Vec2),
}

/// On a row of a panel: the thing it stands for, whose menu the
/// right button opens there as it does in the world.
#[derive(Component)]
pub struct Asks(pub Thing);

pub(crate) fn ask_of_row(
    press: On<Pointer<Press>>,
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

fn has(world: &World, deed: &str) -> bool {
    world
        .get_resource::<Deeds>()
        .is_some_and(|deeds| deeds.has(deed))
}

fn close(world: &mut World) {
    *world.resource_mut::<Open>() = Open::Nothing;
}

/// Stops what `agent` is doing now.
pub(super) fn stop(world: &mut World, agent: Entity) {
    world.trigger(Veto { agent: Some(agent) });
    world.flush();
}

/// The place of the pointer in the window.
fn pointer(world: &mut World) -> Vec2 {
    world
        .query::<&bevy::window::Window>()
        .iter(world)
        .find_map(bevy::window::Window::cursor_position)
        .unwrap_or_default()
}

/// Opens the menu of `thing` at the pointer, as a right click on
/// it does.
pub(super) fn open_for(world: &mut World, thing: Thing) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Menu(thing, at);
}

/// Opens the form a hamlet is founded with, at the pointer.
pub(super) fn ask_found(world: &mut World) {
    if !has(world, "found") {
        return;
    }
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Founding(at);
}

/// Asks before `deed` is done, at the pointer.
pub(crate) fn ask(world: &mut World, deed: Deed) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Question(deed, at);
}

/// Brings back `agent`, which was removed or merged away.
pub(super) fn restore(world: &mut World, agent: Entity) {
    if !has(world, "restore") {
        return;
    }
    let text = format!("/restore {}", agents::name_of(world, agent));
    orders::decree(world, agent, text);
}

/// A menu of `items` at `at`.
fn menu(at: Vec2, items: Vec<AnyView>) -> impl View {
    popup(Rect::from_corners(at, at), items)
        .on_dismiss(|world, _| close(world))
        .min_width(px(200.0))
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
            Open::Question(deed, at) => question(deed, at),
            Open::Ground(at) => AnyView::new(move |cx: &mut Cx| {
                if !has(cx.world, "found") {
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
            Open::Founding(at) => founding::founding(at),
            Open::Skills(agent, at) => {
                AnyView::new(move |cx: &mut Cx| {
                    let items = items::skills(cx.world, agent);
                    cx.build(menu(at, items))
                })
            }
        },
    )
    .boxed()
}

fn gift_reports() -> AnyView {
    keyed(
        derived(|world| {
            world
                .get_resource::<orders::GiftReports>()
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
            popup(
                Rect::new(16.0, 52.0, 16.0, 52.0),
                (column((
                    column(lines).gap(6.0),
                    crate::looks::lettered("DISMISS")
                        .rules(crate::looks::brass)
                        .on_activate(|world| {
                            world
                                .resource_mut::<orders::GiftReports>()
                                .0
                                .clear();
                        }),
                ))
                .gap(10.0)
                .width(px(360.0)),),
            )
            .padding(bevy::ui::UiRect::all(px(14.0)))
            .boxed()
        },
    )
    .boxed()
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_client::realm::{HOME, Settlement};
    use hamlet_protocol::v0;
    use hamlet_realm::pick::{Also, Selected};
    use hamlet_testing::realm;

    use super::*;
    use crate::me::tests::{panel_app, press, words};

    fn app(state: v0::State) -> App {
        let mut app = panel_app();
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        app.add_plugins((crate::chat::plugin, plugin))
            .init_resource::<Asked>()
            .init_resource::<AskedGround>()
            .init_resource::<Selected>()
            .init_resource::<Also>();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
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

    fn sent(app: &mut App) -> Vec<v0::ClientMessage> {
        app.world_mut().resource_mut::<Outbox>().drain().collect()
    }

    #[test]
    fn ground_and_town_offer_only_received_deeds() {
        let mut state = realm::state();
        state.deeds = hamlet_testing::realm::deeds(&["new", "undo"]);
        let mut app = app(state.clone());
        app.world_mut().resource_mut::<AskedGround>().0 =
            Some(Vec2::ZERO);
        app.update();
        app.update();
        assert!(
            !words(&mut app)
                .iter()
                .any(|text| text == "Found a Hamlet...")
        );
        let world = app.world_mut();
        let home = world.query_filtered::<Entity, bevy::ecs::query::With<Settlement>>()
            .single(world).unwrap();
        open(&mut app, Open::Menu(Thing::Common(home), Vec2::ZERO));
        assert!(
            !words(&mut app)
                .iter()
                .any(|text| text == "Move home...")
        );

        state.deeds =
            hamlet_testing::realm::deeds(&["found", "move-hamlet"]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
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
            realm::agent(HOME, 1, "scout"),
            realm::agent(HOME, 2, "scribe"),
        ]);
        let mut app = app(state.clone());
        let a = agents::named(app.world_mut(), "scout").unwrap();
        let b = agents::named(app.world_mut(), "scribe").unwrap();
        app.world_mut().resource_mut::<Selected>().0 =
            Some(Thing::Resident(a));
        app.world_mut().resource_mut::<Also>().0 =
            Some(Thing::Resident(b));
        open(&mut app, Open::Menu(Thing::Resident(a), Vec2::ZERO));
        let shown = words(&mut app);
        for absent in [
            "Build a road to scribe",
            "Remove scout...",
            "Make scout Mayor...",
            "Merge scout into scribe...",
        ] {
            assert!(
                !shown.iter().any(|text| text == absent),
                "unsupported: {absent}"
            );
        }
        state.deeds = hamlet_testing::realm::deeds(&["road"]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        open(&mut app, Open::Nothing);
        open(&mut app, Open::Menu(Thing::Resident(a), Vec2::ZERO));
        press(&mut app, "Build a road to scribe");
        let messages = sent(&mut app);
        let [v0::ClientMessage::Say(say)] = &messages[..] else {
            panic!("one road: {messages:?}");
        };
        assert_eq!(
            say.to,
            v0::Address {
                hamlet: HOME.into(),
                agent: "mayor".into()
            }
        );
        assert_eq!(say.text, "/road scout scribe");
    }

    #[test]
    fn the_founders_menu_keeps_new_prompt_stop_and_undo() {
        let mut app = app(realm::state());
        let founder = agents::mayor(app.world_mut()).unwrap();
        open(&mut app, Open::Menu(Thing::Keeper, Vec2::ZERO));
        assert!(words(&mut app).iter().any(|text| text == "Prompt"));
        press(&mut app, "New Conversation");
        let messages = sent(&mut app);
        let [v0::ClientMessage::Say(say)] = &messages[..] else {
            panic!("one new: {messages:?}");
        };
        assert_eq!(
            say.to,
            v0::Address {
                hamlet: HOME.into(),
                agent: "founder".into()
            }
        );
        assert_eq!(say.text, "/new");
        open(&mut app, Open::Menu(Thing::Keeper, Vec2::ZERO));
        press(&mut app, "Stop the Mayor");
        let messages = sent(&mut app);
        let [v0::ClientMessage::Stop(stop)] = &messages[..] else {
            panic!("one stop: {messages:?}");
        };
        assert_eq!(
            stop.agent,
            agents::address_of(app.world(), founder)
        );
        open(&mut app, Open::Menu(Thing::Keeper, Vec2::ZERO));
        press(&mut app, "Undo the Last Change");
        assert!(sent(&mut app).iter().any(|message| matches!(message, v0::ClientMessage::Say(say) if say.text == "/undo")));
    }

    #[test]
    fn gift_replies_are_shown_in_words_and_can_be_dismissed() {
        let mut app = app(realm::state());
        app.insert_resource(orders::GiftReports(vec![(
            "The inbox already holds books.".into(),
            true,
        )]));
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "The inbox already holds books.")
        );
        press(&mut app, "DISMISS");
        app.update();
        assert!(
            !words(&mut app)
                .iter()
                .any(|text| text == "The inbox already holds books.")
        );
    }
    #[test]
    fn the_founders_skill_menu_sends_a_received_starter_or_agents_skill()
     {
        let mut state = realm::state();
        state.deeds = hamlet_testing::realm::deeds(&["give"]);
        state.starters = Some(vec![v0::Skill {
            name: "research".into(),
            description: "Find sources".into(),
        }]);
        let mut scout = realm::agent(HOME, 1, "scout");
        scout.skills = Some(vec!["maps".into()]);
        state.agents.push(scout);
        let mut app = app(state);
        for (label, expected) in [
            (
                "research (starter)",
                v0::GiveSkillSkill::Starter {
                    starter: "research".into(),
                },
            ),
            (
                "maps (of scout)",
                v0::GiveSkillSkill::From {
                    from: v0::Address {
                        hamlet: HOME.into(),
                        agent: "scout".into(),
                    },
                    name: "maps".into(),
                },
            ),
        ] {
            open(&mut app, Open::Menu(Thing::Keeper, Vec2::ZERO));
            press(&mut app, "Give a skill...");
            app.update();
            press(&mut app, label);
            let messages = sent(&mut app);
            let [
                v0::ClientMessage::Give(v0::Give::Skill {
                    skill,
                    to,
                    ..
                }),
            ] = &messages[..]
            else {
                panic!("one skill: {messages:?}");
            };
            assert_eq!(skill, &expected);
            assert_eq!(to.agent, "founder");
        }
    }
}
