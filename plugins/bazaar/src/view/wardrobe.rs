//! The Asset stall drawn: the official looks agents and hamlets wear,
//! found by what wears them and their kind, each on a page of its own
//! that turns its pictures on a turntable, where it is bought, worn
//! and taken off; and the fitting, what each agent and hamlet wears.

mod fitting;
mod turntable;

use agents_ui::{Rostered, either, moved, note};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, Fill, FrameProps, Size,
    Space, Tone, UiExt, ViewExt, button, column, derived,
    description, dropdown, icon, icon_button, keyed, label, resource,
    row,
};
use looks::wardrobe::{Asset, Category, Target};

use crate::market::lot::price_words;
use crate::market::{Deal, Market};
use crate::shelf::{Listing, Shelf};
use crate::view::parts::{Line, page_head, section, top};
use crate::view::shelf::{self, Shown, browse};
use crate::view::trade;
use crate::view::wares::price;
use crate::wardrobe::closet::Closet;
use crate::wardrobe::{self, AssetDeal, Fitting, Wardrobe};

impl Shown for Asset {
    const BACK: &'static str = "ALL LOOKS";
    const ABOUT: &'static str = "What agents and hamlets wear.";
    const NOUN: &'static str = "looks";
    const FIND: &'static str = "Search looks";
    const FROM: Option<&'static str> = Some("the bazaar");

    fn controls() -> Vec<AnyView> {
        vec![
            // Not AGENTS and HAMLETS alone, the names of two stalls.
            chips(
                &Target::ALL,
                |target| match target {
                    Target::Agent => "For agents",
                    Target::Hamlet => "For hamlets",
                },
                (|filter| filter.0, |filter, to| filter.0 = to),
            ),
            chips(
                &Category::ALL,
                Category::name,
                (|filter| filter.1, |filter, to| filter.1 = to),
            ),
        ]
    }

    fn again(world: &mut World) {
        wardrobe::ask_catalogue(world);
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
        top(vec![
            page_head(
                &self.title,
                description(self.summary.clone()).boxed(),
                format!("{}, version {}", kind(self), self.version),
                None,
            ),
            turntable::turntable(self.clone()),
            deal(self),
            wear(self.clone()),
        ])
    }
}

/// The Asset stall: who is signed in, over its shelf or the fitting.
pub(super) fn stall() -> AnyView {
    column((
        trade::account("to buy, and to wear what you bought."),
        modes(),
        keyed(resource::<Fitting, _>(|fitting| fitting.on), |on| {
            match on {
                true => fitting::fitting(),
                false => shelf::stall::<Asset>(),
            }
        }),
    ))
    .boxed()
}

/// The shelf and the fitting, the one shown lit.
fn modes() -> AnyView {
    keyed(resource::<Fitting, _>(|fitting| fitting.on), |on| {
        let mode = |which: &str, words: &str, lit: bool, to: bool| {
            icon_button(which.to_string(), words.to_string())
                .style(if lit {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Quiet
                })
                .on_click(move |world| {
                    world.resource_mut::<Fitting>().on = to
                })
                .boxed()
        };
        row((
            mode("tray", "SHELF", !*on, false),
            mode("robot", "FITTING", *on, true),
        ))
        .space(Space::Half)
        .boxed()
    })
    .boxed()
}

/// What an asset is and what wears it, in a few words.
pub(super) fn kind(asset: &Asset) -> String {
    let by = match asset.target {
        Target::Agent => "agents",
        Target::Hamlet => "hamlets",
    };
    format!("{} for {by}", asset.category.one())
}

/// What part of the shelf's filter a row of chips reads and sets.
type Part<T> = (
    fn(&<Asset as Listing>::Filter) -> Option<T>,
    fn(&mut <Asset as Listing>::Filter, Option<T>),
);

/// A word to press for each of `all`, and ALL, the one chosen lit,
/// choosing the part `part` of the shelf's filter.
fn chips<T: Copy + PartialEq + Send + Sync + 'static>(
    all: &'static [T],
    named: fn(T) -> &'static str,
    (get, set): Part<T>,
) -> AnyView {
    let chosen =
        resource::<Shelf<Asset>, _>(move |shelf| get(&shelf.filter));
    keyed(chosen, move |chosen| {
        let chip = |text: String, on: bool, to: Option<T>| {
            button(text)
                .style(if on {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Quiet
                })
                .on_click(move |world| {
                    browse::<Asset>(world, |shelf| {
                        set(&mut shelf.filter, to)
                    })
                })
                .boxed()
        };
        let mut chips =
            vec![chip("ALL".into(), chosen.is_none(), None)];
        chips.extend(all.iter().map(|one| {
            chip(
                named(*one).to_uppercase(),
                *chosen == Some(*one),
                Some(*one),
            )
        }));
        row(chips).space(Space::Half).boxed()
    })
    .boxed()
}

/// OWNED beside the title of the asset `id`, once it is the chief's,
/// and WORN once anyone wears it.
fn mark(id: String) -> AnyView {
    let mark = derived(move |world: &World| {
        let wardrobe = world.resource::<Wardrobe>();
        match wardrobe.owns(&id) {
            true if !wardrobe.wearing.wearers(&id).is_empty() => {
                Some("WORN")
            }
            true => Some("OWNED"),
            false => None,
        }
    })
    .when(moved::<Wardrobe>());
    keyed(mark, |mark| match mark {
        Some(mark) => row((
            icon("check").tone(Tone::Accent),
            label(*mark)
                .size(Size::Small)
                .tone(Tone::Accent)
                .face(Face::Display),
        ))
        .space(Space::Half)
        .boxed(),
        None => column(()).boxed(),
    })
    .boxed()
}

/// Where an asset stands with the chief, and its deal if one is going
/// or just went.
#[derive(Clone, PartialEq, Debug)]
struct Stand {
    owned: bool,
    signed_in: bool,
    deal: Option<Deal>,
}

/// The box `asset` is bought from.
fn deal(asset: &Asset) -> AnyView {
    let (id, cents) = (asset.id.clone(), asset.price_cents);
    let read = id.clone();
    let stand = derived(move |world: &World| Stand {
        owned: world.resource::<Wardrobe>().owns(&read),
        signed_in: world.resource::<Market>().signed_in(),
        deal: world
            .resource::<AssetDeal>()
            .0
            .as_ref()
            .filter(|(dealt, _)| *dealt == read)
            .map(|(_, deal)| deal.clone()),
    })
    .when(either(
        either(moved::<Wardrobe>(), moved::<AssetDeal>()),
        moved::<Market>(),
    ));
    keyed(stand, move |stand| {
        let mut parts = vec![
            row((
                label(price_words(cents)).bold(true),
                note(if cents > 0 { "once" } else { "" }),
            ))
            .space(Space::Half)
            .boxed(),
            get(&id, cents, stand),
        ];
        let (mut actions, mut notes) = (Vec::new(), Vec::new());
        trade::told(
            stand.deal.as_ref(),
            "It is yours as soon as Stripe says it is paid.",
            &mut actions,
            &mut notes,
        );
        parts.extend(actions);
        parts.extend(notes);
        column(parts).fill(Fill::Card).space(Space::Half).boxed()
    })
    .boxed()
}

/// The way the asset `id` is had as it stands, or how its deal goes.
fn get(id: &str, cents: u32, stand: &Stand) -> AnyView {
    if stand.deal.as_ref().is_some_and(Deal::going) {
        return trade::going(stand.deal.as_ref())
            .unwrap_or_else(|| column(()).boxed());
    }
    if stand.owned {
        return row((
            icon("check").tone(Tone::Accent),
            note("YOURS"),
        ))
        .space(Space::Half)
        .boxed();
    }
    if !stand.signed_in {
        return description("Sign in with GitHub to get it.").boxed();
    }
    let id = id.to_string();
    let (which, words) = match cents {
        0 => ("plus", "GET"),
        _ => ("coins", "BUY"),
    };
    row((icon_button(which, words)
        .style(ButtonStyle::Primary)
        .on_click(move |world| wardrobe::buy(world, &id)),))
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

/// All the box an owned asset is worn from is drawn from.
#[derive(Clone, PartialEq, Debug)]
struct Wearing {
    owned: bool,
    /// Who may wear it, by name, with their words.
    may: Vec<(String, String)>,
    chosen: usize,
    /// Who wears it, by name, with their words.
    on: Vec<(String, String)>,
    /// How its file stands, in words, once it is worn.
    file: Option<String>,
    said: Option<(String, bool)>,
}

impl Wearing {
    fn of(world: &World, asset: &Asset) -> Wearing {
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
        Wearing {
            owned: wardrobe.owns(&asset.id),
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

/// The box an owned asset is worn from: who it goes on, and who wears
/// it, each to be taken off.
fn wear(asset: Asset) -> AnyView {
    let read = asset.clone();
    let now = derived(move |world: &World| Wearing::of(world, &read))
        .when(either(
            either(moved::<Wardrobe>(), moved::<Rostered>()),
            either(moved::<Fitting>(), moved::<Closet>()),
        ));
    keyed(now, move |now| {
        if !now.owned {
            return column(()).boxed();
        }
        let (id, target, category) =
            (asset.id.clone(), asset.target, asset.category);
        let mut parts = Vec::new();
        if now.may.is_empty() {
            parts.push(
                description(match target {
                    Target::Agent => {
                        "There is no agent to wear it yet."
                    }
                    Target::Hamlet => {
                        "There is no hamlet to wear it yet."
                    }
                })
                .boxed(),
            );
        } else {
            let names = now
                .may
                .iter()
                .map(|(name, _)| name.clone())
                .collect();
            let words = now
                .may
                .iter()
                .map(|(_, words)| words.clone())
                .collect::<Vec<_>>();
            let on = id.clone();
            parts.push(
                row((
                    dropdown(words, now.chosen as u32).on_pick(
                        |world, at| {
                            world.resource_mut::<Fitting>().wearer =
                                at as usize
                        },
                    ),
                    icon_button("check", "WEAR")
                        .style(ButtonStyle::Primary)
                        .on_click(move |world| {
                            let names: &Vec<String> = &names;
                            let at =
                                world.resource::<Fitting>().wearer;
                            if let Some(name) =
                                names.get(at.min(names.len() - 1))
                            {
                                wardrobe::wear(
                                    world,
                                    (target, name),
                                    category,
                                    Some(on.clone()),
                                );
                            }
                        }),
                ))
                .space(Space::Half)
                .boxed(),
            );
        }
        for (name, words) in &now.on {
            let name = name.clone();
            parts.push(
                row((
                    note(format!("{words} wears it.")),
                    icon_button("cross", "TAKE OFF")
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
                .space(Space::Half)
                .boxed(),
            );
        }
        if let Some(file) = &now.file {
            parts.push(description(file.clone()).boxed());
        }
        if let Some((words, wrong)) = &now.said {
            let tone =
                if *wrong { Tone::Critical } else { Tone::Accent };
            parts.push(label(words.clone()).tone(tone).boxed());
        }
        section("WEAR IT", column(parts).space(Space::Half).boxed())
    })
    .boxed()
}
