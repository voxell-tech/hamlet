//! The Asset stall: the official assets agents and hamlets wear,
//! found by what wears them and their kind, each on a page of its own
//! that turns its pictures on a turntable and is bought from there.
//! What is bought is worn from an agent's or a hamlet's page.

mod fitting;
mod turntable;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Res};
use bevy::ecs::world::World;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius, Node,
    UiRect, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, derived, keyed, resource};
use hamlet_core::market::{Deal, Market};
use hamlet_core::wardrobe::{AskWardrobe, BuyAsset, Wardrobe};
use hamlet_model::market::Price;
use hamlet_model::wardrobe::{Asset, Category, Target};
use hamlet_world::bazaar::Traded;

pub(crate) use self::fitting::looks;
use super::artisans::{chip, deal_box, named, pressed_card};
use super::trade;
use crate::icons::Glyph;
use crate::looks::{
    Fills, action, bare, buttons, good_action, note, page_head,
    stated,
};
use crate::pictures::{self, Pictures, Source, State};
use crate::platform::{DisplayText, Wraps};
use crate::roll::{either, moved};
use crate::{AnyView, Cx, with_cx};

/// The size of the still of an asset in the list.
const STILL: f32 = 72.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Fitting>()
        .add_plugins(turntable::plugin)
        .add_systems(Update, cheer);
}

/// What the chief is looking at in the Asset stall.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub(crate) struct Fitting {
    /// Only assets worn by this, when some.
    target: Option<Target>,
    category: Option<Category>,
    /// The asset open on a page of its own, by its id.
    open: Option<String>,
}

/// The merchant in the world cheers each asset bought.
fn cheer(
    wardrobe: Option<Res<Wardrobe>>,
    mut seen: Local<Option<String>>,
    mut commands: Commands,
) {
    let Some(wardrobe) = wardrobe.filter(|w| w.is_changed()) else {
        return;
    };
    if let Some((id, Deal::Done(_))) = &wardrobe.deal
        && seen.as_ref() != Some(id)
    {
        *seen = Some(id.clone());
        commands.trigger(Traded);
    }
}

/// The assets of `catalog` the chips let through.
fn shown(
    catalog: &[Asset],
    target: Option<Target>,
    category: Option<Category>,
) -> impl Iterator<Item = &Asset> {
    catalog.iter().filter(move |asset| {
        target.is_none_or(|t| asset.target == t)
            && category.is_none_or(|c| asset.category == c)
    })
}

/// The address of `path` on the bazaar's server.
fn address(world: &World, path: &str) -> String {
    let server = world
        .get_resource::<Market>()
        .map(|market| market.server.clone())
        .unwrap_or_default();
    format!("{}{path}", server.trim_end_matches('/'))
}

/// The picture at `path` on the bazaar's server, asked for if it
/// never was.
fn ask_picture(world: &mut World, path: &str) -> Source {
    let address = address(world, path);
    let source = Source::Web(address.clone());
    let state =
        world.get_resource_or_init::<Pictures>().state(&source);
    if state == State::Unasked {
        pictures::fetch(world, &address);
    }
    source
}

/// The stall: who is signed in over its shelves or one asset's page.
pub(super) fn stall() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        // Drawn without the core's plugins in the gallery.
        cx.world.init_resource::<Fitting>();
        cx.world.init_resource::<Market>();
        cx.world.init_resource::<Wardrobe>();
        let page = keyed(
            resource::<Fitting, _>(|fitting| fitting.open.clone()),
            |open| match open {
                Some(id) => asset_page(id.clone()),
                None => shelves(),
            },
        )
        .within(bare());
        cx.build(column((trade::account(None), page)).gap(14.0))
    })
}

/// An asset's card, as it is drawn.
#[derive(Clone, PartialEq)]
struct Card {
    id: String,
    title: String,
    summary: String,
    kind: String,
    price: Price,
    owned: bool,
    /// The path of its first picture.
    still: Option<String>,
}

fn cards(world: &World) -> Vec<Card> {
    let fitting = world.resource::<Fitting>();
    let wardrobe = world.resource::<Wardrobe>();
    shown(&wardrobe.catalog, fitting.target, fitting.category)
        .map(|asset| Card {
            id: asset.id.clone(),
            title: asset.title.clone(),
            summary: asset.summary.clone(),
            kind: kind(asset),
            price: asset.price(),
            owned: wardrobe.owns(&asset.id) || asset.owned,
            still: (asset.frames > 0).then(|| asset.frame(0)),
        })
        .collect()
}

/// What an asset is and what wears it, in a few words.
fn kind(asset: &Asset) -> String {
    let by = match asset.target {
        Target::Agent => "agents",
        Target::Hamlet => "hamlets",
    };
    format!("{} for {by}", asset.category.one())
}

/// A price as a card shows it.
fn price_words(price: Price) -> String {
    match price {
        Price::Free => "FREE".into(),
        _ => price.words(),
    }
}

/// A row of chips, one lit, that sets what `set` sets.
fn chips<T: Copy + PartialEq + Send + Sync + 'static>(
    all: &[T],
    chosen: Option<T>,
    name: fn(T) -> &'static str,
    set: fn(&mut Fitting, Option<T>),
) -> AnyView {
    let mut row = vec![chip("All", chosen.is_none(), move |world| {
        set(&mut world.resource_mut::<Fitting>(), None);
    })];
    row.extend(all.iter().map(|&one| {
        chip(name(one), chosen == Some(one), move |world| {
            set(&mut world.resource_mut::<Fitting>(), Some(one));
        })
    }));
    buttons(row).boxed()
}

fn shelves() -> AnyView {
    let filters = keyed(
        resource::<Fitting, _>(|f| (f.target, f.category)),
        |(target, category)| {
            column((
                chips(&Target::ALL, *target, Target::name, |f, t| {
                    f.target = t
                }),
                chips(
                    &Category::ALL,
                    *category,
                    Category::name,
                    |f, c| f.category = c,
                ),
            ))
            .gap(8.0)
            .boxed()
        },
    )
    .within(bare());
    let list = keyed(
        derived(cards)
            .when(either(moved::<Fitting>(), moved::<Wardrobe>())),
        |cards| {
            column(cards.iter().map(card).collect::<Vec<_>>())
                .gap(8.0)
                .boxed()
        },
    )
    .within(bare());
    column((filters, standing(), list)).gap(12.0).boxed()
}

/// How the shelves stand, when that is worth saying: asked for, out
/// of reach, bare, or bare of what the chips let through.
fn standing() -> AnyView {
    let now = derived(|world: &World| {
        let wardrobe = world.resource::<Wardrobe>();
        let fitting = world.resource::<Fitting>();
        let server = world.resource::<Market>().server.clone();
        (
            wardrobe.asking,
            wardrobe.problem.clone(),
            wardrobe.catalog.len(),
            shown(
                &wardrobe.catalog,
                fitting.target,
                fitting.category,
            )
            .count(),
            server,
        )
    })
    .when(either(
        moved::<Wardrobe>(),
        either(moved::<Fitting>(), moved::<Market>()),
    ));
    keyed(now, |(asking, problem, all, shown, server)| {
        if let Some(why) = problem {
            return column((
                label(format!(
                    "The bazaar at {server} could not be reached: \
                     {why}"
                ))
                .size(12.0)
                .tone(Tone::Critical),
                buttons(vec![action(
                    Glyph::Restore,
                    "TRY AGAIN",
                    |world| world.trigger(AskWardrobe),
                )]),
            ))
            .gap(6.0)
            .boxed();
        }
        match (*all, *shown, *asking) {
            (0, _, true) => note("Asking the bazaar...").boxed(),
            (0, _, false) => {
                note("Nothing is on these shelves yet.").boxed()
            }
            (_, 0, _) => note("Nothing of that kind yet.").boxed(),
            _ => bare().boxed(),
        }
    })
    .within(bare())
    .boxed()
}

fn card(card: &Card) -> AnyView {
    let id = card.id.clone();
    let mut foot = vec![note(card.kind.clone()).size(11.0).boxed()];
    if card.owned {
        foot.push(
            row((stated(Glyph::Check, "OWNED", Tone::Accent, 11.0),))
                .margin(UiRect::left(bevy::ui::Val::Auto))
                .boxed(),
        );
    }
    let body = row((
        still(card.still.clone()),
        column((
            row((
                row((label(card.title.clone())
                    .size(13.0)
                    .bold(true)
                    .tagged(DisplayText),))
                .fills_width(),
                label(price_words(card.price))
                    .size(12.0)
                    .bold(true)
                    .wrap(false)
                    .tone(Tone::Faint),
            ))
            .gap(10.0)
            .align(AlignItems::FlexStart),
            label(card.summary.clone()).size(12.0).tone(Tone::Dim),
            row(foot)
                .gap(8.0)
                .align(AlignItems::Center)
                .tagged(Wraps),
        ))
        .gap(4.0)
        .grow(1.0)
        .min_width(px(0.0)),
    ))
    .gap(12.0)
    .align(AlignItems::Center);
    pressed_card(body, move |world| {
        world.resource_mut::<Fitting>().open = Some(id.clone());
    })
}

/// The first picture of an asset, small, in a frame; the frame alone
/// while it comes or when it cannot.
fn still(path: Option<String>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (ground, edge, round) = (
            theme.palette.well,
            theme.palette.brass_6,
            theme.space.round,
        );
        let within = cx.build(bare().shrink(0.0));
        let source =
            path.as_deref().map(|p| ask_picture(cx.world, p));
        let image = source.as_ref().and_then(|source| {
            match cx.world.resource::<Pictures>().state(source) {
                State::Ready(drawn) => Some(drawn.image),
                _ => None,
            }
        });
        let mut framed = cx.world.spawn((
            Node {
                width: px(STILL),
                height: px(STILL),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(round)),
                ..Node::default()
            },
            BackgroundColor(ground),
            BorderColor::all(edge),
            bevy::ecs::hierarchy::ChildOf(within),
        ));
        if let Some(image) = image {
            framed.insert(
                ImageNode::new(image)
                    .with_mode(NodeImageMode::Stretch),
            );
        }
        if let Some(source) = source {
            framed.insert((turntable::Still, source));
        }
        within
    })
}

/// The asset `id` on a page of its own.
fn asset_page(id: String) -> AnyView {
    // Whether it is owned is left to the deal, so that buying it
    // does not draw the page and its turntable again.
    let found = derived(move |world: &World| {
        world.resource::<Wardrobe>().asset(&id).map(|asset| Asset {
            owned: false,
            ..asset.clone()
        })
    })
    .when(moved::<Wardrobe>());
    let body = keyed(found, |asset| match asset {
        None => note("This asset is no longer at the stall.").boxed(),
        Some(asset) => about(asset),
    })
    .within(bare());
    column((
        page_head(
            "ASSETS",
            |world| world.resource_mut::<Fitting>().open = None,
            Glyph::Hanger,
            "ASSET",
        ),
        body,
    ))
    .gap(16.0)
    .boxed()
}

fn about(asset: &Asset) -> AnyView {
    let wear = match asset.target {
        Target::Agent => {
            "Wear it from an agent's page in the Inspector."
        }
        Target::Hamlet => {
            "Wear it from a hamlet's page in the Inspector."
        }
    };
    column((
        column((
            named(asset.title.clone()),
            note(format!(
                "{}, version {}",
                kind(asset),
                asset.version
            ))
            .size(12.0),
        ))
        .gap(4.0),
        turntable::turntable(asset),
        label(asset.summary.clone()).size(13.0),
        deal(asset.id.clone(), asset.price()),
        note(wear),
    ))
    .gap(14.0)
    .boxed()
}

/// Where an asset stands with the chief, and the deal for it if one
/// is going or just went.
#[derive(Clone, PartialEq, Debug)]
struct Stand {
    owned: bool,
    deal: Option<Deal>,
}

/// The box the asset `id` is bought from.
fn deal(id: String, price: Price) -> AnyView {
    let read = id.clone();
    let state = derived(move |world: &World| {
        let wardrobe = world.resource::<Wardrobe>();
        Stand {
            owned: wardrobe.owns(&read),
            deal: match &wardrobe.deal {
                Some((dealt, deal)) if *dealt == read => {
                    Some(deal.clone())
                }
                _ => None,
            },
        }
    })
    .when(moved::<Wardrobe>());
    keyed(state, move |stand| {
        let act = deal_part(&id, stand, price);
        let mut under = Vec::new();
        match &stand.deal {
            Some(Deal::Done(words)) => under.push(
                label(words.clone())
                    .size(12.0)
                    .tone(Tone::Accent)
                    .boxed(),
            ),
            Some(Deal::Failed(why)) => under.push(
                label(why.clone())
                    .size(12.0)
                    .tone(Tone::Critical)
                    .boxed(),
            ),
            Some(Deal::Paying(url)) => {
                let url = url.clone();
                under.push(
                    column((
                        note(
                            "Finish paying on the page that opened \
                             in your browser. It is yours as soon \
                             as Stripe says it is paid.",
                        ),
                        buttons(vec![action(
                            Glyph::Coins,
                            "OPEN THE PAGE",
                            move |_| {
                                hamlet_desktop::open::browse(&url);
                            },
                        )]),
                    ))
                    .gap(6.0)
                    .boxed(),
                );
            }
            _ => {}
        }
        with_cx(move |cx| {
            let mut parts =
                vec![deal_box(cx, price, String::new(), act).boxed()];
            parts.extend(under);
            column(parts).gap(8.0).boxed()
        })
    })
    .within(bare())
    .boxed()
}

fn deal_part(id: &str, stand: &Stand, price: Price) -> AnyView {
    let waiting = match &stand.deal {
        Some(Deal::Buying) => {
            Some((Glyph::Coins, "Asking the bazaar..."))
        }
        Some(Deal::Paying(_)) => {
            Some((Glyph::Coins, "Waiting for Stripe..."))
        }
        Some(Deal::Fetching) => {
            Some((Glyph::Clock, "Setting it up..."))
        }
        _ => None,
    };
    if let Some((which, words)) = waiting {
        return stated(which, words, Tone::Accent, 12.0).boxed();
    }
    if stand.owned {
        return stated(Glyph::Check, "OWNED", Tone::Accent, 12.0)
            .boxed();
    }
    let id = id.to_string();
    let buy =
        move |world: &mut World| world.trigger(BuyAsset(id.clone()));
    match price {
        Price::Free => good_action(Glyph::Plus, "GET", buy),
        _ => good_action(Glyph::Coins, "BUY", buy),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, target: Target, category: Category) -> Asset {
        Asset {
            id: id.into(),
            target,
            category,
            title: id.into(),
            summary: String::new(),
            price_cents: 0,
            version: "1.0.0".into(),
            frames: 0,
            preview_url: String::new(),
            owned: false,
        }
    }

    #[test]
    fn the_chips_let_through_what_wears_it_and_its_kind() {
        let catalog = [
            asset("a", Target::Agent, Category::Skin),
            asset("b", Target::Hamlet, Category::Skin),
            asset("c", Target::Agent, Category::Emote),
        ];
        let ids = |target, category| {
            shown(&catalog, target, category)
                .map(|asset| asset.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(None, None), ["a", "b", "c"]);
        assert_eq!(ids(Some(Target::Agent), None), ["a", "c"]);
        assert_eq!(ids(None, Some(Category::Skin)), ["a", "b"]);
        assert_eq!(
            ids(Some(Target::Hamlet), Some(Category::Emote)),
            Vec::<&str>::new()
        );
    }
}
