//! One listing's page: its versions, newest first, beside the version
//! chosen, or over it in a narrow desk; the version's details, facts,
//! files and what can be done with it.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, Val, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{Fills, action, bare, note};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::{MonoText, Wraps};

use super::items::source_of;
use super::look::{card, idle, state_pill};
use super::{
    Desk, Room, actions, choose, chosen, details, files, open_list,
};
use crate::bazaar::market::lot::{Lot, next_version};
use crate::bazaar::market::sell::Step;
use crate::bazaar::market::versions::{
    State, Version, before, newest_first, shown,
};
use crate::bazaar::market::{Market, Offer, Selling, sell};
use crate::bazaar::view::parts::{crumb, problem, short};
use crate::roll::{either, moved};

/// The width of the column of versions.
const SIDE: f32 = 200.0;

/// The most letters of a title in the page's head.
const HEAD: usize = 40;

/// The page of the listing `id`.
pub(super) fn page(id: String) -> AnyView {
    let now = derived(|world: &World| {
        let room = *world.resource::<Room>();
        (room.narrow, chosen(world))
    })
    .when(either(
        either(moved::<Desk>(), moved::<Market>()),
        moved::<Room>(),
    ));
    let missing = id.clone();
    let body = keyed(now, move |(narrow, now)| match now {
        Some((lot, version)) => laid_out(lot, version, *narrow),
        None => column((
            crumb("SELL", open_list, "LISTING".into()),
            note(format!(
                "The listing {missing} is not yours, or is gone."
            )),
        ))
        .gap(14.0)
        .boxed(),
    })
    .within(super::filling());
    body.boxed()
}

fn laid_out(lot: &Lot, version: &Version, narrow: bool) -> AnyView {
    let title = short(&shown(lot).title.to_uppercase(), HEAD);
    let head = crumb("SELL", open_list, title);
    let content = scroll((column((
        details::details(version, narrow),
        actions::facts(version),
        files::files(
            lot.id.clone(),
            version.version.clone(),
            before(lot, &version.version)
                .map(|one| one.version.clone()),
        ),
        actions::actions(lot, version),
    ))
    .gap(14.0)
    .padding(UiRect::bottom(px(12.0))),))
    .fills_height()
    .fills_width();
    let picker = versions(lot, version, narrow);
    let middle = match narrow {
        true => {
            column((picker, content)).gap(12.0).fills_height().boxed()
        }
        false => {
            row((picker, content)).gap(16.0).fills_height().boxed()
        }
    };
    column((head, middle)).gap(14.0).fills_height().boxed()
}

/// The versions of `lot`, the one `chosen` lit, and NEW VERSION:
/// a column at the side, or a wrapping row in a narrow desk.
fn versions(lot: &Lot, chosen: &Version, narrow: bool) -> AnyView {
    let width = match narrow {
        true => Val::Auto,
        false => percent(100.0),
    };
    let all = newest_first(lot)
        .iter()
        .map(|one| {
            version_button(one, one.version == chosen.version, width)
        })
        .collect::<Vec<_>>();
    let new = new_version(lot);
    match narrow {
        true => column((row(all).gap(6.0).tagged(Wraps), new))
            .gap(8.0)
            .boxed(),
        false => column((
            column(all).gap(4.0),
            column(()).fills_height(),
            new,
        ))
        .gap(8.0)
        .width(px(SIDE))
        .min_width(px(SIDE))
        .padding(UiRect::right(px(4.0)))
        .boxed(),
    }
}

/// A version's button, `width` wide, lit when it is chosen.
fn version_button(
    version: &Version,
    lit: bool,
    width: Val,
) -> AnyView {
    let name = version.version.clone();
    let inside = row((
        label(format!("v{name}"))
            .size(12.5)
            .bold(true)
            .tone(Tone::Body)
            .wrap(false)
            .tagged(MonoText),
        column(()).fills_width(),
        state_pill(version.state),
    ))
    .gap(8.0)
    .grow(1.0)
    .align(AlignItems::Center);
    button(inside)
        .width(width)
        .rules(card(lit))
        .tagged(Variant::Default)
        .on_activate(move |world: &mut World| choose(world, &name))
        .boxed()
}

/// NEW VERSION, while no draft waits, and how its upload goes.
fn new_version(lot: &Lot) -> AnyView {
    let id = lot.id.clone();
    let drafted = newest_first(lot)
        .first()
        .is_some_and(|one| one.state == State::Draft);
    let now = derived(|world: &World| {
        let desk = world.resource::<Desk>();
        let step = world
            .resource::<Selling>()
            .0
            .as_ref()
            .filter(|_| desk.selling || desk.problem.is_some())
            .map(|sale| sale.step.clone());
        (step, desk.problem.clone())
    })
    .when(either(moved::<Desk>(), moved::<Selling>()));
    keyed(now, move |(step, why)| {
        let mut parts = vec![match (drafted, step) {
            (_, Some(Step::Packing(_))) => {
                note("Packing it...").boxed()
            }
            (_, Some(Step::Uploading | Step::Creating)) => {
                note("Uploading it...").boxed()
            }
            (true, _) => {
                idle("NEW VERSION", Some("Finish the draft first."))
            }
            (false, _) => {
                let id = id.clone();
                action(Glyph::Plus, "NEW VERSION", move |world| {
                    upload(world, &id);
                })
            }
        }];
        if let Some(Step::Failed(why)) = step {
            parts.push(problem(why.clone()));
        }
        parts.extend(why.clone().map(problem));
        column(parts).gap(6.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// Packs what the listing `id` sells as it is in the home now, and
/// uploads it as its next draft version.
fn upload(world: &mut World, id: &str) {
    let Some(lot) = super::mine(world, id) else {
        return;
    };
    let Some(item) = source_of(world, &lot) else {
        world.resource_mut::<Desk>().problem = Some(format!(
            "{} is no longer in your home.",
            shown(&lot).title
        ));
        return;
    };
    let newest = newest_first(&lot)
        .first()
        .map(|one| one.version.clone())
        .unwrap_or_default();
    let mut desk = world.resource_mut::<Desk>();
    desk.selling = true;
    desk.problem = None;
    sell(
        world,
        Offer {
            id: Some(lot.id.clone()),
            kind: lot.kind,
            title: item.name,
            wares: item.wares,
            version: next_version(&newest),
        },
    );
}
