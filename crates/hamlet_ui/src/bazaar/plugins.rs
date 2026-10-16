//! The Tinker's Stall: plugins, tools other chiefs made for every
//! agent to call, found by their words and their shelf, each on a
//! page of its own with the tools it adds, what they ask to be let do
//! and what changed in it.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, Val, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, derived, keyed, resource};
use hamlet_core::market::Market;
use hamlet_model::market::plugins::{
    Addition, Contraption, Kind, Permission,
};
use hamlet_model::market::{
    Order, Price, Seller, Standing, lot, shown,
};

use super::artisans::{
    chip, count, finder, heading, mark, named, orders, part,
    pressed_card, rating, tags, typed,
};
use super::trade;
use crate::icons::{Glyph, glyph};
use crate::looks::{bare, buttons, note, page_head, stated};
use crate::platform::Wraps;
use crate::roll::{either, moved};
use crate::{AnyView, Cx};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Tinkering>()
        .add_systems(Update, read_words);
}

/// What the chief is looking at in the Tinker's Stall.
#[derive(Resource, Default)]
struct Tinkering {
    words: String,
    kind: Option<Kind>,
    order: Order,
    /// The plugin open on a page of its own, by its listing's id.
    open: Option<String>,
}

/// The field the stall is searched in.
#[derive(Component)]
struct Finding;

/// Follows what the stall is searched for as it is typed.
fn read_words(
    field: Query<&Children, With<Finding>>,
    texts: Query<&EditableText>,
    mut tinkering: ResMut<Tinkering>,
) {
    if let Some(words) = typed(&field, &texts)
        && words != tinkering.words
    {
        tinkering.words = words;
    }
}

/// What the stall sells: its shelves, or one plugin on its page.
pub(super) fn page() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        // Drawn without the bazaar's plugin in the gallery.
        cx.world.init_resource::<Tinkering>();
        cx.world.init_resource::<Market>();
        let page = keyed(
            resource::<Tinkering, _>(|tinkering| {
                tinkering.open.clone()
            }),
            |open| match open {
                Some(id) => contraption_page(id.clone()),
                None => shelves(),
            },
        )
        .within(bare());
        cx.build(page)
    })
}

/// A plugin's card, as it is drawn.
#[derive(Clone, PartialEq)]
struct Card {
    id: String,
    name: String,
    publisher: Seller,
    summary: String,
    price: Price,
    standing: Standing,
    mark: Option<&'static str>,
}

fn cards(world: &World) -> Vec<Card> {
    let tinkering = world.resource::<Tinkering>();
    let market = world.resource::<Market>();
    let all = market
        .lots(lot::Kind::Plugin)
        .iter()
        .map(Contraption::of)
        .collect::<Vec<_>>();
    let kept = |contraption: &Contraption| {
        tinkering
            .kind
            .is_none_or(|kind| contraption.kind == Some(kind))
    };
    shown(&all, &tinkering.words, kept, tinkering.order)
        .into_iter()
        .map(|contraption| Card {
            id: contraption.id.clone(),
            name: contraption.name.clone(),
            publisher: contraption.publisher.clone(),
            summary: contraption.summary.clone(),
            price: contraption.price,
            standing: contraption.standing,
            mark: mark(market, &contraption.id),
        })
        .collect()
}

fn shelves() -> AnyView {
    let search = finder(
        resource::<Tinkering, _>(|tinkering| tinkering.words.clone()),
        |world, words| {
            world.resource_mut::<Tinkering>().words = words
        },
        Finding,
        orders::<Tinkering>(
            |tinkering| tinkering.order,
            |world, order| {
                world.resource_mut::<Tinkering>().order = order
            },
        ),
    );
    let kinds = keyed(
        resource::<Tinkering, _>(|tinkering| tinkering.kind),
        |chosen| {
            let mut all =
                vec![chip("All", chosen.is_none(), |world| {
                    world.resource_mut::<Tinkering>().kind = None;
                })];
            all.extend(Kind::ALL.into_iter().map(|kind| {
                chip(
                    kind.name(),
                    *chosen == Some(kind),
                    move |world| {
                        world.resource_mut::<Tinkering>().kind =
                            Some(kind);
                    },
                )
            }));
            buttons(all).boxed()
        },
    )
    .within(bare());
    let list = keyed(
        derived(cards)
            .when(either(moved::<Tinkering>(), moved::<Market>())),
        |cards| {
            if cards.is_empty() {
                return bare().boxed();
            }
            column(cards.iter().map(card).collect::<Vec<_>>())
                .gap(8.0)
                .boxed()
        },
    )
    .within(bare());
    column((search, kinds, trade::shelves(lot::Kind::Plugin), list))
        .gap(12.0)
        .boxed()
}

fn card(card: &Card) -> AnyView {
    let id = card.id.clone();
    let mut foot = vec![
        rating(card.standing, 11.0),
        note(format!("{} installs", count(card.standing.owners)))
            .size(11.0)
            .boxed(),
    ];
    if let Some(mark) = card.mark {
        foot.push(
            row((stated(Glyph::Check, mark, Tone::Accent, 11.0),))
                .margin(UiRect::left(Val::Auto))
                .boxed(),
        );
    }
    let body = column((
        heading(card.name.clone(), card.price),
        label(card.summary.clone()).size(12.0).tone(Tone::Dim),
        row(foot).gap(8.0).align(AlignItems::Center).tagged(Wraps),
    ))
    .gap(4.0);
    pressed_card(body, move |world| {
        world.resource_mut::<Tinkering>().open = Some(id.clone());
        trade::ask(world, &id);
    })
}

/// The plugin `id` on a page of its own.
fn contraption_page(id: String) -> AnyView {
    let found = derived(move |world: &World| {
        let market = world.resource::<Market>();
        match market.detail_of(&id) {
            Some(detail) => Some(Contraption::of_detail(detail)),
            None => market.lot(&id).map(Contraption::of),
        }
    })
    .when(moved::<Market>());
    let body = keyed(found, |contraption| match contraption {
        None => note("This plugin is no longer listed.").boxed(),
        Some(contraption) => about(contraption),
    })
    .within(bare());
    column((
        page_head(
            "PLUGINS",
            |world| world.resource_mut::<Tinkering>().open = None,
            Glyph::Bolt,
            "PLUGIN",
        ),
        body,
    ))
    .gap(16.0)
    .boxed()
}

fn about(contraption: &Contraption) -> AnyView {
    let c = contraption;
    let adds = c.adds.iter().map(addition).collect::<Vec<_>>();
    let mut parts = vec![
        column((
            named(c.name.clone()),
            row((
                rating(c.standing, 12.0),
                note(format!(
                    "{} installs  by {}",
                    count(c.standing.owners),
                    c.publisher.name
                ))
                .size(12.0),
            ))
            .gap(8.0)
            .align(AlignItems::Center)
            .tagged(Wraps),
        ))
        .gap(4.0)
        .boxed(),
        trade::cover(c.cover.clone()),
        label(c.summary.clone()).size(13.0).boxed(),
        trade::deal(
            lot::Kind::Plugin,
            c.id.clone(),
            c.price,
            &c.publisher,
        ),
        label(c.description.clone())
            .size(12.0)
            .tone(Tone::Dim)
            .boxed(),
        part("Tools it adds", column(adds).gap(8.0).boxed()),
    ];
    if !c.tags.is_empty() {
        parts.push(tags(&c.tags));
    }
    if !c.asks.is_empty() {
        let asks = c.asks.iter().map(permission).collect::<Vec<_>>();
        parts
            .push(part("Permissions", column(asks).gap(6.0).boxed()));
    }
    if let Some(change) = c.changes.first() {
        parts.push(part(
            "Latest version",
            note(format!("{}: {}", change.version, change.words))
                .boxed(),
        ));
    }
    column(parts).gap(14.0).boxed()
}

/// One tool a plugin adds.
fn addition(addition: &Addition) -> AnyView {
    row((
        row((glyph(Glyph::Robot, 14.0).tone(Tone::Faint),))
            .padding(UiRect::top(px(2.0))),
        column((
            label(addition.name.clone())
                .size(12.0)
                .bold(true)
                .wrap(false),
            note(addition.words.clone()),
        ))
        .gap(2.0)
        .grow(1.0)
        .min_width(px(0.0)),
    ))
    .gap(10.0)
    .align(AlignItems::FlexStart)
    .boxed()
}

/// Something a plugin asks to do; what could change or hear what is
/// the chief's in red.
fn permission(asks: &Permission) -> AnyView {
    let tone = if asks.grave() {
        Tone::Critical
    } else {
        Tone::Body
    };
    row((
        glyph(Glyph::Shield, 12.0).tone(tone),
        column((label(asks.words()).size(12.0).tone(tone),))
            .grow(1.0)
            .min_width(px(0.0)),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}
