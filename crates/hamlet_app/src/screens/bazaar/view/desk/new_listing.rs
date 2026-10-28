//! A new listing: what kind it is, as a card each, then which of the
//! home's agents or hamlets it sells, searched for, then CONTINUE,
//! which packs it and uploads it as a draft.

use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row, scroll,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    Fills, bare, buttons, command, good_action, note, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::{DisplayText, Wraps};

use super::items::{Item, items_of};
use super::look::{card, dimmed, grid, idle, initial, plain_pill};
use super::{Desk, Room, open_list};
use crate::bazaar::market::drafts::Drafts;
use crate::bazaar::market::lot::{Kind, price_words};
use crate::bazaar::market::sell::{FIRST_VERSION, Step};
use crate::bazaar::market::{Market, Offer, Selling, sell};
use crate::bazaar::view::parts::{crumb, problem, section};
use crate::roll::{either, hamlets_moved, moved};

/// The field the home's items are searched in.
#[derive(Component)]
pub(super) struct Search;

/// The new listing's page.
pub(super) fn page() -> AnyView {
    let body = column((kinds(), items()))
        .gap(20.0)
        .padding(UiRect::bottom(px(12.0)));
    column((
        crumb("SELL", open_list, "NEW LISTING".into()),
        scroll((body,)).fills_height(),
        foot(),
    ))
    .gap(14.0)
    .fills_height()
    .boxed()
}

/// The kinds of listing, a card each.
fn kinds() -> AnyView {
    let now = derived(|world: &World| {
        let fees = world.resource::<Drafts>().fees.clone();
        let narrow = world.resource::<Room>().narrow;
        (world.resource::<Desk>().new.kind, fees, narrow)
    })
    .when(either(
        either(moved::<Desk>(), moved::<Drafts>()),
        moved::<Room>(),
    ));
    let cards = keyed(now, |(chosen, fees, narrow)| {
        let fee = |kind| {
            fees.as_ref()
                .and_then(|fees| fees.of(kind))
                .map(|one| one.verify_cents)
        };
        let cards = vec![
            kind_card(
                Kind::Agent,
                "AGENT",
                "One of your agents, with its skills and character.",
                fee(Kind::Agent),
                *chosen == Some(Kind::Agent),
            ),
            kind_card(
                Kind::Hamlet,
                "HAMLET",
                "A whole hamlet: its agents, laws and look.",
                fee(Kind::Hamlet),
                *chosen == Some(Kind::Hamlet),
            ),
            dimmed(card_body(
                "PLUGIN",
                "Tools other agents can use.",
                note("Coming soon").boxed(),
            )),
        ];
        grid(cards, if *narrow { 1 } else { 3 })
    })
    .within(bare());
    section("WHAT YOU SELL", cards.boxed())
}

/// A kind's card: its name, what it is and its fee to verify.
fn kind_card(
    kind: Kind,
    name: &'static str,
    about: &'static str,
    fee: Option<u32>,
    lit: bool,
) -> AnyView {
    let fee = match fee {
        Some(cents) => {
            format!("+ optional verification {}", price_words(cents))
        }
        None => "+ optional verification".to_string(),
    };
    let body = card_body(
        name,
        about,
        label(fee).size(12.0).tone(Tone::Accent).boxed(),
    );
    button(body)
        .grow(1.0)
        .rules(card(lit))
        .tagged(Variant::Default)
        .on_activate(move |world: &mut World| {
            let mut desk = world.resource_mut::<Desk>();
            desk.new.kind = Some(kind);
            desk.new.picked = None;
        })
        .boxed()
}

fn card_body(
    name: &'static str,
    about: &'static str,
    foot: AnyView,
) -> AnyView {
    column((
        label(name).size(13.0).bold(true).tagged(DisplayText),
        label(about).size(12.0).bold(false).tone(Tone::Dim),
        foot,
    ))
    .gap(6.0)
    .boxed()
}

/// The home's agents or hamlets, searched, a card each.
fn items() -> AnyView {
    let now = derived(|world: &World| {
        let desk = world.resource::<Desk>();
        let kind = desk.new.kind?;
        let words = desk.new.search.trim().to_lowercase();
        let all = items_of(world, kind)
            .into_iter()
            .filter(|item| item.name.to_lowercase().contains(&words))
            .collect::<Vec<_>>();
        let narrow = world.resource::<Room>().narrow;
        Some((kind, all, desk.new.picked.clone(), words, narrow))
    })
    .when(either(
        either(moved::<Desk>(), moved::<Market>()),
        either(hamlets_moved(), moved::<Room>()),
    ));
    let cards = keyed(now, |now| {
        let Some((kind, all, picked, words, narrow)) = now else {
            return bare().boxed();
        };
        let noun = kind.name().to_lowercase();
        let cards = match all.is_empty() {
            true if words.is_empty() => {
                note(format!("Your home has no {noun}s to sell."))
                    .boxed()
            }
            true => {
                note(format!("No {noun}s match \"{words}\".")).boxed()
            }
            false => grid(
                all.iter()
                    .map(|item| {
                        item_card(
                            item,
                            picked.as_ref() == Some(&item.key),
                        )
                    })
                    .collect(),
                if *narrow { 1 } else { 2 },
            ),
        };
        column((cards,)).gap(0.0).boxed()
    })
    .within(bare());
    let shown =
        derived(|world: &World| world.resource::<Desk>().new.kind)
            .when(moved::<Desk>());
    let search = keyed(shown, |kind| {
        let Some(kind) = kind else {
            return bare().boxed();
        };
        let noun = kind.name().to_lowercase();
        let field = text_field(
            bevy_fynix::resource::<Desk, _>(|desk| {
                desk.new.search.clone()
            }),
            |world, words| {
                world.resource_mut::<Desk>().new.search = words;
            },
        )
        .rules(text_input)
        .tagged(Search);
        let head = match kind {
            Kind::Hamlet => "WHICH HAMLET",
            _ => "WHICH AGENT",
        };
        section(
            head,
            column((
                field,
                note(format!("Search your {noun}s by name.")),
            ))
            .gap(6.0)
            .boxed(),
        )
    })
    .within(bare());
    column((search, cards)).gap(10.0).boxed()
}

/// An item's card: its initial, name, what it is and its counts.
fn item_card(item: &Item, lit: bool) -> AnyView {
    let mut facts = item
        .counts
        .iter()
        .map(|count| plain_pill(count.clone()))
        .collect::<Vec<_>>();
    if item.listed {
        facts.push(plain_pill("already listed"));
    }
    let body = row((
        initial(&item.name, 36.0),
        column((
            label(item.name.clone()).size(13.0).bold(true),
            label(item.about.clone())
                .size(12.0)
                .bold(false)
                .tone(Tone::Dim),
            row(facts).gap(6.0).tagged(Wraps),
        ))
        .gap(4.0)
        .fills_width(),
    ))
    .gap(10.0)
    .grow(1.0);
    if item.listed {
        return dimmed(body.boxed());
    }
    let key = item.key.clone();
    button(body)
        .grow(1.0)
        .rules(card(lit))
        .tagged(Variant::Default)
        .on_activate(move |world: &mut World| {
            world.resource_mut::<Desk>().new.picked =
                Some(key.clone());
        })
        .boxed()
}

/// The foot of the page: how the sale goes, and CONTINUE.
fn foot() -> AnyView {
    let now = derived(|world: &World| {
        let desk = world.resource::<Desk>();
        let step = world
            .resource::<Selling>()
            .0
            .as_ref()
            .filter(|_| desk.selling || desk.problem.is_some())
            .map(|sale| sale.step.clone());
        (desk.new.picked.is_some(), step, desk.problem.clone())
    })
    .when(either(moved::<Desk>(), moved::<Selling>()));
    keyed(now, |(picked, step, problem_now)| {
        let going = match step {
            Some(Step::Packing(_)) => Some("Packing it..."),
            Some(Step::Creating) => Some("Making the listing..."),
            Some(Step::Uploading) => Some("Uploading it..."),
            _ => None,
        };
        let words = match going {
            Some(going) => note(going).boxed(),
            None if *picked => {
                note("It is packed and uploaded when you continue.")
                    .boxed()
            }
            None => bare().boxed(),
        };
        let mut pressed = vec![command("CANCEL", open_list).boxed()];
        pressed.push(match (picked, going) {
            (true, None) => {
                good_action(Glyph::Into, "CONTINUE", carry_on)
            }
            _ => idle("CONTINUE", None),
        });
        let mut parts = vec![
            row((column((words,)).fills_width(), buttons(pressed)))
                .gap(12.0)
                .align(AlignItems::Center)
                .tagged(Wraps)
                .boxed(),
        ];
        if let Some(Step::Failed(why)) = step {
            parts.push(problem(why.clone()));
        }
        parts.extend(problem_now.clone().map(problem));
        column(parts).gap(6.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// Packs the item chosen and uploads it as a new listing's draft.
fn carry_on(world: &mut World) {
    let desk = world.resource::<Desk>();
    let (Some(kind), Some(key)) =
        (desk.new.kind, desk.new.picked.clone())
    else {
        return;
    };
    let Some(item) = items_of(world, kind)
        .into_iter()
        .find(|item| item.key == key)
    else {
        world.resource_mut::<Desk>().problem =
            Some("It is no longer in your home.".into());
        return;
    };
    let mut desk = world.resource_mut::<Desk>();
    desk.selling = true;
    desk.problem = None;
    sell(
        world,
        Offer {
            id: None,
            kind,
            title: item.name,
            wares: item.wares,
            version: FIRST_VERSION.into(),
        },
    );
}

/// Follows what the home's items are searched for as it is typed.
pub(super) fn searching(
    fields: Query<&Children, With<Search>>,
    texts: Query<&EditableText>,
    mut desk: ResMut<Desk>,
) {
    let typed = fields
        .iter()
        .flat_map(|children| children.iter())
        .find_map(|child| texts.get(*child).ok());
    if let Some(text) = typed
        && text.value() != desk.new.search.as_str()
    {
        desk.new.search = text.value().to_string();
    }
}
