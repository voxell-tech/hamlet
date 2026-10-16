//! The Tinker's Stall: plugins that add to Hamlet itself, found by
//! their words and their shelf, each on a page of its own with what
//! it adds, what it asks to be let do and what changed in it.

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
use hamlet_model::market::plugins::{
    Addition, Contraption, Kind, Part, Permission, contraptions,
};
use hamlet_model::market::{Order, Price, Seller, Standing, shown};
use hamlet_world::bazaar::Traded;

use super::artisans::{
    chip, count, deal_box, finder, heading, named, orders, part,
    pressed_card, rating, said, terms, typed,
};
use crate::icons::{Glyph, glyph};
use crate::looks::{
    action, bare, buttons, good_action, note, page_head, stated,
};
use crate::platform::Wraps;
use crate::roll::moved;
use crate::{AnyView, Cx, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Tinkering>()
        .add_systems(Update, read_words);
}

/// What the chief is looking at in the Tinker's Stall, and what they
/// installed.
#[derive(Resource)]
struct Tinkering {
    all: Vec<Contraption>,
    words: String,
    kind: Option<Kind>,
    order: Order,
    /// The plugin open on a page of its own, by its id.
    open: Option<String>,
    /// Installed while the app runs. Nothing is added to Hamlet yet.
    installed: Vec<String>,
    said: Option<String>,
}

impl Default for Tinkering {
    fn default() -> Self {
        Tinkering {
            all: contraptions(),
            words: String::new(),
            kind: None,
            order: Order::default(),
            open: None,
            installed: Vec::new(),
            said: None,
        }
    }
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
    kind: Kind,
    installed: bool,
}

fn cards(world: &World) -> Vec<Card> {
    let tinkering = world.resource::<Tinkering>();
    let kept = |contraption: &Contraption| {
        tinkering.kind.is_none_or(|kind| contraption.kind == kind)
    };
    shown(&tinkering.all, &tinkering.words, kept, tinkering.order)
        .into_iter()
        .map(|contraption| Card {
            id: contraption.id.clone(),
            name: contraption.name.clone(),
            publisher: contraption.publisher.clone(),
            summary: contraption.summary.clone(),
            price: contraption.price,
            standing: contraption.standing,
            kind: contraption.kind,
            installed: tinkering.installed.contains(&contraption.id),
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
    let list =
        keyed(derived(cards).when(moved::<Tinkering>()), |cards| {
            if cards.is_empty() {
                return note("No plugins match your search.").boxed();
            }
            column(cards.iter().map(card).collect::<Vec<_>>())
                .gap(8.0)
                .boxed()
        })
        .within(bare());
    column((search, kinds, list)).gap(12.0).boxed()
}

fn card(card: &Card) -> AnyView {
    let id = card.id.clone();
    let mut foot = vec![
        rating(card.standing, 11.0),
        note(format!("{} installs", count(card.standing.owners)))
            .size(11.0)
            .boxed(),
    ];
    if card.installed {
        foot.push(
            row((stated(
                Glyph::Check,
                "INSTALLED",
                Tone::Accent,
                11.0,
            ),))
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
        let mut tinkering = world.resource_mut::<Tinkering>();
        tinkering.open = Some(id.clone());
        tinkering.said = None;
    })
}

/// The plugin `id` on a page of its own.
fn contraption_page(id: String) -> AnyView {
    let found = derived(move |world: &World| {
        let tinkering = world.resource::<Tinkering>();
        tinkering.all.iter().find(|c| c.id == id).cloned()
    })
    .when(moved::<Tinkering>());
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
        label(c.summary.clone()).size(13.0).boxed(),
        install(c),
        label(c.description.clone())
            .size(12.0)
            .tone(Tone::Dim)
            .boxed(),
        part("Adds", column(adds).gap(8.0).boxed()),
    ];
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

/// One thing a plugin adds, behind the mark of its part.
fn addition(addition: &Addition) -> AnyView {
    let mark = match addition.part {
        Part::Panel => Glyph::Minimal,
        Part::Tool => Glyph::Robot,
        Part::Command => Glyph::Play,
        Part::InChat => Glyph::Talk,
        Part::Theme => Glyph::Sun,
    };
    row((
        row((glyph(mark, 14.0).tone(Tone::Faint),))
            .padding(UiRect::top(px(2.0))),
        column((
            row((
                label(addition.name.clone())
                    .size(12.0)
                    .bold(true)
                    .wrap(false),
                note(addition.part.name()).size(11.0).wrap(false),
            ))
            .gap(8.0)
            .align(AlignItems::Center)
            .tagged(Wraps),
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

/// The box a plugin is installed, or bought, from.
fn install(contraption: &Contraption) -> AnyView {
    let (id, name) =
        (contraption.id.clone(), contraption.name.clone());
    let price = contraption.price;
    let terms = terms(price, &contraption.publisher);
    let now = {
        let id = id.clone();
        derived(move |world: &World| {
            let tinkering = world.resource::<Tinkering>();
            (
                tinkering.installed.contains(&id),
                tinkering.said.clone(),
            )
        })
        .when(moved::<Tinkering>())
    };
    keyed(now, move |(installed, words)| {
        let deal = if *installed {
            let id = id.clone();
            row((
                stated(Glyph::Check, "INSTALLED", Tone::Accent, 12.0),
                action(Glyph::Cross, "REMOVE", move |world| {
                    let mut tinkering =
                        world.resource_mut::<Tinkering>();
                    tinkering.installed.retain(|kept| *kept != id);
                    tinkering.said = None;
                }),
            ))
            .gap(10.0)
            .align(AlignItems::Center)
            .tagged(Wraps)
            .boxed()
        } else {
            let (id, name) = (id.clone(), name.clone());
            let get = move |world: &mut World| {
                let mut tinkering = world.resource_mut::<Tinkering>();
                tinkering.installed.push(id.clone());
                tinkering.said = Some(format!(
                    "Cogsworth bolts {name} on with a flourish. \
                     Installing is pretend for now: Hamlet gains \
                     nothing yet, and nothing was paid."
                ));
                // The merchant in the world cheers the sale.
                world.trigger(Traded);
            };
            match price {
                Price::Free => {
                    good_action(Glyph::Plus, "INSTALL", get)
                }
                _ => good_action(Glyph::Coins, "BUY", get),
            }
        };
        let terms = terms.clone();
        let words = words.clone();
        with_cx(move |cx| {
            column((deal_box(cx, price, terms, deal), said(words)))
                .gap(8.0)
                .boxed()
        })
    })
    .within(bare())
    .boxed()
}
