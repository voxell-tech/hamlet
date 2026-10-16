//! The Artisans' Stall: agents other chiefs made, found by their
//! words and their craft, each on a page of its own with its maker,
//! what chiefs say of it and the way to hire it. Its cards, its
//! controls and its facts serve the Tinker's Stall too.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, dropdown, label, row,
    text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, keyed, resource, style,
};
use hamlet_model::market::artisans::{Craft, Hireling, hirelings};
use hamlet_model::market::{
    Order, Price, SELLERS_SHARE, Seller, Standing, shown,
};
use hamlet_world::bazaar::Traded;

use super::shelf::small;
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    CONTROL, Fills, ICON, action, bare, brass, buttons, good_action,
    note, pad, page_head, stated, well,
};
use crate::platform::{DisplayText, Wraps};
use crate::roll::moved;
use crate::theme::HamletTheme;
use crate::{AnyView, Cx, View, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Hiring>()
        .add_systems(Update, read_words);
}

/// What the chief is looking at in the Artisans' Stall, and whom
/// they hired.
#[derive(Resource)]
struct Hiring {
    all: Vec<Hireling>,
    words: String,
    craft: Option<Craft>,
    order: Order,
    /// The hireling open on a page of its own, by its id.
    open: Option<String>,
    /// Hired while the app runs. No agent moves in yet.
    hired: Vec<String>,
    said: Option<String>,
}

impl Default for Hiring {
    fn default() -> Self {
        Hiring {
            all: hirelings(),
            words: String::new(),
            craft: None,
            order: Order::default(),
            open: None,
            hired: Vec::new(),
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
    mut hiring: ResMut<Hiring>,
) {
    if let Some(words) = typed(&field, &texts)
        && words != hiring.words
    {
        hiring.words = words;
    }
}

/// The words in the field whose children are `field`.
pub(super) fn typed<F: Component>(
    field: &Query<&Children, With<F>>,
    texts: &Query<&EditableText>,
) -> Option<String> {
    field
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
        .map(|text| text.value().to_string())
}

/// What the stall sells: its shelves, or one hireling on its page.
pub(super) fn page() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        // Drawn without the bazaar's plugin in the gallery.
        cx.world.init_resource::<Hiring>();
        let page = keyed(
            resource::<Hiring, _>(|hiring| hiring.open.clone()),
            |open| match open {
                Some(id) => hireling_page(id.clone()),
                None => shelves(),
            },
        )
        .within(bare());
        cx.build(page)
    })
}

/// A hireling's card, as it is drawn.
#[derive(Clone, PartialEq)]
struct Card {
    id: String,
    name: String,
    maker: Seller,
    purpose: String,
    price: Price,
    standing: Standing,
    craft: Craft,
    hired: bool,
}

fn cards(world: &World) -> Vec<Card> {
    let hiring = world.resource::<Hiring>();
    let kept = |hireling: &Hireling| {
        hiring.craft.is_none_or(|craft| hireling.craft == craft)
    };
    shown(&hiring.all, &hiring.words, kept, hiring.order)
        .into_iter()
        .map(|hireling| Card {
            id: hireling.id.clone(),
            name: hireling.name.clone(),
            maker: hireling.maker.clone(),
            purpose: hireling.purpose.clone(),
            price: hireling.price,
            standing: hireling.standing,
            craft: hireling.craft,
            hired: hiring.hired.contains(&hireling.id),
        })
        .collect()
}

fn shelves() -> AnyView {
    let search = finder(
        resource::<Hiring, _>(|hiring| hiring.words.clone()),
        |world, words| world.resource_mut::<Hiring>().words = words,
        Finding,
        orders::<Hiring>(
            |hiring| hiring.order,
            |world, order| {
                world.resource_mut::<Hiring>().order = order
            },
        ),
    );
    let crafts = keyed(
        resource::<Hiring, _>(|hiring| hiring.craft),
        |chosen| {
            let mut all =
                vec![chip("All", chosen.is_none(), |world| {
                    world.resource_mut::<Hiring>().craft = None;
                })];
            all.extend(Craft::ALL.into_iter().map(|craft| {
                chip(
                    craft.name(),
                    *chosen == Some(craft),
                    move |world| {
                        world.resource_mut::<Hiring>().craft =
                            Some(craft);
                    },
                )
            }));
            buttons(all).boxed()
        },
    )
    .within(bare());
    let list =
        keyed(derived(cards).when(moved::<Hiring>()), |cards| {
            if cards.is_empty() {
                return note("No agents match your search.").boxed();
            }
            column(cards.iter().map(card).collect::<Vec<_>>())
                .gap(8.0)
                .boxed()
        })
        .within(bare());
    column((search, crafts, list)).gap(12.0).boxed()
}

fn card(card: &Card) -> AnyView {
    let id = card.id.clone();
    let mut foot = vec![
        rating(card.standing, 11.0),
        note(format!("by {}", card.maker.name)).size(11.0).boxed(),
    ];
    if card.hired {
        foot.push(
            row((stated(Glyph::Check, "HIRED", Tone::Accent, 11.0),))
                .margin(UiRect::left(bevy::ui::Val::Auto))
                .boxed(),
        );
    }
    let body = column((
        heading(card.name.clone(), card.price),
        label(card.purpose.clone()).size(12.0).tone(Tone::Dim),
        row(foot).gap(8.0).align(AlignItems::Center).tagged(Wraps),
    ))
    .gap(4.0);
    pressed_card(body, move |world| {
        let mut hiring = world.resource_mut::<Hiring>();
        hiring.open = Some(id.clone());
        hiring.said = None;
    })
}

/// A name in the display face, with its price at the right.
pub(super) fn heading(name: String, price: Price) -> impl View {
    row((
        row((label(name).size(13.0).bold(true).tagged(DisplayText),))
            .fills_width(),
        label(price.words())
            .size(12.0)
            .bold(true)
            .wrap(false)
            .tone(Tone::Faint),
    ))
    .gap(10.0)
    .align(AlignItems::FlexStart)
}

/// The name at the head of a listing's page, on as many lines as it
/// needs in a narrow panel.
pub(super) fn named(name: String) -> impl View {
    label(name).size(16.0).bold(true).tagged(DisplayText)
}

/// Stars out of five and the reviews they come from.
pub(super) fn rating(standing: Standing, size: f32) -> AnyView {
    row((
        label(format!("★ {:.1}", standing.stars))
            .size(size)
            .bold(true)
            .wrap(false)
            .tone(Tone::Faint),
        label(format!("({})", count(standing.reviews)))
            .size(size)
            .wrap(false)
            .tone(Tone::Dim),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
    .boxed()
}

/// A count, short: `940`, `1.9k`, `48k`.
pub(super) fn count(count: u32) -> String {
    match count {
        0..1000 => count.to_string(),
        1000..10_000 => {
            format!("{:.1}k", f64::from(count) / 1000.0)
        }
        _ => format!("{}k", count / 1000),
    }
}

/// A card that opens what it shows when pressed: a shade above the
/// panel in a dim brass frame, which warms under the pointer.
pub(super) fn stand(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.card)
                .border(1.0)
                .border_color(t.palette.brass_6)
                .radius(t.space.round)
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::Stretch)
                .padding(UiRect::axes(px(14.0), px(10.0)))
        })
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.brass_hover)
                    .border_color(t.palette.brass_4)
            })
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.well).border_color(t.palette.brass_5)
            })
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// `body` as a [`stand`] that runs `act`.
pub(super) fn pressed_card(
    body: impl View + FrameProps,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    button(body.grow(1.0).min_width(px(0.0)))
        .rules(stand)
        .tagged(Mood::Plain)
        .on_activate(act)
        .boxed()
}

/// A word to press among others, the one chosen lit.
pub(super) fn chip(
    text: &'static str,
    on: bool,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    button(
        label(text.to_uppercase())
            .size(11.0)
            .wrap(false)
            .tagged(DisplayText),
    )
    .padding(pad())
    .min_height(px(CONTROL - 4.0))
    .rules(if on { picked } else { brass })
    .tagged(Mood::Plain)
    .on_activate(act)
    .boxed()
}

/// A choice that is taken: a brass button held as under the pointer,
/// leaving the lamp to the one good action.
fn picked(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.brass_hover)
                .border(1.0)
                .border_color(t.palette.brass_3)
                .radius(t.space.round)
        })
        .label(|l, _| l.tone(Tone::Body).bold(true))
        .icon(|i, _| i.tone(Tone::Body))
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.well).border_color(t.palette.brass_5)
            })
            .tone(Tone::Dim)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// The field a stall is searched in, tagged `tag`, with `beside` at
/// its right.
pub(super) fn finder<F: Component>(
    words: impl Into<bevy_fynix::Prop<String>>,
    set: impl Fn(&mut World, String) + Send + Sync + 'static,
    tag: F,
    beside: AnyView,
) -> AnyView {
    row((
        small("FIND"),
        text_field(words, set)
            .grow(1.0)
            .min_width(px(0.0))
            .rules(well)
            .tagged(tag),
        beside,
    ))
    .gap(10.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The choice of the order a stall's listings are shown in, kept in
/// `R`.
pub(super) fn orders<R: Resource>(
    at: fn(&R) -> Order,
    set: fn(&mut World, Order),
) -> AnyView {
    dropdown(
        Order::ALL.iter().map(|order| order.name()),
        resource::<R, _>(move |kept| {
            Order::ALL
                .iter()
                .position(|order| *order == at(kept))
                .unwrap_or(0)
        }),
        Glyph::Chevron.image(ICON),
        move |world, chosen| set(world, Order::ALL[chosen]),
    )
    .min_width(px(128.0))
    .boxed()
}

/// The name of a part of a page.
pub(super) fn part(name: &'static str, body: AnyView) -> AnyView {
    column((
        label(name).size(12.0).bold(true).tone(Tone::Faint),
        body,
    ))
    .gap(6.0)
    .boxed()
}

/// Words that tag a listing, each in a small frame.
pub(super) fn tags(words: &[String]) -> AnyView {
    let words = words.to_vec();
    with_cx(move |cx| {
        let edge = cx.theme().palette.brass_6;
        let all = words
            .iter()
            .map(|word| {
                row((label(word.clone())
                    .size(11.0)
                    .wrap(false)
                    .tone(Tone::Faint),))
                .padding(UiRect::axes(px(8.0), px(3.0)))
                .border(1.0)
                .border_color(edge)
                .radius(10.0)
                .boxed()
            })
            .collect::<Vec<_>>();
        row(all).gap(6.0).tagged(Wraps).boxed()
    })
}

/// What a price is, and who is paid, for the box where a listing is
/// bought.
pub(super) fn terms(price: Price, seller: &Seller) -> String {
    match price {
        Price::Free => String::new(),
        _ => format!(
            "{} receives {}% of each sale.",
            seller.name, SELLERS_SHARE
        ),
    }
}

/// The box a listing is had from: its price large, what is paid and
/// to whom, and `deal`.
pub(super) fn deal_box(
    cx: &Cx,
    price: Price,
    terms: String,
    deal: AnyView,
) -> impl View + use<> {
    let palette = &cx.theme().palette;
    let (amount, per) = match price {
        Price::Free => ("Free".to_string(), ""),
        Price::Once(dollars) => (format!("${dollars:.2}"), "once"),
        Price::Monthly(dollars) => {
            (format!("${dollars:.2}"), "a month")
        }
    };
    column((
        row((
            row((
                label(amount)
                    .size(18.0)
                    .bold(true)
                    .wrap(false)
                    .tagged(DisplayText),
                label(per).size(12.0).tone(Tone::Dim).wrap(false),
            ))
            .gap(6.0)
            .align(AlignItems::FlexEnd)
            .fills_width(),
            deal,
        ))
        .gap(10.0)
        .align(AlignItems::Center),
        match terms.is_empty() {
            true => bare().boxed(),
            false => note(terms).boxed(),
        },
    ))
    .gap(8.0)
    .padding(UiRect::axes(px(14.0), px(12.0)))
    .fill(palette.hearth)
    .border(1.0)
    .border_color(palette.brass_5)
    .radius(cx.theme().space.round)
}

/// What came of the last thing done at a stall.
pub(super) fn said(words: Option<String>) -> AnyView {
    match words {
        Some(words) => {
            label(words).size(12.0).tone(Tone::Accent).boxed()
        }
        None => bare().boxed(),
    }
}

/// The hireling `id` on a page of its own.
fn hireling_page(id: String) -> AnyView {
    let found = {
        let id = id.clone();
        derived(move |world: &World| {
            let hiring = world.resource::<Hiring>();
            hiring.all.iter().find(|h| h.id == id).cloned()
        })
        .when(moved::<Hiring>())
    };
    let body = keyed(found, |hireling| match hireling {
        None => note("This agent is no longer at the stall.").boxed(),
        Some(hireling) => about(hireling),
    })
    .within(bare());
    column((
        page_head(
            "AGENTS",
            |world| world.resource_mut::<Hiring>().open = None,
            Glyph::Robot,
            "AGENT",
        ),
        body,
    ))
    .gap(16.0)
    .boxed()
}

fn about(hireling: &Hireling) -> AnyView {
    let h = hireling.clone();
    let mut parts = vec![
        column((
            named(h.name.clone()),
            row((
                rating(h.standing, 12.0),
                note(format!("by {}", h.maker.name)).size(12.0),
            ))
            .gap(8.0)
            .align(AlignItems::Center)
            .tagged(Wraps),
        ))
        .gap(4.0)
        .boxed(),
        label(h.purpose.clone()).size(13.0).boxed(),
        hire(&h),
        part(
            "How it works",
            label(h.character.clone())
                .size(12.0)
                .tone(Tone::Dim)
                .boxed(),
        ),
        part("Skills", tags(&h.skills)),
    ];
    if let Some(review) = h.reviews.first() {
        parts.push(part(
            "What users say",
            column((
                label(format!("\"{}\"", review.words)).size(12.0),
                note(format!(
                    "{}  {}",
                    stars(review.stars),
                    review.by
                ))
                .size(11.0),
            ))
            .gap(3.0)
            .boxed(),
        ));
    }
    column(parts).gap(14.0).boxed()
}

/// Stars, five in all, `lit` of them filled.
pub(super) fn stars(lit: u8) -> String {
    (0..5).map(|at| if at < lit { '★' } else { '☆' }).collect()
}

/// The box a hireling is hired from.
fn hire(hireling: &Hireling) -> AnyView {
    let (id, name) = (hireling.id.clone(), hireling.name.clone());
    let price = hireling.price;
    let terms = terms(price, &hireling.maker);
    let now = {
        let id = id.clone();
        derived(move |world: &World| {
            let hiring = world.resource::<Hiring>();
            (hiring.hired.contains(&id), hiring.said.clone())
        })
        .when(moved::<Hiring>())
    };
    keyed(now, move |(hired, words)| {
        let deal = if *hired {
            let id = id.clone();
            row((
                stated(Glyph::Check, "HIRED", Tone::Accent, 12.0),
                action(Glyph::Cross, "LET GO", move |world| {
                    let mut hiring = world.resource_mut::<Hiring>();
                    hiring.hired.retain(|hired| *hired != id);
                    hiring.said = None;
                }),
            ))
            .gap(10.0)
            .align(AlignItems::Center)
            .boxed()
        } else {
            let (id, name) = (id.clone(), name.clone());
            good_action(Glyph::Coins, "HIRE", move |world| {
                let mut hiring = world.resource_mut::<Hiring>();
                hiring.hired.push(id.clone());
                hiring.said = Some(format!(
                    "{name} shakes your hand. Hiring is pretend for \
                     now: nobody moves in yet, and nothing was paid."
                ));
                // The merchant in the world cheers the sale.
                world.trigger(Traded);
            })
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
