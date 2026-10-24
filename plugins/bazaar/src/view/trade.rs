//! Trading at the stalls that sell what chiefs made: signing in to
//! the bazaar with GitHub, the bazaar's shelves or, where it cannot
//! be reached, the samples, and the way a listing is bought,
//! installed or brought up to date on its page.

use agents_ui::{Rostered, breadcrumb, either, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, FrameProps, Space, Tone, UiExt, ViewExt,
    button, column, derived, description, dropdown, heading,
    icon_button, keyed, label, row, scroll,
};

use crate::listings::{SELLERS_SHARE, Sells, Wares};
use crate::market::lot::{Kind, price_words, semver};
use crate::market::{
    Account, Deal, Dealing, Market, Places, Reach, ask_shelves, buy,
    install, sign_in, sign_out, update, {self},
};
use crate::view::desk;
use crate::view::parts::{
    self, mark as accent, note, outcome, problem,
};
use crate::view::shelf::{self, Shown};
use crate::view::wares::price;

/// The stall of `K`: the desk's page, or the bazaar's shelf when it
/// was reached, or else the samples `S`; or the page the chief signs
/// in from while they do.
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
    signing(K::BACK, move || desk::or(K::BACK, shelf))
}

/// The page the chief signs in from while they do, else what `rest`
/// makes. `back` names the stall on the way back to it, which stops
/// signing in.
pub(super) fn signing(
    back: &'static str,
    rest: impl Fn() -> AnyView + Send + Sync + 'static,
) -> AnyView {
    let signing = derived(|world: &World| {
        match &world.resource::<Market>().account {
            Account::Asking => Some(None),
            Account::Code { device, .. } => Some(Some((
                device.user_code.clone(),
                device.verification_uri.clone(),
            ))),
            _ => None,
        }
    })
    .when(moved::<Market>());
    keyed(signing, move |signing| match signing {
        Some(code) => signing_in(back, code.clone()),
        None => rest(),
    })
    .boxed()
}

/// The page the chief signs in from, as Welcome shows its sign-in:
/// what to do, the code GitHub gave once it did and the way to
/// GitHub, and how it goes.
fn signing_in(
    back: &'static str,
    code: Option<(String, String)>,
) -> AnyView {
    let cancel = || {
        button("CANCEL")
            .style(ButtonStyle::Quiet)
            .on_click(sign_out)
            .boxed()
    };
    let body = match code {
        None => vec![
            said("GitHub is asked for a code to sign in with."),
            said("Asking GitHub..."),
            row((cancel(),)).boxed(),
        ],
        Some((code, uri)) => vec![
            said(format!(
                "Enter this code at {uri} and choose Authorize. The \
                 bazaar carries on once you have."
            )),
            heading(code).boxed(),
            row((
                icon_button("globe", "OPEN GITHUB").on_click(
                    move |world| market::browse(world, &uri),
                ),
                cancel(),
            ))
            .space(Space::Half)
            .boxed(),
            said("Waiting for GitHub..."),
            description(
                "Nothing opened? Go to the address above in your \
                 browser.",
            )
            .boxed(),
        ],
    };
    column((
        breadcrumb(vec![back.to_string()], "SIGN IN", |world, _| {
            sign_out(world)
        }),
        scroll((column((column(body).space(Space::Double),))
            .padding(Space::Half),)),
    ))
    .boxed()
}

/// Words of a page that stand back, as Welcome says them.
fn said(words: impl Into<String>) -> AnyView {
    label(words.into()).tone(Tone::Dim).boxed()
}

/// Under the head of the samples: that they are made up.
pub(super) fn samples_notice() -> AnyView {
    description(
        "Samples, made up to show the stall: nothing here is had or \
         paid.",
    )
    .boxed()
}

/// At the foot of the samples, as LOOK AGAIN is at the foot of a
/// shelf: how asking the bazaar goes, or why it could not be reached,
/// in red, over ASK AGAIN with what it does beside it.
pub(super) fn samples_foot() -> AnyView {
    let now = derived(|world: &World| {
        let market = world.resource::<Market>();
        (market.reach.clone(), market.server.base.clone())
    })
    .when(moved::<Market>());
    keyed(now, |(reach, server)| match reach {
        Reach::Unreached(why) => column((
            problem(why.clone()),
            row((
                icon_button("restore", "ASK AGAIN")
                    .on_click(ask_shelves),
                column((description(format!(
                    "Asks the bazaar at {server} again, which \
                     HAMLET_BAZAAR_URL can change."
                )),))
                .grow(1.0),
            ))
            .space(Space::Gap),
        ))
        .space(Space::Half)
        .boxed(),
        _ => description(format!(
            "The bazaar at {server} is asked for what it sells."
        ))
        .boxed(),
    })
    .boxed()
}

/// Under the head of the bazaar's shelf of `kind`, while the chief is
/// signed in, as the Inspector has FOUND A HAMLET under its head: the
/// way to sell one, and to see what is theirs.
pub(super) fn selling(kind: Kind) -> AnyView {
    let signed = derived(|world: &World| {
        world.resource::<Market>().signed_in()
    })
    .when(moved::<Market>());
    keyed(signed, move |signed| {
        if !*signed {
            return column(()).boxed();
        }
        let sell = match kind {
            Kind::Agent => "SELL AN AGENT",
            Kind::Hamlet => "SELL A HAMLET",
            Kind::Plugin => "SELL TOOLS",
        };
        row((
            icon_button("plus", sell).on_click(move |world| {
                desk::open_form(world, kind, None)
            }),
            icon_button("book", "YOURS").on_click(desk::open_yours),
        ))
        .space(Space::Half)
        .boxed()
    })
    .boxed()
}

/// At the foot of a shelf, as the Inspector has YOUR HEARTH at the
/// foot of its hamlets: SIGN IN with what it is `for` beside it, or
/// SIGN OUT beside who is signed in; and what came of the last thing
/// asked of the bazaar over it.
pub(super) fn account(what_for: &'static str) -> AnyView {
    let now = derived(|world: &World| {
        let market = world.resource::<Market>();
        (market.account.clone(), market.said.clone())
    })
    .when(moved::<Market>());
    keyed(now, move |(account, said)| {
        let (button, beside) = match account {
            Account::In(user) => (
                icon_button("power", "SIGN OUT").on_click(sign_out),
                description(format!(
                    "Signed in to the bazaar as @{}.",
                    user.login
                ))
                .boxed(),
            ),
            Account::Failed(why) => (
                icon_button("signpost", "SIGN IN").on_click(sign_in),
                problem(why.clone()),
            ),
            _ => (
                icon_button("signpost", "SIGN IN").on_click(sign_in),
                description(format!("With GitHub, {what_for}"))
                    .boxed(),
            ),
        };
        let mut parts = Vec::new();
        parts.extend(
            said.clone().map(|(words, wrong)| outcome(words, wrong)),
        );
        parts.push(
            row((button, column((beside,)).grow(1.0)))
                .space(Space::Gap)
                .boxed(),
        );
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

/// Where the listing `id` stands with the chief, as its accent line.
pub(super) fn mark(id: String) -> AnyView {
    let mark = derived(move |world: &World| {
        let market = world.resource::<Market>();
        if market.bought.contains_key(&id) {
            Some("Installed")
        } else if market.owns(&id) {
            Some("You own this")
        } else if market.sells(&id) {
            Some("You sell this")
        } else {
            None
        }
    })
    .when(moved::<Market>());
    keyed(mark, |mark| match mark {
        Some(mark) => accent(mark),
        None => column(()).boxed(),
    })
    .boxed()
}

/// The way the listing `id` of `kind` is had, as the bazaar's other
/// pages have theirs: its price while it is for sale, where it goes,
/// the button that buys, installs or brings it up to its newest
/// version, and what that does.
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
        let mut buttons = Vec::new();
        let mut notes = Vec::new();
        let place =
            get(kind, &id, cents, now, &mut buttons, &mut notes);
        told(
            now.deal.as_ref(),
            "It is set up as soon as Stripe says it is paid.",
            &mut buttons,
            &mut notes,
        );
        if cents > 0 && now.stand == Stand::ForSale {
            notes.push(
                description(format!(
                    "{seller} receives {SELLERS_SHARE}% of each \
                     sale."
                ))
                .boxed(),
            );
        }
        let price = match now.stand {
            Stand::ForSale => Some(price(cents)),
            _ => None,
        };
        let mut parts = Vec::new();
        parts.extend(place);
        parts.push(parts::deal(None, buttons, notes));
        parts::deal(price, Vec::new(), parts)
    })
    .boxed()
}

/// What came of `deal`, or the way to finish paying for it, into
/// `buttons` and `notes`, `paid` saying what follows the payment.
pub(super) fn told(
    deal: Option<&Deal>,
    paid: &str,
    buttons: &mut Vec<AnyView>,
    notes: &mut Vec<AnyView>,
) {
    match deal {
        Some(Deal::Done(words)) => {
            notes.push(outcome(words.clone(), false))
        }
        Some(Deal::Failed(why)) => notes.push(problem(why.clone())),
        Some(Deal::Paying { url, .. }) => {
            let url = url.clone();
            buttons.push(
                icon_button("coins", "OPEN THE PAYMENT PAGE")
                    .on_click(move |world| {
                        market::browse(world, &url)
                    })
                    .boxed(),
            );
            notes.push(
                description(format!(
                    "Waiting for Stripe: finish paying on the page \
                     that opened in your browser. {paid}"
                ))
                .boxed(),
            );
        }
        _ => {}
    }
}

/// How `deal` goes while it does, as one dim line; paying says it
/// with the way to finish it, in [`told`].
pub(super) fn going(deal: Option<&Deal>) -> Option<AnyView> {
    let going = match deal {
        Some(Deal::Buying) => "Asking the bazaar...",
        Some(Deal::Fetching(_)) => "Downloading it...",
        Some(Deal::Settling(_)) => "Setting it up...",
        _ => return None,
    };
    Some(description(going).boxed())
}

/// The choice of where a listing of `kind` goes, as the Inspector
/// has the choice of an agent's model: its name over it.
fn place_choice(kind: Kind, now: &Now) -> AnyView {
    column((
        note("Goes to"),
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

/// What updating a listing of `kind` keeps and replaces.
fn update_words(kind: Kind) -> &'static str {
    match kind {
        Kind::Agent => {
            "Updating replaces its character and skills; its notes \
             stay its own."
        }
        Kind::Hamlet => {
            "Updating replaces its agents' characters and skills and \
             moves in any new ones; their notes stay their own."
        }
        Kind::Plugin => "Updating replaces its tools.",
    }
}

/// The way the listing `id` is had as it stands, or how its deal
/// goes, into `buttons` and `notes`; and the choice of where it goes,
/// if there is one to make.
fn get(
    kind: Kind,
    id: &str,
    cents: u32,
    now: &Now,
    buttons: &mut Vec<AnyView>,
    notes: &mut Vec<AnyView>,
) -> Option<AnyView> {
    if now.deal.as_ref().is_some_and(Deal::going) {
        notes.extend(going(now.deal.as_ref()));
        return None;
    }
    let id = id.to_string();
    let choice =
        (now.places.len() > 1).then(|| place_choice(kind, now));
    match &now.stand {
        Stand::Mine => {
            buttons.push(
                icon_button("pencil", "CHANGE THIS LISTING")
                    .on_click(move |world| {
                        desk::open_edit(world, &id)
                    })
                    .boxed(),
            );
            None
        }
        Stand::Kept { version, newer } => {
            notes.push(
                description(format!(
                    "Version {version} is installed."
                ))
                .boxed(),
            );
            if let Some(newer) = newer {
                buttons.push(
                    icon_button(
                        "restore",
                        format!("UPDATE TO {newer}"),
                    )
                    .on_click(move |world| update(world, &id))
                    .boxed(),
                );
                notes.push(
                    description(format!(
                        "Version {newer} is out, and updating is up \
                         to you. {}",
                        update_words(kind)
                    ))
                    .boxed(),
                );
            }
            None
        }
        Stand::Owned if now.places.is_empty() => {
            notes.push(description(nowhere(kind)).boxed());
            None
        }
        Stand::Owned => {
            buttons.push(
                icon_button("plus", "INSTALL")
                    .on_click(move |world| install(world, &id))
                    .boxed(),
            );
            choice
        }
        Stand::ForSale => {
            match (&now.latest, now.places.is_empty()) {
                (None, _) => {
                    notes.push(
                        description(
                            "Not ready yet: its seller has not \
                             uploaded a version.",
                        )
                        .boxed(),
                    );
                    None
                }
                (Some(_), true) => {
                    notes.push(description(nowhere(kind)).boxed());
                    None
                }
                (Some(_), false) if !now.signed_in => {
                    buttons.push(
                        icon_button("signpost", "SIGN IN")
                            .on_click(sign_in)
                            .boxed(),
                    );
                    notes.push(
                        description("Sign in with GitHub to get it.")
                            .boxed(),
                    );
                    None
                }
                (Some(_), false) => {
                    let (which, words) = match cents {
                        0 => ("plus", "GET IT FOR FREE".to_string()),
                        _ => (
                            "coins",
                            format!("BUY FOR {}", price_words(cents)),
                        ),
                    };
                    buttons.push(
                        icon_button(which, words)
                            .on_click(move |world| buy(world, &id))
                            .boxed(),
                    );
                    choice
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
