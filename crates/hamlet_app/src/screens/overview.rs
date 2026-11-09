//! The Hub's pages of the realm and of one hamlet.

mod hamlet;
mod record;
mod rows;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::{Has, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, Display, InteractionDisabled, UiRect, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, frame, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, Visual, derived, keyed, resource,
};
use hamlet_client::messages::Veto;
use hamlet_client::realm::{
    Busy, Closed, Deeds, HamletId, Identity, Left, Scholar, Treasury,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_realm::harbor::glide_to_other_pier;
use hamlet_realm::pick::{Selected, Thing, select};
use hamlet_ui::styles::{
    Fills, ICON, action, bad_action, bare, buttons, close, framed,
    group, note, raised, scroll_area, small_rules, stacked_bar,
    title,
};
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::effects::lit_panels;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::{DisplayText, Wraps};
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use self::hamlet::budget;
pub use self::hamlet::{Naming, Page, hamlet};
pub(crate) use self::hamlet::{rename, typed};
pub use self::record::Annals;
pub(crate) use self::rows::standing;
use self::rows::{
    agent_rows, hamlet_name, hamlet_rows, pressable, stale,
};
use crate::petition::Queue;
use crate::standing::{Standing, need_you};
use crate::{chat, world_menu};

pub fn plugin(app: &mut App) {
    app.add_plugins((record::plugin, hamlet::plugin))
        .init_resource::<Erasing>()
        // The realm's page lists what waits.
        .init_resource::<Queue>()
        .add_observer(disarm)
        .add_systems(Update, reconcile_erasing)
        .init_resource::<crate::forms::OpenForm>()
        .init_resource::<Page>()
        .add_systems(Update, tabs::mark::<Page>);
}

/// A heading over a part of the panel, behind its icon: larger than
/// the names of the settings under it.
pub(crate) fn heading(
    which: Glyph,
    text: impl Into<String>,
) -> AnyView {
    let text = text.into();
    with_cx(move |cx| {
        row((
            raised(glyph(which, ICON).tone(Tone::Dim)),
            label(text.clone())
                .size(cx.theme().text.heading)
                .bold(true)
                .wrap(false),
        ))
        .gap(6.0)
        .align(AlignItems::Center)
    })
    .boxed()
}

/// A [`heading`] with `end` at the far end of its line.
pub(crate) fn heading_line(
    which: Glyph,
    text: &'static str,
    end: AnyView,
) -> AnyView {
    with_cx(move |cx| {
        row((heading(which, text), frame().grow(1.0), end))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
            .width(percent(100.0))
    })
    .boxed()
}

/// The title of a part of a list, small in the display face and in
/// `tone`, as the rail heads its parts.
pub(crate) fn part_title(text: String, tone: Tone) -> impl View {
    with_cx(move |cx| {
        label(text.clone())
            .size(cx.theme().text.heading)
            .bold(true)
            .tone(tone)
            .wrap(false)
            .tagged(DisplayText)
    })
}

/// A [`part_title`] with how many there are after it, dim, as the
/// rail counts its parts.
fn counted(text: &str, count: usize) -> AnyView {
    row((
        part_title(text.to_owned(), Tone::Faint),
        label(count.to_string()).tone(Tone::Dim).wrap(false),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The size of a figure of the day.
const FIGURE: f32 = 22.0;

/// The realm at a glance: its figures, what waits for the chief in
/// full, then the hamlets, open and closed down. The hamlets and the
/// Hand are in the rail beside it too.
pub fn overview() -> impl View {
    with_cx(move |cx| {
        column((
            // Stopping every agent stands apart, at the far end.
            row((title("Realm"), frame().grow(1.0), stop_all()))
                .gap(cx.theme().space.gap)
                .align(AlignItems::Center),
            today(),
            // Ways about the world, under the figures they lead to.
            buttons(vec![
                action(
                    Glyph::Hearth,
                    "Your hearth",
                    crate::chief::go_home,
                )
                .boxed(),
                action(Glyph::Anchor, "Harbor", glide_to_other_pier)
                    .tooltip(|| note("Cross to the other island"))
                    .boxed(),
                action(
                    Glyph::Scholar,
                    "New scholar",
                    world_menu::ask_new_scholar,
                )
                .boxed(),
            ]),
            scroll_area((column((needs(), hamlets(), closed()))
                .gap(cx.theme().space.section),)),
        ))
        .fills_height()
        .gap(cx.theme().space.section)
    })
}

/// Stops every agent, as a danger button that cannot be pressed
/// while none works or waits.
fn stop_all() -> AnyView {
    keyed(
        derived(|world: &World| {
            let today = today_of(world);
            today.working + today.needs > 0
        })
        .when(bevy_fynix::every_frame()),
        |&any| {
            let stop = bad_action(Glyph::Stop, "Stop all", |world| {
                world.trigger(Veto { agent: None });
                world.flush();
            });
            if any {
                stop
            } else {
                row((stop.tagged(InteractionDisabled),))
                    .rules(faded)
                    .boxed()
            }
        },
    )
    .within(bare())
    .boxed()
}

/// See-through, as what cannot be pressed: half over dark panels,
/// and less over light ones, where the same fade leaves a red button
/// unreadable.
fn faded(cx: &mut Cx) {
    let opacity = if lit_panels(cx.theme()) { 0.85 } else { 0.5 };
    cx.set::<Visual>(move |visual, _| visual.opacity(opacity));
}

/// How the realm stands today.
#[derive(Clone, PartialEq)]
struct Today {
    needs: usize,
    working: usize,
    resting: usize,
    scholars: usize,
    harbor: usize,
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
    }
}

/// One figure of the day on a card of its own: its mark and how
/// many, large, over what they are, all in `tone`, or dim while
/// there are none, and `tip` under the pointer. The cards share the
/// row's width and are as tall as the tallest; a card that does not
/// fit beside the others goes on below.
fn figure(
    count: usize,
    which: Glyph,
    words: &'static str,
    tone: Tone,
    tip: &'static str,
) -> AnyView {
    let (mark, words_tone) = if count == 0 {
        (Tone::Dim, Tone::Dim)
    } else {
        (tone, Tone::Body)
    };
    with_cx(move |cx| {
        let theme = cx.theme();
        let space = &theme.space;
        column((
            row((
                glyph(which, ICON).tone(mark),
                label(count.to_string())
                    .size(FIGURE)
                    .bold(true)
                    .tone(words_tone)
                    .wrap(false),
            ))
            .gap(6.0)
            .align(AlignItems::Center),
            label(words)
                .tone(words_tone)
                .wrap(false)
                .rules(small_rules),
        ))
        .gap(space.gap / 2.0)
        // Less room at its sides than a card has, so that five fit a
        // narrow page.
        .padding(UiRect::axes(px(8.0), px(space.card_padding_y)))
        .radius(space.round)
        .fill(theme.palette.card)
        // As wide as its words, and the row's room left shared out.
        .grow(1.0)
        .tooltip(move || note(tip))
    })
    .boxed()
}

/// The day's figures: what waits, what works and rests, and the
/// scholars in the city and the newcomers at its harbor.
fn today() -> AnyView {
    keyed(
        derived(today_of).when(bevy_fynix::every_frame()),
        |today| {
            let (waits, works, rests) =
                (Standing::Waits, Standing::Works, Standing::Rests);
            let figures = vec![
                figure(
                    today.needs,
                    waits.glyph(),
                    need_you(today.needs),
                    waits.tone(),
                    "Petitions waiting for your answer",
                ),
                figure(
                    today.working,
                    works.glyph(),
                    "at work",
                    works.tone(),
                    "Agents working now",
                ),
                figure(
                    today.resting,
                    rests.glyph(),
                    "at rest",
                    rests.tone(),
                    "Agents resting until there is work",
                ),
                figure(
                    today.scholars,
                    Glyph::Scholar,
                    match today.scholars {
                        1 => "scholar",
                        _ => "scholars",
                    },
                    Tone::Body,
                    "Scholars living in the city",
                ),
                figure(
                    today.harbor,
                    Glyph::Anchor,
                    "arriving",
                    Tone::Body,
                    "Newcomers waiting at the harbor to be let in",
                ),
            ];
            // Closer than cards elsewhere, so their words have the
            // room.
            row(figures)
                .gap(6.0)
                .align(AlignItems::Stretch)
                .tagged(Wraps)
                .boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// What waits for the chief, in full, under its heading while there
/// is any.
fn needs() -> AnyView {
    let head = keyed(resource(Queue::len), |&count| {
        counted("Needs you", count)
    })
    .within(bare());
    with_cx(move |cx| {
        column((head, crate::petition::waiting()))
            .gap(cx.theme().space.gap)
            // Out of the page while nothing waits, its gap with it.
            .display(resource::<Queue, _>(|queue| {
                if queue.is_empty() {
                    Display::None
                } else {
                    Display::Flex
                }
            }))
    })
    .boxed()
}

/// A hamlet as its card on the realm's page.
#[derive(Clone, PartialEq)]
struct HamletCard {
    hamlet: Entity,
    title: String,
    standing: (Glyph, String, StatusKind),
    /// Its agents by what they do: waiting, working, at rest.
    mix: [usize; 3],
    mayor: Option<String>,
    account: String,
    spent: Option<f32>,
}

fn hamlet_cards(world: &World) -> Vec<HamletCard> {
    hamlet_rows(world)
        .into_iter()
        .map(|facts| {
            let agents = agent_rows(world, Some(facts.hamlet));
            let living = agents
                .iter()
                .filter(|row| !row.gone)
                .collect::<Vec<_>>();
            let how_many = |standing| {
                living
                    .iter()
                    .filter(|row| row.standing == standing)
                    .count()
            };
            let mayor = living
                .iter()
                .find(|row| row.mayor)
                .map(|row| format!("Mayor: {}", row.name));
            HamletCard {
                hamlet: facts.hamlet,
                title: facts.title,
                standing: standing(
                    living.len(),
                    how_many(Standing::Works),
                    how_many(Standing::Waits),
                ),
                mix: [
                    how_many(Standing::Waits),
                    how_many(Standing::Works),
                    living.len()
                        - how_many(Standing::Waits)
                        - how_many(Standing::Works),
                ],
                mayor,
                // A card shows its budget only once something is
                // spent.
                account: match world.get::<Treasury>(facts.hamlet) {
                    Some(t)
                        if t.0.spent.micros > 0
                            || t.0.spent.ms > 0 =>
                    {
                        facts.account
                    }
                    _ => String::new(),
                },
                spent: facts.spent,
            }
        })
        .collect()
}

/// Each hamlet on a card that opens its page.
fn hamlets() -> AnyView {
    keyed(derived(hamlet_cards).when(stale()), |cards| {
        if cards.is_empty() {
            return column(()).boxed();
        }
        let mut parts = vec![counted("Hamlets", cards.len())];
        parts.extend(cards.iter().cloned().map(|card| {
            let hamlet = card.hamlet;
            let mut lines = Vec::from_iter(
                card.mayor.map(|mayor| note(mayor).boxed()),
            );
            // Its agents as one bar, from two agents up.
            if card.mix.iter().sum::<usize>() > 1 {
                let [waits, works, rests] = card.mix;
                lines.push(
                    column((stacked_bar(vec![
                        (waits as f32, StatusKind::Attention),
                        (works as f32, StatusKind::Warning),
                        (rests as f32, StatusKind::Rest),
                    ]),))
                    .max_width(px(160.0))
                    .boxed(),
                );
            }
            if !card.account.is_empty() {
                lines.push(budget(card.account, card.spent));
            }
            pressable(
                Thing::Common(hamlet),
                card.title,
                card.standing,
                lines,
                move |world| {
                    crate::hub::show(
                        world,
                        crate::hub::Showing::Hamlet(hamlet),
                    );
                    if world.contains_resource::<Selected>() {
                        select(world, Some(Thing::Common(hamlet)));
                    }
                },
            )
            .boxed()
        }));
        group(parts).boxed()
    })
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
            let mut parts =
                vec![counted("Closed down", closed.len())];
            for hamlet in closed {
                let mut actions = Vec::new();
                if *restore {
                    let id = hamlet.id.clone();
                    actions.push(action(
                        Glyph::Restore,
                        "Restore",
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
                // Deleting for good stands apart, at the far end.
                actions.push(frame().grow(1.0).boxed());
                if *erase {
                    let id = hamlet.id.clone();
                    let at = hamlet.at;
                    actions.push(bad_action(
                        Glyph::Gone,
                        "Delete for good",
                        move |world| {
                            world.resource_mut::<Erasing>().0 =
                                Some((id.clone(), at));
                        },
                    ));
                }
                let mut card = vec![
                    hamlet_name(hamlet.name.clone()).boxed(),
                    note(hamlet.reason.clone()).boxed(),
                    row(actions).align(AlignItems::Center).boxed(),
                ];
                if *erase
                    && asking.as_ref()
                        == Some(&(hamlet.id.clone(), hamlet.at))
                {
                    let id = hamlet.id.clone();
                    let at = hamlet.at;
                    card.push(
                        close((
                            note(format!(
                                "Delete {} for good?",
                                hamlet.name
                            )),
                            note(
                                "Every archived lodge is deleted. \
                                 This cannot be undone.",
                            ),
                            buttons(vec![
                                bad_action(
                                    Glyph::Gone,
                                    "Delete",
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
                                    "Cancel",
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
                parts.push(framed(card).boxed());
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
        assert!(words(&mut app).iter().any(|s| s == "Closed down"));
        press(&mut app, "Restore");
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Say(s) if s.text == "/restore-hamlet mill" && s.to == protocol::AgentId(1))));
        press(&mut app, "Delete for good");
        assert!(!app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Say(s) if s.text.starts_with("/erase-hamlet"))));
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|s| s == "Delete The Mill for good?")
        );
        press(&mut app, "Delete");
        assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Say(s) if s.text == "/erase-hamlet mill" && s.to == protocol::AgentId(1))));
        app.world_mut().resource_mut::<Deeds>().0.clear();
        app.update();
        let shown = words(&mut app);
        assert!(
            !shown
                .iter()
                .any(|s| s == "Restore" || s == "Delete for good")
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
        press(&mut app, "Delete for good");
        app.update();
        press(&mut app, "Restore");
        app.update();
        assert!(
            !words(&mut app)
                .contains(&"Delete Mill for good?".into())
        );
        press(&mut app, "Delete for good");
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
                .contains(&"Delete Mill for good?".into())
        );
        press(&mut app, "Delete for good");
        app.update();
        state.closed[0].at = 200;
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert!(
            !words(&mut app)
                .contains(&"Delete Mill for good?".into())
        );
    }
}
