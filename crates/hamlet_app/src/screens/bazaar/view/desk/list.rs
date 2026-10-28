//! The chief's listings, a row each with + NEW LISTING over them, and
//! what they bought under them; and the seller terms agreed to before
//! the first listing.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_ui::styles::{
    Fills, bare, buttons, command, good_action, link_row, note,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{AnyView, markdown};

use super::look::{initial, plain_pill, state_pill};
use super::{close, open_listing, open_new};
use crate::bazaar::market::lot::{Lot, price_words, semver};
use crate::bazaar::market::versions::{
    newest_first, shown, standing,
};
use crate::bazaar::market::{Market, agree, disagree};
use crate::bazaar::view::parts::{
    Line, crumb, mark as accent, nothing, page_head, section,
    sections,
};
use crate::bazaar::view::trade;
use crate::bazaar::view::wares::open_lot;
use crate::roll::moved;

/// The seller terms to agree to, under the way back to the stall
/// `back`, which puts them away.
pub(super) fn terms(back: &'static str) -> AnyView {
    let terms = derived(|world: &World| {
        world.resource::<Market>().terms.clone()
    })
    .when(moved::<Market>());
    let body = keyed(terms, |terms| {
        let Some(terms) = terms else {
            return nothing();
        };
        sections(vec![
            page_head(
                "Seller terms",
                note("Agree to them before your first listing.")
                    .boxed(),
                String::new(),
                None,
            ),
            markdown::fixed(&terms.text, Tone::Body).boxed(),
            buttons(vec![
                good_action(Glyph::Check, "I AGREE", agree),
                command("CANCEL", cancel).boxed(),
            ])
            .boxed(),
        ])
    })
    .within(bare());
    column((
        crumb(back, cancel, "SELLER TERMS".into()),
        scroll((body,)).fills_height(),
    ))
    .gap(14.0)
    .fills_height()
    .boxed()
}

fn cancel(world: &mut World) {
    disagree(world);
    close(world);
}

/// The chief's listings and what they bought.
pub(super) fn page(back: &'static str) -> AnyView {
    let now = derived(|world: &World| {
        let market = world.resource::<Market>();
        let bought = market
            .library
            .iter()
            .map(|lot| (lot.clone(), kept_words(market, lot)))
            .collect::<Vec<_>>();
        (market.mine.clone(), bought, market.signed_in())
    })
    .when(moved::<Market>());
    let body = keyed(now, |(mine, bought, signed)| {
        let mut parts = Vec::new();
        if *signed {
            parts.push(match mine.is_empty() {
                true => empty(),
                false => column(
                    mine.iter().map(row_of).collect::<Vec<_>>(),
                )
                .gap(4.0)
                .boxed(),
            });
            if !bought.is_empty() {
                let owned = bought
                    .iter()
                    .map(|(lot, kept)| bought_row(lot, kept))
                    .collect::<Vec<_>>();
                parts.push(section(
                    "BOUGHT BY YOU",
                    column(owned).gap(2.0).boxed(),
                ));
            }
        }
        parts.push(trade::account("to sell what you made."));
        sections(parts)
    })
    .within(bare());
    let head = row((
        column((crumb(back, close, "SELL".into()),)).fills_width(),
        good_action(Glyph::Plus, "NEW LISTING", |world| {
            open_new(world, None);
        }),
    ))
    .gap(12.0)
    .align(AlignItems::Center);
    column((head, scroll((body,)).fills_height()))
        .gap(14.0)
        .fills_height()
        .boxed()
}

/// What the page says with nothing for sale.
fn empty() -> AnyView {
    column((
        label("NOTHING FOR SALE YET")
            .size(15.0)
            .bold(true)
            .tone(Tone::Faint)
            .tagged(DisplayText),
        note("List an agent or a hamlet from your home."),
        good_action(Glyph::Plus, "NEW LISTING", |world| {
            open_new(world, None);
        }),
    ))
    .gap(10.0)
    .align(AlignItems::Center)
    .padding(UiRect::vertical(px(48.0)))
    .boxed()
}

/// One listing of the chief's, which opens its page.
fn row_of(lot: &Lot) -> AnyView {
    let details = shown(lot);
    let newest = newest_first(lot)
        .first()
        .map_or(String::new(), |one| format!("v{}", one.version));
    let id = lot.id.clone();
    let summary = match details.summary.is_empty() {
        true => "No summary yet".to_string(),
        false => details.summary.clone(),
    };
    let facts = row((
        plain_pill(lot.kind.name().to_lowercase()),
        label(price_words(details.price_cents))
            .size(12.0)
            .bold(true)
            .tone(Tone::Faint)
            .wrap(false),
        label(newest).size(12.0).tone(Tone::Dim).wrap(false),
        state_pill(standing(lot)),
    ))
    .gap(10.0)
    .align(AlignItems::Center)
    .tagged(hamlet_ui::widgets::platform::Wraps);
    button(
        row((
            initial(&details.title, 40.0),
            column((
                label(details.title.clone()).size(13.0).bold(true),
                label(summary)
                    .bold(false)
                    .rules(hamlet_ui::styles::note_rules),
                facts,
            ))
            .gap(4.0)
            .fills_width(),
        ))
        .gap(12.0)
        .grow(1.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(12.0), px(10.0)))
    .rules(link_row)
    .tagged(Variant::Default)
    .on_activate(move |world| open_listing(world, &id))
    .boxed()
}

/// How the bought `lot` stands in the home, in a few words, if it is
/// installed.
fn kept_words(market: &Market, lot: &Lot) -> Option<String> {
    let kept = market.bought.get(&lot.id)?;
    let latest = lot.version();
    Some(match semver(latest) > semver(&kept.version) {
        true => format!("Version {latest} is out"),
        false => format!("Installed, version {}", kept.version),
    })
}

/// One listing the chief bought, which opens its page.
fn bought_row(lot: &Lot, kept: &Option<String>) -> AnyView {
    let opened = lot.clone();
    Line {
        name: lot.title.clone(),
        price: String::new(),
        purpose: lot.summary.clone(),
        meta: format!(
            "{}, by {}",
            lot.kind.name(),
            lot.seller.username
        ),
        mark: kept.as_deref().map(accent),
    }
    .view(move |world| open_lot(world, &opened))
}
