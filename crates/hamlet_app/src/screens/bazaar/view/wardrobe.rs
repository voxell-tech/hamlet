//! The LOOKS stall drawn as the other stalls are: the official looks
//! agents and hamlets wear, found by what wears them and their kind,
//! each on a page of its own, worn in the viewer under its head, with
//! the way it is had, worn and taken off; and the fitting, what each
//! agent and hamlet wears, a page of its own.

mod fitting;

use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::ui::{AlignItems, percent};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_realm::wardrobe::{Asset, Category, Target};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    Fills, ICON, action, back_link, bare, buttons, filling, lettered,
    note,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;

use super::parts::{
    self, Line, mark as accent, nothing, outcome, page_head, rows,
    section, sections, top, wide,
};
use super::shelf::{self, Shown, choice};
use super::trade;
use super::wares::price;
use crate::bazaar::market::lot::price_words;
use crate::bazaar::market::{Deal, Market, sign_in};
use crate::bazaar::shelf::Listing;
use crate::bazaar::wardrobe::closet::{self, Closet};
use crate::bazaar::wardrobe::viewing::{Sight, Viewing};
use crate::bazaar::wardrobe::{
    self, AssetDeal, Dressed, Fitting, Wardrobe, Who, file_of,
};
use crate::roll::{either, moved};
use crate::viewer;

impl Shown for Asset {
    const BACK: &'static str = "LOOKS";
    const ABOUT: &'static str = "What agents and hamlets wear.";
    const NOUN: &'static str = "looks";
    const FIND: &'static str =
        "Search by a look's name, or what it is.";
    const FROM: Option<&'static str> = Some("the bazaar");

    fn notice() -> AnyView {
        buttons(vec![action(Glyph::Robot, "FITTING", |world| {
            world.resource_mut::<Fitting>().on = true;
            world.resource_mut::<Wardrobe>().said = None;
        })])
        .boxed()
    }

    fn controls() -> Vec<AnyView> {
        vec![
            narrow(
                "For both",
                &Target::ALL,
                |target| match target {
                    Target::Agent => "For agents",
                    Target::Hamlet => "For hamlets",
                },
                (|filter| filter.0, |filter, to| filter.0 = to),
            ),
            narrow(
                "All kinds",
                &Category::ALL,
                Category::name,
                (|filter| filter.1, |filter, to| filter.1 = to),
            ),
        ]
    }

    fn again(world: &mut World) {
        wardrobe::ask_catalogue(world);
    }

    fn foot() -> AnyView {
        trade::account("to buy, and to wear what you bought.")
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn row(&self) -> Line {
        Line {
            name: self.title.clone(),
            price: price(self.price_cents),
            purpose: self.summary.clone(),
            meta: kind(self),
            mark: Some(mark(self.id.clone())),
        }
    }

    fn page(&self) -> AnyView {
        let head = page_head(
            &self.title,
            note(self.summary.clone()).boxed(),
            format!("{}, version {}", kind(self), self.version),
            Some(mark(self.id.clone())),
        );
        sections(vec![
            top(vec![head, viewed(), deal(self.clone())]),
            worn_by(self.clone()),
        ])
    }
}

/// The LOOKS stall: its shelf, or the fitting; or the page the chief
/// signs in from while they do.
pub(super) fn stall() -> AnyView {
    trade::signing(Asset::BACK, || {
        keyed(resource::<Fitting, _>(|fitting| fitting.on), |on| {
            match on {
                true => fitting::fitting(),
                false => shelf::stall::<Asset>(),
            }
        })
        .within(filling())
        .boxed()
    })
}

/// What an asset is and what wears it, in a few words.
pub(super) fn kind(asset: &Asset) -> String {
    let by = match asset.target {
        Target::Agent => "agents",
        Target::Hamlet => "hamlets",
    };
    format!("{} for {by}", asset.category.one())
}

/// What part of the shelf's filter a dropdown reads and sets.
type Part<T> = (
    fn(&<Asset as Listing>::Filter) -> Option<T>,
    fn(&mut <Asset as Listing>::Filter, Option<T>),
);

/// The way to narrow the shelf to one of `all`, or to show `any` of
/// them, choosing the part `part` of its filter.
fn narrow<T: Copy + PartialEq + Send + Sync + 'static>(
    any: &str,
    all: &'static [T],
    named: fn(T) -> &'static str,
    (get, set): Part<T>,
) -> AnyView {
    let mut options = vec![any.to_string()];
    options.extend(all.iter().map(|one| named(*one).to_string()));
    choice::<Asset>(
        options,
        move |shelf| {
            get(&shelf.filter)
                .and_then(|now| {
                    all.iter().position(|one| *one == now)
                })
                .map_or(0, |at| at + 1)
        },
        move |shelf, chosen| {
            set(
                &mut shelf.filter,
                chosen.checked_sub(1).map(|at| all[at]),
            );
        },
    )
}

/// The open asset worn in the viewer, as wide as a listing's cover on
/// its page, or how its file stands.
fn viewed() -> AnyView {
    keyed(resource::<Viewing, _>(|viewing| viewing.sight), |sight| {
        match sight {
            Sight::Fetching => note("Fetching it...").boxed(),
            Sight::Shown => viewer::view(Vec2::new(320.0, 240.0)),
            Sight::Failed => note(
                "It could not be fetched. Open it again to try once \
                 more.",
            )
            .boxed(),
        }
    })
    .within(bare())
    .boxed()
}

/// The accent line of the asset `id`, once it is the chief's: Worn
/// once anyone wears it.
fn mark(id: String) -> AnyView {
    let mark = derived(move |world: &World| {
        let wardrobe = world.resource::<Wardrobe>();
        let worn =
            !world.resource::<Dressed>().wearers(&id).is_empty();
        match wardrobe.owns(&id) {
            true if worn => Some("Worn"),
            true => Some("You own this"),
            false => None,
        }
    })
    .when(either(moved::<Wardrobe>(), moved::<Dressed>()));
    keyed(mark, |mark| match mark {
        Some(mark) => accent(mark),
        None => nothing(),
    })
    .within(bare())
    .boxed()
}

/// All the box of an asset is drawn from: where it stands with the
/// chief, its deal if one is going or just went, and who wears it.
#[derive(Clone, PartialEq, Debug)]
struct Now {
    owned: bool,
    signed_in: bool,
    deal: Option<Deal>,
    /// Who may wear it, with their words.
    may: Vec<(Who, String)>,
    chosen: usize,
    /// Who wears it, with their words, and as what.
    on: Vec<(Who, String, Category)>,
    /// How its file stands, in words, once it is worn.
    file: Option<String>,
    said: Option<(String, bool)>,
}

impl Now {
    fn of(world: &World, asset: &Asset) -> Now {
        let wardrobe = world.resource::<Wardrobe>();
        let dressed = world.resource::<Dressed>();
        let may = dressed
            .of(asset.target)
            .map(|one| (one.who.clone(), one.words.clone()))
            .collect::<Vec<_>>();
        let on = dressed
            .wearers(&asset.id)
            .into_iter()
            .map(|(one, category)| {
                (one.who.clone(), one.words.clone(), category)
            })
            .collect::<Vec<_>>();
        let file = (wardrobe.owns(&asset.id) && !on.is_empty())
            .then(|| file_words(world, &file_of(asset)).to_string());
        Now {
            owned: wardrobe.owns(&asset.id),
            signed_in: world.resource::<Market>().signed_in(),
            deal: world
                .resource::<AssetDeal>()
                .0
                .as_ref()
                .filter(|(dealt, _)| *dealt == asset.id)
                .map(|(_, deal)| deal.clone()),
            chosen: world
                .resource::<Fitting>()
                .wearer
                .min(may.len().saturating_sub(1)),
            may,
            on,
            file,
            said: wardrobe.said.clone(),
        }
    }
}

/// A signal of the parts of an asset's page that change.
fn now_of(asset: &Asset) -> bevy_fynix::Signal<Now> {
    let read = asset.clone();
    derived(move |world: &World| Now::of(world, &read)).when(either(
        either(
            either(moved::<Wardrobe>(), moved::<AssetDeal>()),
            either(moved::<Market>(), moved::<Dressed>()),
        ),
        either(moved::<Fitting>(), moved::<Closet>()),
    ))
}

/// The way `asset` is had, as the bazaar's other pages have theirs:
/// its price and the button that buys it while it is not the
/// chief's, then who it goes on and WEAR IT; what that does, and what
/// came of it.
fn deal(asset: Asset) -> AnyView {
    keyed(now_of(&asset), move |now| {
        let mut buttons = Vec::new();
        let mut notes = Vec::new();
        let mut first = Vec::new();
        match now.owned {
            true => first.extend(wear(
                &asset,
                now,
                &mut buttons,
                &mut notes,
            )),
            false => get(&asset, now, &mut buttons, &mut notes),
        }
        trade::told(
            now.deal.as_ref(),
            "It is yours as soon as Stripe says it is paid.",
            &mut buttons,
            &mut notes,
        );
        if let Some((words, wrong)) = &now.said {
            notes.push(outcome(words.clone(), *wrong));
        }
        let price = (!now.owned).then(|| price(asset.price_cents));
        first.push(parts::deal(None, buttons, notes));
        parts::deal(price, Vec::new(), first)
    })
    .within(bare())
    .boxed()
}

/// The way `asset` is had, or how its deal goes, into `buttons` and
/// `notes`.
fn get(
    asset: &Asset,
    now: &Now,
    buttons: &mut Vec<AnyView>,
    notes: &mut Vec<AnyView>,
) {
    if now.deal.as_ref().is_some_and(Deal::going) {
        notes.extend(trade::going(now.deal.as_ref()));
        return;
    }
    if !now.signed_in {
        buttons.push(action(Glyph::Signpost, "SIGN IN", sign_in));
        notes.push(note("Sign in with GitHub to get it.").boxed());
        return;
    }
    let id = asset.id.clone();
    let (which, words) = match asset.price_cents {
        0 => (Glyph::Plus, "GET IT FOR FREE".to_string()),
        cents => {
            (Glyph::Coins, format!("BUY FOR {}", price_words(cents)))
        }
    };
    buttons.push(action(which, words, move |world| {
        wardrobe::buy(world, &id);
    }));
}

/// The way the owned `asset` is worn, into `buttons` and `notes`, and
/// the choice of who it goes on, as the Inspector has the choice of
/// an agent's model, if there is anyone.
fn wear(
    asset: &Asset,
    now: &Now,
    buttons: &mut Vec<AnyView>,
    notes: &mut Vec<AnyView>,
) -> Option<AnyView> {
    let (id, category) = (asset.id.clone(), asset.category);
    if now.may.is_empty() {
        notes.push(
            note(match asset.target {
                Target::Agent => "There is no agent to wear it yet.",
                Target::Hamlet => {
                    "There is no hamlet to wear it yet."
                }
            })
            .boxed(),
        );
        return None;
    }
    let who = now
        .may
        .iter()
        .map(|(who, _)| who.clone())
        .collect::<Vec<_>>();
    let words = now
        .may
        .iter()
        .map(|(_, words)| words.clone())
        .collect::<Vec<_>>();
    buttons.push(action(Glyph::Check, "WEAR IT", move |world| {
        let at = world.resource::<Fitting>().wearer;
        if let Some(who) = who.get(at.min(who.len() - 1)) {
            wardrobe::wear(world, who, category, Some(id.clone()));
        }
    }));
    Some(
        column((
            note("Goes on"),
            wide(
                dropdown(
                    words,
                    now.chosen,
                    Glyph::Chevron.image(ICON),
                    |world, at| {
                        world.resource_mut::<Fitting>().wearer = at;
                    },
                )
                .width(percent(100.0))
                .max_width(percent(100.0)),
            ),
        ))
        .gap(6.0)
        .boxed(),
    )
}

/// Who wears `asset`, each with TAKE OFF, as Settings lists what was
/// given for good, and how its file stands; nothing while no one
/// does.
fn worn_by(asset: Asset) -> AnyView {
    keyed(now_of(&asset), move |now| {
        if now.on.is_empty() {
            return nothing();
        }
        let wearers = now
            .on
            .iter()
            .map(|(who, words, category)| {
                let (who, category) = (who.clone(), *category);
                row((
                    column((bevy_fynix::views::label(words.clone())
                        .size(13.0),))
                    .fills_width(),
                    lettered("TAKE OFF")
                        .rules(back_link)
                        .tagged(Variant::Text)
                        .on_activate(move |world| {
                            wardrobe::wear(
                                world, &who, category, None,
                            );
                        }),
                ))
                .gap(12.0)
                .align(AlignItems::Center)
                .boxed()
            })
            .collect();
        let mut parts = vec![rows(wearers)];
        parts.extend(now.file.clone().map(|file| note(file).boxed()));
        section("WORN BY", column(parts).gap(6.0).boxed())
    })
    .within(bare())
    .boxed()
}

/// How the file `file` worn stands, in words.
pub(super) fn file_words(world: &World, file: &str) -> &'static str {
    let closet = world.resource::<Closet>();
    if closet::has(world, file) {
        "In the realm."
    } else if closet
        .download
        .as_ref()
        .is_some_and(|download| download.file == file)
    {
        "Downloading it for the realm..."
    } else if closet.missed(file) {
        "It could not be downloaded. It is asked for again shortly."
    } else if !world.resource::<Market>().signed_in() {
        "Sign in with GitHub to download it for the realm."
    } else {
        "Waiting to be downloaded for the realm..."
    }
}
