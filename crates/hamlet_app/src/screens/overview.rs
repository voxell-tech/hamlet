//! The Inspector with no agent selected: every hamlet, to search and
//! to open, and one hamlet with everyone in it.

mod hamlet;
mod record;
mod rows;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::{Has, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::messages::Veto;
use hamlet_client::realm::{
    Busy, Closed, Deeds, HamletId, Identity, Left, Scholar,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_realm::harbor::glide_to_other_pier;
use hamlet_realm::pick::{Thing, bring_into_view, select};
use hamlet_ui::styles::{
    Fills, ICON, action, bad_action, bare, beside, close,
    description, framed, group, note, raised, scroll_area, section,
    status_label_large, text_input, title,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, View, with_cx};

pub(crate) use self::hamlet::typed;
pub use self::hamlet::{Naming, Page, hamlet};
pub use self::record::Annals;
use self::rows::{hamlet_rows, pressable};
pub(crate) use self::rows::{stale, standing};
use crate::status::Bar;
use crate::{chat, world_menu};

pub fn plugin(app: &mut App) {
    app.add_plugins(record::plugin)
        .init_resource::<Erasing>()
        // The realm's page lists what waits.
        .init_resource::<crate::petition::Queue>()
        .add_observer(disarm)
        .add_systems(Update, reconcile_erasing)
        .init_resource::<Search>()
        .init_resource::<Naming>()
        .init_resource::<crate::forms::OpenForm>()
        .init_resource::<Page>()
        .add_systems(Update, (read_search, tabs::mark::<Page>));
}

/// The words typed in the search over the hamlets.
#[derive(Resource, Default, PartialEq)]
pub struct Search(String);

/// The search field's root, to find its editable child.
#[derive(Component)]
pub struct SearchField;

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
pub(crate) fn heading(
    which: Glyph,
    text: impl Into<String>,
) -> AnyView {
    row((
        raised(glyph(which, ICON).tone(Tone::Faint)),
        section(text.into()),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .boxed()
}

/// Every hamlet, with a search over them.
pub fn overview() -> impl View {
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
            return description("No hamlet by that name.").boxed();
        }
        let cards = rows
            .iter()
            .map(|row| {
                let hamlet = row.hamlet;
                let mut lines = vec![
                    format!(
                        "Mayor: {}",
                        hamlet_realm::pick::short(&row.mayor)
                    ),
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
                        bring_into_view(world, thing);
                    },
                )
            })
            .collect::<Vec<_>>();
        group(cards).boxed()
    })
    .within(bare());
    let search = with_cx(|cx| {
        row((
            text_field(String::new(), |_, _| {})
                .grow(1.0)
                .rules(text_input)
                .tagged(SearchField),
            keyed(
                resource::<Deeds, _>(|deeds| {
                    deeds.is_supported("found")
                }),
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
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
    });
    with_cx(move |cx| {
        column((
            title("REALM"),
            today(),
            search,
            // What waits for the chief comes first, in full.
            scroll_area((
                crate::petition::waiting(),
                hand(),
                list,
                closed(),
            )),
            beside((
                action(
                    Glyph::Hearth,
                    "YOUR HEARTH",
                    crate::chief::go_home,
                ),
                action(
                    Glyph::Notes,
                    "NEW SCHOLAR",
                    world_menu::ask_new_scholar,
                ),
                bad_action(Glyph::Stop, "STOP ALL", |world| {
                    world.trigger(Veto { agent: None });
                    world.flush();
                }),
            )),
        ))
        .fills_height()
        .gap(cx.theme().space.section)
    })
}

/// The Hand, the one agent of the whole realm, to talk to, and the
/// harbor to cross between the city and the hamlets.
fn hand() -> AnyView {
    with_cx(|cx| {
        column((
            beside((
                action(
                    Glyph::Mayor,
                    "THE HAND",
                    crate::convo::talk_to_hand,
                ),
                note(
                    "Speaks for the realm: hires, founding and its \
                     limits.",
                ),
            )),
            beside((
                action(
                    Glyph::Signpost,
                    "HARBOR",
                    glide_to_other_pier,
                ),
                note("Crosses to the other island's pier."),
            )),
        ))
        .gap(cx.theme().space.gap)
    })
    .boxed()
}

/// How the realm stands today.
#[derive(Clone, PartialEq)]
struct Today {
    needs: usize,
    working: usize,
    resting: usize,
    scholars: usize,
    harbor: usize,
    budget: String,
}

fn today_of(world: &World) -> Today {
    let needs = world
        .try_query::<&Waiting>()
        .map_or(0, |mut waits| waits.iter(world).count());
    let (mut working, mut resting) = (0, 0);
    if let Some(mut query) = world.try_query_filtered::<Has<Busy>, (
        With<Identity>,
        Without<Left>,
    )>() {
        for busy in query.iter(world) {
            if busy {
                working += 1;
            } else {
                resting += 1;
            }
        }
    }
    let (mut scholars, mut harbor) = (0, 0);
    if let Some(mut query) = world.try_query::<&Scholar>() {
        for scholar in query.iter(world).map(|scholar| &scholar.0) {
            if scholar.left.is_some() {
                continue;
            }
            match scholar.arrival {
                protocol::Arrival::Housed => scholars += 1,
                _ => harbor += 1,
            }
        }
    }
    Today {
        needs,
        working,
        resting,
        scholars,
        harbor,
        budget: world
            .get_resource::<Bar>()
            .map(|bar| bar.budget.clone())
            .unwrap_or_default(),
    }
}

/// A card of the day's figures: what waits, what works, who lives
/// in the city and what was spent.
fn today() -> AnyView {
    keyed(
        derived(today_of).when(bevy_fynix::every_frame()),
        |today| {
            let (mark, words, tone) = match today.needs {
                0 => (
                    Glyph::Check,
                    "Nothing waits for you".into(),
                    Tone::Dim,
                ),
                1 => (
                    Glyph::Bell,
                    "1 waits for you".into(),
                    Tone::Critical,
                ),
                n => (
                    Glyph::Bell,
                    format!("{n} wait for you"),
                    Tone::Critical,
                ),
            };
            let mut lines = vec![
                status_label_large(mark, words, tone).boxed(),
                status_label_large(
                    Glyph::Bolt,
                    format!("{} at work", today.working),
                    Tone::Accent,
                )
                .boxed(),
                status_label_large(
                    Glyph::Moon,
                    format!("{} at rest", today.resting),
                    Tone::Dim,
                )
                .boxed(),
                status_label_large(
                    Glyph::Hamlet,
                    format!("{} in the city", today.scholars),
                    Tone::Body,
                )
                .boxed(),
            ];
            if today.harbor > 0 {
                lines.push(
                    status_label_large(
                        Glyph::Signpost,
                        format!("{} at the harbor", today.harbor),
                        Tone::Body,
                    )
                    .boxed(),
                );
            }
            if !today.budget.is_empty() {
                lines.push(
                    status_label_large(
                        Glyph::Coins,
                        format!("Spent today: {}", today.budget),
                        Tone::Body,
                    )
                    .boxed(),
                );
            }
            framed(lines).boxed()
        },
    )
    .within(bare())
    .boxed()
}

#[derive(Resource, Default)]
struct Erasing(Option<(String, i64)>);

#[derive(Component)]
struct ClosedPanel;

fn disarm(
    _: bevy::ecs::observer::On<
        bevy::ecs::lifecycle::Remove<ClosedPanel>,
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
                world
                    .resource::<Deeds>()
                    .is_supported("restore-hamlet"),
                world
                    .resource::<Deeds>()
                    .is_supported("erase-hamlet"),
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
                                .is_supported("restore-hamlet")
                            {
                                world.resource_mut::<Erasing>().0 =
                                    None;
                                chat::command(
                                    world,
                                    format!("/restore-hamlet {id}"),
                                );
                            }
                        },
                    ));
                }
                if *erase {
                    let id = hamlet.id.clone();
                    let at = hamlet.at;
                    actions.push(bad_action(
                        Glyph::Gone,
                        "DELETE FOR GOOD",
                        move |world| {
                            world.resource_mut::<Erasing>().0 =
                                Some((id.clone(), at));
                        },
                    ));
                }
                parts.push(
                    close((
                        note(hamlet.name.clone()),
                        note(hamlet.reason.clone()),
                        hamlet_ui::styles::buttons(actions),
                    ))
                    .boxed(),
                );
                if *erase
                    && asking.as_ref()
                        == Some(&(hamlet.id.clone(), hamlet.at))
                {
                    let id = hamlet.id.clone();
                    let at = hamlet.at;
                    parts.push(
                        close((
                            note(format!(
                                "DELETE {} FOR GOOD?",
                                hamlet.name.to_uppercase()
                            )),
                            note(
                                "Every archived house is deleted. \
                                 This cannot be undone.",
                            ),
                            hamlet_ui::styles::buttons(vec![
                                bad_action(
                                    Glyph::Gone,
                                    "DELETE",
                                    move |world| {
                                        world
                                            .resource_mut::<Erasing>()
                                            .0 = None;
                                        if world
                                            .resource::<Deeds>()
                                            .is_supported(
                                                "erase-hamlet",
                                            )
                                            && world
                                                .resource::<Closed>()
                                                .0
                                                .iter()
                                                .any(|h| {
                                                    h.id == id
                                                        && h.at == at
                                                })
                                        {
                                            chat::command(
                                                world,
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
                                            .0 = None;
                                    },
                                ),
                            ]),
                        ))
                        .boxed(),
                    );
                }
            }
            group(parts).boxed()
        },
    )
    .within(bare().tagged(ClosedPanel))
    .boxed()
}

/// Gives a chief's command to `hamlet`, whoever the chat is with.
pub(crate) fn tell(world: &mut World, hamlet: Entity, text: String) {
    let Some(name) =
        world.get::<HamletId>(hamlet).map(|id| id.0.clone())
    else {
        return;
    };
    chat::decree(world, &name, text);
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client as protocol;

    use super::*;
    use crate::testing::{panel_app, words};

    #[test]
    fn closed_hamlets_restore_and_need_confirmation_to_erase() {
        use hamlet_client::outbox::Outbox;

        use crate::testing::press;
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state.deeds = hamlet_testing::realm::deeds(&[
            "restore-hamlet",
            "erase-hamlet",
        ]);
        state.closed = vec![protocol::ClosedHamlet {
            id: "mill".into(),
            name: "The Mill".into(),
            at: 100,
            by: "chief".into(),
            reason: "Finished grinding".into(),
        }];
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            overview(),
        );
        app.update();
        assert!(
            words(&mut app).iter().any(|s| s == "CLOSED DOWN (1)")
        );
        press(&mut app, "RESTORE");
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Say(s) if s.text == "/restore-hamlet mill" && s.to == protocol::AgentId(1))));
        press(&mut app, "DELETE FOR GOOD");
        assert!(!app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Say(s) if s.text.starts_with("/erase-hamlet"))));
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|s| s == "DELETE THE MILL FOR GOOD?")
        );
        press(&mut app, "DELETE");
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Say(s) if s.text == "/erase-hamlet mill" && s.to == protocol::AgentId(1))));
        app.world_mut().resource_mut::<Deeds>().0.clear();
        app.update();
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
        use crate::testing::press;
        let mut app = panel_app();
        app.add_plugins(plugin);
        let mut state = hamlet_testing::realm::state();
        state.deeds = hamlet_testing::realm::deeds(&[
            "restore-hamlet",
            "erase-hamlet",
        ]);
        state.closed = vec![protocol::ClosedHamlet {
            id: "mill".into(),
            name: "Mill".into(),
            at: 100,
            by: "chief".into(),
            reason: "Done".into(),
        }];
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        let root = bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
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
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
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
        state.closed[0].at = 200;
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert!(
            !words(&mut app)
                .contains(&"DELETE MILL FOR GOOD?".into())
        );
    }

    #[test]
    fn only_a_server_that_lists_the_deed_offers_to_move_a_town() {
        use bevy::ecs::query::With;
        use hamlet_client::realm::Deeds;
        use hamlet_realm::generator::GeneratorSettings;
        use hamlet_realm::generator::land::Land;
        use hamlet_realm::generator::society::{Ground, Society};
        use hamlet_realm::hamlet::Sited;
        use hamlet_realm::moving::Carrying;
        use hamlet_realm::palette::Palette;
        use hamlet_realm::play::Played;

        use crate::testing::press;

        let mut app = panel_app();
        app.add_plugins(plugin);
        let hamlet = app
            .world_mut()
            .query_filtered::<Entity, With<HamletId>>()
            .single(app.world())
            .unwrap();
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
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
        app.update();
        assert!(words(&mut app).iter().any(|text| text == "MOVE"));
        let mut land = Land::new(
            1,
            GeneratorSettings::default(),
            Palette::default(),
        );
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
            Ground::Town(town)
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
            protocol::ServerMessage::State(state),
        );
        let hamlet = app
            .world_mut()
            .query::<(Entity, &HamletId)>()
            .iter(app.world())
            .next()
            .unwrap()
            .0;
        *app.world_mut().resource_mut::<Page>() = Page::Record;
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
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
    }
}
