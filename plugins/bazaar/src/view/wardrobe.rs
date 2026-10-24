//! The LOOKS stall drawn as the other stalls are: the official looks
//! agents and hamlets wear, found by what wears them and their kind,
//! each on a page of its own with its pictures on a turntable under
//! its head and the way it is had, worn and taken off; and the
//! fitting, what each agent and hamlet wears, a page of its own.

mod fitting;
mod turntable;

use agents_ui::{Rostered, either, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, FrameProps, Space, UiExt, ViewExt, button,
    column, derived, description, dropdown, icon_button, keyed,
    label, resource, row,
};
use looks::wardrobe::{Asset, Category, Target};

use crate::market::lot::price_words;
use crate::market::{Deal, Market, sign_in};
use crate::shelf::Listing;
use crate::view::parts::{
    self, Line, mark as accent, note, outcome, page_head, rows,
    section, sections, top,
};
use crate::view::shelf::{self, Shown, choice};
use crate::view::trade;
use crate::view::wares::price;
use crate::wardrobe::closet::Closet;
use crate::wardrobe::{self, AssetDeal, Fitting, Wardrobe};

impl Shown for Asset {
    const BACK: &'static str = "LOOKS";
    const ABOUT: &'static str = "What agents and hamlets wear.";
    const NOUN: &'static str = "looks";
    const FIND: &'static str = "Search looks";
    const FROM: Option<&'static str> = Some("the bazaar");

    fn notice() -> AnyView {
        row((icon_button("robot", "FITTING").on_click(|world| {
            world.resource_mut::<Fitting>().on = true
        }),))
        .boxed()
    }

    fn controls() -> Vec<AnyView> {
        vec![
            // Not Agents and Hamlets alone, the names of two stalls.
            narrow(
                "For agents and hamlets",
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
            description(self.summary.clone()).boxed(),
            format!("{}, version {}", kind(self), self.version),
            Some(mark(self.id.clone())),
        );
        sections(vec![
            top(vec![
                head,
                turntable::turntable(self.clone()),
                deal(self.clone()),
            ]),
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
            )
        },
    )
}

/// The accent line of the asset `id`, once it is the chief's: Worn
/// once anyone wears it.
fn mark(id: String) -> AnyView {
    let mark = derived(move |world: &World| {
        let wardrobe = world.resource::<Wardrobe>();
        match wardrobe.owns(&id) {
            true if !wardrobe.wearing.wearers(&id).is_empty() => {
                Some("Worn")
            }
            true => Some("You own this"),
            false => None,
        }
    })
    .when(moved::<Wardrobe>());
    keyed(mark, |mark| match mark {
        Some(mark) => accent(mark),
        None => column(()).boxed(),
    })
    .boxed()
}

/// All the box of an asset is drawn from: where it stands with the
/// chief, its deal if one is going or just went, and who wears it.
#[derive(Clone, PartialEq, Debug)]
struct Now {
    owned: bool,
    signed_in: bool,
    deal: Option<Deal>,
    /// Who may wear it, by name, with their words.
    may: Vec<(String, String)>,
    chosen: usize,
    /// Who wears it, by name, with their words.
    on: Vec<(String, String)>,
    /// How its file stands, in words, once it is worn.
    file: Option<String>,
    said: Option<(String, bool)>,
}

impl Now {
    fn of(world: &World, asset: &Asset) -> Now {
        let wardrobe = world.resource::<Wardrobe>();
        let may = wearers(world.resource::<Rostered>(), asset.target);
        let shown = |name: &str| {
            may.iter().find(|(one, _)| one == name).map_or_else(
                || name.to_string(),
                |(_, words)| words.clone(),
            )
        };
        let on = wardrobe
            .wearing
            .wearers(&asset.id)
            .into_iter()
            .map(|(_, name, _)| {
                let words = shown(&name);
                (name, words)
            })
            .collect::<Vec<_>>();
        let file = wardrobe
            .file(&asset.id)
            .filter(|_| !on.is_empty())
            .map(|file| file_words(world, &file).to_string());
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

/// What the parts of an asset's page that change are drawn from.
fn now_of(asset: &Asset) -> impl Fn(&World) -> Now + use<> {
    let read = asset.clone();
    move |world: &World| Now::of(world, &read)
}

/// The way `asset` is had, as the bazaar's other pages have theirs:
/// its price and the button that buys it while it is not the
/// chief's, then who it goes on and WEAR IT; what that does, and what
/// came of it.
fn deal(asset: Asset) -> AnyView {
    let now = derived(now_of(&asset)).when(either(
        either(
            either(moved::<Wardrobe>(), moved::<AssetDeal>()),
            either(moved::<Market>(), moved::<Rostered>()),
        ),
        either(moved::<Fitting>(), moved::<Closet>()),
    ));
    keyed(now, move |now| {
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
        buttons.push(
            icon_button("signpost", "SIGN IN")
                .on_click(sign_in)
                .boxed(),
        );
        notes.push(
            description("Sign in with GitHub to get it.").boxed(),
        );
        return;
    }
    let id = asset.id.clone();
    let (which, words) = match asset.price_cents {
        0 => ("plus", "GET IT FOR FREE".to_string()),
        cents => ("coins", format!("BUY FOR {}", price_words(cents))),
    };
    buttons.push(
        icon_button(which, words)
            .on_click(move |world| wardrobe::buy(world, &id))
            .boxed(),
    );
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
    let (id, target, category) =
        (asset.id.clone(), asset.target, asset.category);
    if now.may.is_empty() {
        notes.push(
            description(match target {
                Target::Agent => "There is no agent to wear it yet.",
                Target::Hamlet => {
                    "There is no hamlet to wear it yet."
                }
            })
            .boxed(),
        );
        return None;
    }
    let names = now
        .may
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let words = now
        .may
        .iter()
        .map(|(_, words)| words.clone())
        .collect::<Vec<_>>();
    buttons.push(
        icon_button("check", "WEAR IT")
            .on_click(move |world| {
                let at = world.resource::<Fitting>().wearer;
                if let Some(name) = names.get(at.min(names.len() - 1))
                {
                    wardrobe::wear(
                        world,
                        (target, name),
                        category,
                        Some(id.clone()),
                    );
                }
            })
            .boxed(),
    );
    Some(
        column((
            note("Goes on"),
            dropdown(words, now.chosen as u32).on_pick(
                |world, at| {
                    world.resource_mut::<Fitting>().wearer =
                        at as usize
                },
            ),
        ))
        .space(Space::Half)
        .boxed(),
    )
}

/// Who wears `asset`, each with TAKE OFF, as Settings lists what was
/// given for good, and how its file stands; nothing while no one
/// does.
fn worn_by(asset: Asset) -> AnyView {
    let now = derived(now_of(&asset))
        .when(either(moved::<Wardrobe>(), moved::<Closet>()));
    let (target, category) = (asset.target, asset.category);
    keyed(now, move |now| {
        if now.on.is_empty() {
            return column(()).boxed();
        }
        let wearers = now
            .on
            .iter()
            .map(|(name, words)| {
                let name = name.clone();
                row((
                    column((label(words.clone()),)).grow(1.0),
                    button("TAKE OFF")
                        .style(ButtonStyle::Quiet)
                        .on_click(move |world| {
                            wardrobe::wear(
                                world,
                                (target, &name),
                                category,
                                None,
                            )
                        }),
                ))
                .space(Space::Gap)
                .boxed()
            })
            .collect();
        let mut parts = vec![rows(wearers)];
        parts.extend(
            now.file.clone().map(|file| description(file).boxed()),
        );
        section("WORN BY", column(parts).space(Space::Half).boxed())
    })
    .boxed()
}

/// Who may wear an asset of `target`: each agent by its name, each
/// hamlet by its name on disk, with the words they are shown by.
pub(super) fn wearers(
    roster: &Rostered,
    target: Target,
) -> Vec<(String, String)> {
    let roster = &roster.0;
    match target {
        Target::Agent => roster
            .agents
            .iter()
            .filter(|agent| !agent.gone && !agent.name.is_empty())
            .map(|agent| (agent.name.clone(), agent.name.clone()))
            .collect(),
        Target::Hamlet => roster
            .hamlets
            .iter()
            .map(|hamlet| (hamlet.id.clone(), hamlet.title.clone()))
            .collect(),
    }
}

/// How the file `file` worn stands, in words.
pub(super) fn file_words(world: &World, file: &str) -> &'static str {
    let closet = world.resource::<Closet>();
    if closet.has(file) {
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
