//! Trading at the stalls that sell what chiefs made: signing in to
//! the bazaar with GitHub, the bazaar's shelves or, where it cannot
//! be reached, the samples, and the box a listing is bought,
//! installed or brought up to date from.

use agents_ui::{Rostered, either, moved, note};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, Fill, FrameProps, Size,
    Space, Tone, UiExt, ViewExt, column, derived, description,
    dropdown, icon, icon_button, keyed, label, row,
};

use crate::listings::{SELLERS_SHARE, Sells, Wares};
use crate::market::lot::{Kind, price_words, semver};
use crate::market::{
    Account, Deal, Dealing, Market, Places, Reach, ask_shelves, buy,
    install, sign_in, sign_out, update, {self},
};
use crate::view::desk;
use crate::view::shelf::{self, Shown};

/// The stall of `K`: who is signed in over the desk's page, or over
/// the bazaar's shelf when it was reached, or else the samples `S`.
pub(super) fn stall<K: Sells, S: Shown>() -> AnyView
where
    Wares<K>: Shown,
{
    let shelf = || {
        let reached = derived(|world: &World| {
            world.resource::<Market>().reach == Reach::Reached
        })
        .when(moved::<Market>());
        keyed(reached, |reached| match reached {
            true => shelf::stall::<Wares<K>>(),
            false => shelf::stall::<S>(),
        })
        .boxed()
    };
    column((account(K::KIND), desk::or(shelf))).boxed()
}

/// Under the head of the samples: that they are made up, and how
/// asking the bazaar goes.
pub(super) fn samples_notice() -> AnyView {
    let now = derived(|world: &World| {
        let market = world.resource::<Market>();
        (market.reach.clone(), market.server.base.clone())
    })
    .when(moved::<Market>());
    keyed(now, |(reach, server)| {
        let state = match reach {
            Reach::Asking | Reach::Unasked => {
                format!("Asking the bazaar at {server}...")
            }
            Reach::Unreached(why) => format!(
                "{why} It is asked at {server}, which \
                 HAMLET_BAZAAR_URL can change."
            ),
            Reach::Reached => String::new(),
        };
        let mut parts = vec![
            label("SAMPLES")
                .size(Size::Small)
                .tone(Tone::Accent)
                .face(Face::Display)
                .boxed(),
            description(
                "Made up, to show the stall: nothing here is had or \
                 paid.",
            )
            .boxed(),
        ];
        if !state.is_empty() {
            parts.push(description(state).boxed());
        }
        if matches!(reach, Reach::Unreached(_)) {
            parts.push(
                row((icon_button("restore", "ASK THE BAZAAR AGAIN")
                    .style(ButtonStyle::Quiet)
                    .on_click(ask_shelves),))
                .boxed(),
            );
        }
        column(parts).space(Space::Half).boxed()
    })
    .boxed()
}

/// Who is signed in to the bazaar, and the ways to sign in, out, sell
/// and see what is the chief's. Selling begins at a listing of
/// `kind`.
fn account(kind: Kind) -> AnyView {
    let now = derived(|world: &World| {
        let market = world.resource::<Market>();
        (market.account.clone(), market.said.clone())
    })
    .when(moved::<Market>());
    keyed(now, move |(account, said)| {
        let sign_in_button = || {
            icon_button("signpost", "SIGN IN WITH GITHUB")
                .style(ButtonStyle::Primary)
                .on_click(sign_in)
                .boxed()
        };
        let mut parts = vec![match account {
            Account::Out => row((
                sign_in_button(),
                note("to buy, and to sell what you made."),
            ))
            .space(Space::Half)
            .boxed(),
            Account::Asking => row((
                icon("clock").tone(Tone::Faint),
                description("Asking GitHub..."),
            ))
            .space(Space::Half)
            .boxed(),
            Account::Code { device, .. } => {
                let uri = device.verification_uri.clone();
                column((
                    row((
                        note("Enter"),
                        label(device.user_code.clone()).bold(true),
                        note(format!("at {uri}")),
                    ))
                    .space(Space::Half),
                    row((
                        icon_button("globe", "OPEN GITHUB").on_click(
                            move |world| market::browse(world, &uri),
                        ),
                        icon_button("cross", "CANCEL")
                            .style(ButtonStyle::Quiet)
                            .on_click(sign_out),
                    ))
                    .space(Space::Half),
                ))
                .space(Space::Half)
                .boxed()
            }
            Account::In(user) => row((
                icon("check").tone(Tone::Accent),
                note(format!("@{}", user.login)),
                icon_button("plus", "SELL")
                    .style(ButtonStyle::Primary)
                    .on_click(move |world| {
                        desk::open_form(world, kind, None)
                    }),
                icon_button("book", "YOURS")
                    .on_click(desk::open_yours),
                icon_button("power", "SIGN OUT")
                    .style(ButtonStyle::Quiet)
                    .on_click(sign_out),
            ))
            .space(Space::Half)
            .boxed(),
            Account::Failed(why) => column((
                label(why.clone()).tone(Tone::Critical),
                row((sign_in_button(),)),
            ))
            .space(Space::Half)
            .boxed(),
        }];
        if let Some((words, wrong)) = said {
            let tone =
                if *wrong { Tone::Critical } else { Tone::Accent };
            parts.push(label(words.clone()).tone(tone).boxed());
        }
        column(parts).space(Space::Half).boxed()
    })
    .boxed()
}

/// Where a listing stands with the chief.
#[derive(Clone, PartialEq, Debug)]
enum Stand {
    /// They sell it.
    Mine,
    /// Written into the home at `version`; `newer` is out.
    Kept {
        version: String,
        newer: Option<String>,
    },
    /// Bought and not written into the home.
    Owned,
    ForSale,
}

/// All the box of a listing is drawn from.
#[derive(Clone, PartialEq, Debug)]
struct Now {
    stand: Stand,
    signed_in: bool,
    /// The deal of this listing, if one is going or just went.
    deal: Option<Deal>,
    /// Its newest version, if it has one.
    latest: Option<String>,
    /// The places it can go, by name, and the one chosen.
    places: Vec<String>,
    chosen: usize,
}

impl Now {
    fn of(world: &World, kind: Kind, id: &str) -> Now {
        let market = world.resource::<Market>();
        let latest = market
            .lot(id)
            .map(|lot| lot.version().to_string())
            .filter(|latest| !latest.is_empty());
        let stand = match market.bought.get(id) {
            Some(bought) => Stand::Kept {
                version: bought.version.clone(),
                newer: latest.clone().filter(|latest| {
                    semver(latest) > semver(&bought.version)
                }),
            },
            None if market.sells(id) => Stand::Mine,
            None if market.owns(id) => Stand::Owned,
            None => Stand::ForSale,
        };
        let deal = world
            .resource::<Dealing>()
            .0
            .as_ref()
            .filter(|(order, _)| order.id == id)
            .map(|(_, deal)| deal.clone());
        let places =
            market::targets(world.resource::<Rostered>(), kind);
        let chosen =
            world.resource::<Places>().at(kind, places.len());
        Now {
            stand,
            signed_in: market.signed_in(),
            deal,
            latest,
            places: places
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
            chosen,
        }
    }
}

/// Where the listing `id` stands with the chief, beside its title.
pub(super) fn mark(id: String) -> AnyView {
    let mark = derived(move |world: &World| {
        let market = world.resource::<Market>();
        if market.bought.contains_key(&id) {
            Some("INSTALLED")
        } else if market.owns(&id) {
            Some("OWNED")
        } else if market.sells(&id) {
            Some("SELLING")
        } else {
            None
        }
    })
    .when(moved::<Market>());
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

/// The box the listing `id` of `kind` is had from: bought, installed
/// or brought up to its newest version, with where it goes.
pub(super) fn deal(
    kind: Kind,
    id: String,
    cents: u32,
    seller: String,
) -> AnyView {
    let read = id.clone();
    let now =
        derived(move |world: &World| Now::of(world, kind, &read))
            .when(either(
                either(moved::<Market>(), moved::<Dealing>()),
                either(moved::<Places>(), moved::<Rostered>()),
            ));
    keyed(now, move |now| {
        let mut parts = vec![
            row((
                label(price_words(cents)).bold(true),
                note(if cents > 0 { "once" } else { "" }),
            ))
            .space(Space::Half)
            .boxed(),
        ];
        if cents > 0 && now.stand == Stand::ForSale {
            parts.push(
                note(format!(
                    "{seller} receives {SELLERS_SHARE}% of each \
                     sale."
                ))
                .boxed(),
            );
        }
        let going = now.deal.as_ref().is_some_and(Deal::going);
        if now.places.len() > 1
            && matches!(now.stand, Stand::Owned | Stand::ForSale)
            && !going
        {
            parts.push(place_choice(kind, now));
        }
        parts.push(get(kind, &id, cents, now));
        match &now.deal {
            Some(Deal::Done(words)) => parts.push(
                label(words.clone()).tone(Tone::Accent).boxed(),
            ),
            Some(Deal::Failed(why)) => parts.push(
                label(why.clone()).tone(Tone::Critical).boxed(),
            ),
            Some(Deal::Paying { url, .. }) => {
                let url = url.clone();
                parts.push(
                    column((
                        description(
                            "Finish paying on the page that opened \
                             in your browser. It is set up as soon \
                             as Stripe says it is paid.",
                        ),
                        row((icon_button("coins", "OPEN THE PAGE")
                            .on_click(move |world| {
                                market::browse(world, &url)
                            }),)),
                    ))
                    .space(Space::Half)
                    .boxed(),
                )
            }
            _ => {}
        }
        if let Stand::Kept {
            newer: Some(newer), ..
        } = &now.stand
        {
            parts
                .push(description(update_words(kind, newer)).boxed());
        }
        column(parts).fill(Fill::Card).space(Space::Half).boxed()
    })
    .boxed()
}

/// The choice of where a listing of `kind` goes.
fn place_choice(kind: Kind, now: &Now) -> AnyView {
    row((
        note("Goes"),
        dropdown(now.places.clone(), now.chosen as u32).on_pick(
            move |world, at| {
                let mut places = world.resource_mut::<Places>();
                match kind {
                    Kind::Agent => places.hamlet = at as usize,
                    Kind::Plugin => places.house = at as usize,
                    Kind::Hamlet => {}
                }
            },
        ),
    ))
    .space(Space::Half)
    .boxed()
}

fn update_words(kind: Kind, newer: &str) -> String {
    let keeps = match kind {
        Kind::Agent => {
            "Updating replaces its character and skills; its notes \
             stay its own."
        }
        Kind::Hamlet => {
            "Updating replaces its agents' characters and skills and \
             moves in any new ones; their notes stay their own."
        }
        Kind::Plugin => "Updating replaces its tools.",
    };
    format!("Version {newer} is out. Updating is up to you. {keeps}")
}

/// The way the listing `id` is had as it stands, or how its deal
/// goes.
fn get(kind: Kind, id: &str, cents: u32, now: &Now) -> AnyView {
    let going = match &now.deal {
        Some(Deal::Buying) => Some("Asking the bazaar..."),
        Some(Deal::Paying { .. }) => Some("Waiting for Stripe..."),
        Some(Deal::Fetching(_)) => Some("Downloading it..."),
        Some(Deal::Settling(_)) => Some("Setting it up..."),
        _ => None,
    };
    if let Some(going) = going {
        return row((
            icon("clock").tone(Tone::Accent),
            description(going),
        ))
        .space(Space::Half)
        .boxed();
    }
    let id = id.to_string();
    match &now.stand {
        Stand::Mine => row((
            icon("check").tone(Tone::Accent),
            note("YOURS"),
            icon_button("pencil", "EDIT")
                .on_click(move |world| desk::open_edit(world, &id)),
        ))
        .space(Space::Half)
        .boxed(),
        Stand::Kept { version, newer } => {
            let mut all = vec![
                icon("check").tone(Tone::Accent).boxed(),
                note(format!("INSTALLED {version}")).boxed(),
            ];
            if newer.is_some() {
                all.push(
                    icon_button("restore", "UPDATE")
                        .style(ButtonStyle::Primary)
                        .on_click(move |world| update(world, &id))
                        .boxed(),
                );
            }
            row(all).space(Space::Half).boxed()
        }
        Stand::Owned if now.places.is_empty() => {
            description(nowhere(kind)).boxed()
        }
        Stand::Owned => row((icon_button("plus", "INSTALL")
            .style(ButtonStyle::Primary)
            .on_click(move |world| install(world, &id)),))
        .boxed(),
        Stand::ForSale => {
            match (&now.latest, now.places.is_empty()) {
                (None, _) => description("Not ready yet.").boxed(),
                (Some(_), true) => description(nowhere(kind)).boxed(),
                (Some(_), false) if !now.signed_in => {
                    description("Sign in with GitHub to get it.")
                        .boxed()
                }
                (Some(_), false) => {
                    let (which, words) = match cents {
                        0 => ("plus", "GET"),
                        _ => ("coins", "BUY"),
                    };
                    row((icon_button(which, words)
                        .style(ButtonStyle::Primary)
                        .on_click(move |world| buy(world, &id)),))
                    .boxed()
                }
            }
        }
    }
}

fn nowhere(kind: Kind) -> &'static str {
    match kind {
        Kind::Agent => "There is no hamlet for it to move into yet.",
        Kind::Plugin => "There is no agent to give the tools to yet.",
        Kind::Hamlet => "There is nowhere for it to go.",
    }
}
