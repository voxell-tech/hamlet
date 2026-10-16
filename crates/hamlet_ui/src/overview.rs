//! The Inspector with no agent selected: every hamlet, to search and
//! to open, and one hamlet with everyone in it.

mod hamlet;
mod record;
mod rows;

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row, scroll, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::realm::{
    Activity, Closed, Deeds, HOME, LivesIn, Settlement, Treasury,
};
use hamlet_client::waits::Waiting;
use hamlet_world::pick::{Thing, focus_on, select};

pub use self::hamlet::{Naming, Page, hamlet};
pub use self::record::Annals;
use self::rows::{hamlet_rows, pressable};
pub(crate) use self::rows::{stale, standing};
use crate::icons::{Glyph, glyph};
use crate::looks::{
    Fills, ICON, action, bare, note, raised, title, well,
};
use crate::platform::DisplayText;
use crate::{AnyView, chat, tabs, world_menu};

pub fn plugin(app: &mut App) {
    app.add_plugins(record::plugin)
        .init_resource::<Erasing>()
        .add_observer(disarm)
        .add_systems(Update, reconcile_erasing)
        .init_resource::<Search>()
        .init_resource::<Naming>()
        .init_resource::<Page>()
        .add_systems(Startup, register)
        .add_systems(Update, (read_search, tabs::mark::<Page>));
}

/// The words typed in the search over the hamlets.
#[derive(Resource, Default, PartialEq)]
pub struct Search(String);

/// The search field's root, to find its editable child.
#[derive(Component)]
pub struct SearchField;

/// Registers the components these views read, so that a shared
/// reference can query them before anything has them.
pub fn register(world: &mut World) {
    hamlet_client::realm::register(world);
    world.register_component::<Settlement>();
    world.register_component::<LivesIn>();
    world.register_component::<Activity>();
    world.register_component::<Treasury>();
    world.register_component::<Waiting>();
}

/// Follows the search as it is typed.
pub fn read_search(
    root: Query<&Children, With<SearchField>>,
    texts: Query<&EditableText>,
    mut search: ResMut<Search>,
) {
    // With no field the search stays as it is.
    let Some(text) = root
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != search.0.as_str() {
        search.0 = text.value().to_string();
    }
}

/// A heading over a part of the panel, behind its icon.
pub(super) fn heading(
    which: Glyph,
    text: impl Into<String>,
) -> AnyView {
    row((
        raised(glyph(which, ICON).tone(Tone::Faint)),
        label(text.into())
            .size(11.0)
            .bold(true)
            .tone(Tone::Faint)
            .tagged(DisplayText),
    ))
    .gap(7.0)
    .align(AlignItems::Center)
    .boxed()
}

/// Every hamlet, with a search over them.
pub fn overview() -> AnyView {
    let shown = derived(|world: &World| {
        let search =
            world.resource::<Search>().0.trim().to_lowercase();
        hamlet_rows(world)
            .into_iter()
            .filter(|row| {
                search.is_empty()
                    || row.title.to_lowercase().contains(&search)
                    || row.id.contains(&search)
                    || row.mayor.contains(&search)
            })
            .collect::<Vec<_>>()
    })
    .when(stale());
    let list = keyed(shown, |rows| {
        if rows.is_empty() {
            return label("No hamlet by that name.")
                .tone(Tone::Dim)
                .boxed();
        }
        let cards = rows
            .iter()
            .map(|row| {
                let hamlet = row.hamlet;
                let mut lines = vec![
                    format!("Mayor: {}", row.mayor),
                    row.account.clone(),
                ];
                if row.gone > 0 {
                    lines.push(format!("{} gone", row.gone));
                }
                pressable(
                    Thing::Common(hamlet),
                    row.title.to_uppercase(),
                    row.state(),
                    lines,
                    move |world| {
                        let thing = Thing::Common(hamlet);
                        select(world, Some(thing));
                        focus_on(world, thing, false);
                    },
                )
            })
            .collect::<Vec<_>>();
        column(cards).gap(4.0).boxed()
    })
    .within(bare());
    column((
        row((
            title("HAMLETS"),
            note("Click a hamlet to see who lives there."),
        ))
        .gap(12.0)
        .align(AlignItems::Center),
        row((
            text_field(String::new(), |_, _| {})
                .grow(1.0)
                .rules(well)
                .tagged(SearchField),
            keyed(
                resource::<Deeds, _>(|deeds| deeds.has("found")),
                |allowed| {
                    if *allowed {
                        action(
                            Glyph::Plus,
                            "FOUND A HAMLET",
                            world_menu::ask_found,
                        )
                    } else {
                        column(()).boxed()
                    }
                },
            )
            .within(bare()),
        ))
        .gap(8.0)
        .align(AlignItems::Center),
        label("Search by a hamlet's name or its mayor's.")
            .size(11.0)
            .tone(Tone::Dim),
        scroll((list, closed())).fills_height(),
        row((
            action(
                Glyph::Hearth,
                "YOUR HEARTH",
                crate::chief::go_home,
            ),
            note("Where you live, and how you look."),
        ))
        .gap(10.0)
        .align(AlignItems::Center),
    ))
    .fills_height()
    .gap(8.0)
    .boxed()
}

#[derive(Resource, Default)]
struct Erasing(Option<(String, i64)>);

#[derive(Component)]
struct ClosedPanel;

fn disarm(
    _: bevy::ecs::observer::On<
        bevy::ecs::lifecycle::Remove,
        ClosedPanel,
    >,
    mut erasing: ResMut<Erasing>,
) {
    erasing.0 = None;
}

fn reconcile_erasing(
    closed: bevy::ecs::system::Res<Closed>,
    mut erasing: ResMut<Erasing>,
) {
    if erasing.0.as_ref().is_some_and(|(id, at)| {
        !closed.0.iter().any(|h| &h.id == id && h.at == *at)
    }) {
        erasing.0 = None;
    }
}

fn closed() -> AnyView {
    keyed(
        derived(|world: &World| {
            (
                world.resource::<Closed>().0.clone(),
                world.resource::<Deeds>().has("restore-hamlet"),
                world.resource::<Deeds>().has("erase-hamlet"),
                world.resource::<Erasing>().0.clone(),
            )
        })
        .when(bevy_fynix::every_frame()),
        |(closed, restore, erase, asking)| {
            if closed.is_empty() {
                return column(()).boxed();
            }
            let mut parts = vec![heading(
                Glyph::Gone,
                format!("CLOSED DOWN ({})", closed.len()),
            )];
            for hamlet in closed {
                let mut actions = Vec::new();
                if *restore {
                    let id = hamlet.id.clone();
                    actions.push(action(
                        Glyph::Restore,
                        "RESTORE",
                        move |world| {
                            if world
                                .resource::<Deeds>()
                                .has("restore-hamlet")
                            {
                                world.resource_mut::<Erasing>().0 =
                                    None;
                                chat::decree(
                                    world,
                                    HOME,
                                    format!("/restore-hamlet {id}"),
                                );
                            }
                        },
                    ));
                }
                if *erase {
                    let id = hamlet.id.clone();
                    let at = hamlet.at;
                    actions.push(crate::looks::bad_action(
                        Glyph::Gone,
                        "DELETE FOR GOOD",
                        move |world| {
                            world.resource_mut::<Erasing>().0 =
                                Some((id.clone(), at))
                        },
                    ));
                }
                parts.push(
                    column((
                        note(hamlet.name.clone()),
                        note(hamlet.reason.clone()),
                        crate::looks::buttons(actions),
                    ))
                    .gap(4.0)
                    .boxed(),
                );
                if *erase
                    && asking.as_ref()
                        == Some(&(hamlet.id.clone(), hamlet.at))
                {
                    let id = hamlet.id.clone();
                    let at = hamlet.at;
                    parts.push(
                        column((
                            note(format!(
                                "DELETE {} FOR GOOD?",
                                hamlet.name.to_uppercase()
                            )),
                            note(
                                "Every archived house is deleted. \
                                 This cannot be undone.",
                            ),
                            crate::looks::buttons(vec![
                                crate::looks::bad_action(
                                    Glyph::Gone,
                                    "DELETE",
                                    move |world| {
                                        world
                                            .resource_mut::<Erasing>()
                                            .0 = None;
                                        if world
                                            .resource::<Deeds>()
                                            .has("erase-hamlet")
                                            && world
                                                .resource::<Closed>()
                                                .0
                                                .iter()
                                                .any(|h| {
                                                    h.id == id
                                                        && h.at == at
                                                })
                                        {
                                            chat::decree(
                                                world,
                                                HOME,
                                                format!(
                                                    "/erase-hamlet \
                                                     {id}"
                                                ),
                                            );
                                        }
                                    },
                                ),
                                action(
                                    Glyph::Back,
                                    "CANCEL",
                                    |world| {
                                        world
                                            .resource_mut::<Erasing>()
                                            .0 = None
                                    },
                                ),
                            ]),
                        ))
                        .gap(6.0)
                        .boxed(),
                    );
                }
            }
            column(parts).gap(8.0).boxed()
        },
    )
    .within(bare().tagged(ClosedPanel))
    .boxed()
}

/// Gives a chief's command to `hamlet`, whoever the chat is with.
pub(super) fn tell(world: &mut World, hamlet: Entity, text: String) {
    let Some(name) = world
        .get::<Settlement>(hamlet)
        .map(|settlement| settlement.name.clone())
    else {
        return;
    };
    chat::decree(world, &name, text);
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::v0;

    use super::*;
    use crate::me::tests::{panel_app, words};

    #[test]
    fn closed_hamlets_restore_and_need_confirmation_to_erase() {
        use hamlet_client::outbox::Outbox;

        use crate::me::tests::press;
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state.deeds = Some(vec![
            "restore-hamlet".into(),
            "erase-hamlet".into(),
        ]);
        state.closed = Some(vec![v0::ClosedHamlet {
            id: "mill".into(),
            name: "The Mill".into(),
            at: 100,
            by: "chief".into(),
            reason: "Finished grinding".into(),
        }]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            overview(),
        );
        app.update();
        assert!(
            words(&mut app).iter().any(|s| s == "CLOSED DOWN (1)")
        );
        press(&mut app, "RESTORE");
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, v0::ClientMessage::Say(s) if s.text == "/restore-hamlet mill" && s.to.hamlet == "home")));
        press(&mut app, "DELETE FOR GOOD");
        assert!(!app.world().resource::<Outbox>().iter().any(|m| matches!(m, v0::ClientMessage::Say(s) if s.text.starts_with("/erase-hamlet"))));
        for _ in 0..3 {
            app.update();
        }
        assert!(
            words(&mut app)
                .iter()
                .any(|s| s == "DELETE THE MILL FOR GOOD?")
        );
        press(&mut app, "DELETE");
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, v0::ClientMessage::Say(s) if s.text == "/erase-hamlet mill" && s.to.hamlet == "home")));
        app.world_mut().resource_mut::<Deeds>().0.clear();
        for _ in 0..3 {
            app.update();
        }
        let shown = words(&mut app);
        assert!(
            !shown
                .iter()
                .any(|s| s == "RESTORE" || s == "DELETE FOR GOOD")
        );
    }

    #[test]
    fn restoring_or_leaving_the_closed_list_disarms_permanent_deletion()
     {
        use crate::me::tests::press;
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state.deeds = Some(vec![
            "restore-hamlet".into(),
            "erase-hamlet".into(),
        ]);
        state.closed = Some(vec![v0::ClosedHamlet {
            id: "mill".into(),
            name: "Mill".into(),
            at: 100,
            by: "chief".into(),
            reason: "Done".into(),
        }]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state.clone()),
        );
        let root = bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            closed(),
        );
        app.update();
        press(&mut app, "DELETE FOR GOOD");
        app.update();
        press(&mut app, "RESTORE");
        app.update();
        assert!(
            !words(&mut app)
                .contains(&"DELETE MILL FOR GOOD?".into())
        );
        press(&mut app, "DELETE FOR GOOD");
        app.update();
        app.world_mut().despawn(root);
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            closed(),
        );
        app.update();
        assert!(
            !words(&mut app)
                .contains(&"DELETE MILL FOR GOOD?".into())
        );
        press(&mut app, "DELETE FOR GOOD");
        app.update();
        state.closed.as_mut().unwrap()[0].at = 200;
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        app.update();
        app.update();
        assert!(
            !words(&mut app)
                .contains(&"DELETE MILL FOR GOOD?".into())
        );
    }

    #[test]
    fn a_received_hamlet_row_follows_busy_and_waiting() {
        let mut app = panel_app();
        app.add_plugins(plugin);
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            overview(),
        );
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "1 agent, all at rest")
        );
        let mut state = hamlet_testing::realm::state();
        state.agents[0].busy = true;
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state.clone()),
        );
        for _ in 0..2 {
            app.update();
        }
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "1 agent, 1 at work")
        );
        state.waits.push(hamlet_testing::realm::wait("approval"));
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        for _ in 0..2 {
            app.update();
        }
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "1 agent, 1 waiting for you")
        );
    }
    #[test]
    fn only_a_server_that_lists_the_deed_offers_to_move_a_town() {
        use bevy::ecs::query::With;
        use hamlet_client::realm::Deeds;
        use hamlet_world::generator::GenSettings;
        use hamlet_world::generator::land::Land;
        use hamlet_world::generator::society::{Place, Society};
        use hamlet_world::hamlet::Sited;
        use hamlet_world::moving::Carrying;
        use hamlet_world::palette::Palette;
        use hamlet_world::play::Played;

        use crate::me::tests::press;

        let mut app = panel_app();
        app.add_plugins(plugin);
        let hamlet = app
            .world_mut()
            .query_filtered::<Entity, With<Settlement>>()
            .single(app.world())
            .unwrap();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            self::hamlet(hamlet),
        );
        app.update();
        assert!(!words(&mut app).iter().any(|text| text == "MOVE"));
        assert!(!words(&mut app).iter().any(|text| text == "RENAME"));
        app.world_mut()
            .resource_mut::<Deeds>()
            .0
            .push("move-hamlet".into());
        for _ in 0..3 {
            app.update();
        }
        assert!(words(&mut app).iter().any(|text| text == "MOVE"));
        let mut land =
            Land::new(1, GenSettings::default(), Palette::default());
        let mut society = Society::default();
        let town = society.found_town(&mut land);
        app.insert_resource(land)
            .insert_resource(society)
            .init_resource::<Carrying>()
            .add_message::<Played>();
        app.world_mut().entity_mut(hamlet).insert(Sited(town));
        press(&mut app, "MOVE");
        assert_eq!(
            app.world().resource::<Carrying>().0.unwrap().place,
            Place::Town(town)
        );
    }

    #[test]
    fn the_record_shows_received_treasury_lines_without_disk_records()
    {
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state.hamlets[0].budget.lines = vec![
            "The server's first budget line.".into(),
            "The server's second budget line.".into(),
        ];
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        let hamlet = app
            .world_mut()
            .query::<(Entity, &Settlement)>()
            .iter(app.world())
            .next()
            .unwrap()
            .0;
        *app.world_mut().resource_mut::<Page>() = Page::Record;
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            self::hamlet(hamlet),
        );
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text == "The server's first budget line."));
        assert!(
            shown.iter().any(
                |text| text == "The server's second budget line."
            )
        );
        assert!(shown.iter().any(|text| text == "VOTES (0)"));
    }
}
