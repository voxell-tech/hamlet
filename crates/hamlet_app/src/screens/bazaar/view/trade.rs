//! Trading at the stalls that sell what chiefs made: signing in to
//! the bazaar with GitHub, the bazaar's shelves or, where it cannot
//! be reached, the samples, and the way a listing is bought,
//! installed or brought up to date on its page.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, percent};
use bevy_fynix::views::{
    FrameProps, column, dropdown, label, row, scroll,
};
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    Fills, ICON, action, bare, buttons, command, filling, note, title,
};
use hamlet_ui::widgets::icons::Glyph;

use super::desk;
use super::parts::{
    self, crumb, mark as accent, nothing, outcome, problem, wide,
};
use super::shelf::{self, Shown};
use super::wares::price;
use crate::bazaar::listings::{SELLERS_SHARE, Sells, Wares};
use crate::bazaar::market::deal::{Dealing, Places, targets};
use crate::bazaar::market::lot::{Kind, price_words, semver};
use crate::bazaar::market::{
    self, Account, Deal, Market, Reach, ask_shelves, buy, install,
    sign_in, sign_out, update,
};
use crate::roll::{either, hamlets_moved, moved};

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
        .within(filling())
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
    .within(filling())
    .boxed()
}

/// The page the chief signs in from, as Welcome shows its sign-in:
/// what to do, the code GitHub gave once it did and the way to
/// GitHub, and how it goes.
fn signing_in(
    back: &'static str,
    code: Option<(String, String)>,
) -> AnyView {
    let body = match code {
        None => vec![
            said("GitHub is asked for a code to sign in with."),
            said("Asking GitHub..."),
            row((command("CANCEL", sign_out),)).boxed(),
        ],
        Some((code, uri)) => vec![
            said(format!(
                "Enter this code at {uri} and choose Authorize. The \
                 bazaar carries on once you have."
            )),
            title(code).boxed(),
            buttons(vec![
                action(Glyph::Globe, "OPEN GITHUB", move |world| {
                    market::browse(world, &uri);
                }),
                command("CANCEL", sign_out).boxed(),
            ])
            .boxed(),
            said("Waiting for GitHub..."),
            note(
                "Nothing opened? Go to the address above in your \
                 browser.",
            )
            .boxed(),
        ],
    };
    column((
        crumb(back, sign_out, "SIGN IN".into()),
        scroll((column(body).gap(16.0),)).fills_height(),
    ))
    .gap(14.0)
    .fills_height()
    .boxed()
}

/// Words of a page that stand back, as Welcome says them.
fn said(words: impl Into<String>) -> AnyView {
    label(words.into())
        .size(13.0)
        .tone(bevy_fynix::tokens::Tone::Dim)
        .boxed()
}

/// Under the head of the samples: that they are made up.
pub(super) fn samples_notice() -> AnyView {
    note(
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
                action(Glyph::Restore, "ASK AGAIN", ask_shelves),
                column((note(format!(
                    "Asks the bazaar at {server} again, which \
                     HAMLET_BAZAAR_URL can change."
                )),))
                .fills_width(),
            ))
            .gap(12.0)
            .align(AlignItems::Center),
        ))
        .gap(8.0)
        .boxed(),
        _ => note(format!(
            "The bazaar at {server} is asked for what it sells."
        ))
        .boxed(),
    })
    .within(bare())
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
            return nothing();
        }
        let sell = match kind {
            Kind::Agent => "SELL AN AGENT",
            Kind::Hamlet => "SELL A HAMLET",
            Kind::Plugin => "SELL TOOLS",
        };
        buttons(vec![
            action(Glyph::Plus, sell, move |world| {
                desk::open_form(world, kind, None);
            }),
            action(Glyph::Book, "YOURS", desk::open_yours),
        ])
        .boxed()
    })
    .within(bare())
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
                action(Glyph::Power, "SIGN OUT", sign_out),
                note(format!(
                    "Signed in to the bazaar as @{}.",
                    user.login
                ))
                .boxed(),
            ),
            Account::Failed(why) => (
                action(Glyph::Signpost, "SIGN IN", sign_in),
                problem(why.clone()),
            ),
            _ => (
                action(Glyph::Signpost, "SIGN IN", sign_in),
                note(format!("With GitHub, {what_for}")).boxed(),
            ),
        };
        let mut parts = Vec::new();
        parts.extend(
            said.clone().map(|(words, wrong)| outcome(words, wrong)),
        );
        parts.push(
            row((button, column((beside,)).fills_width()))
                .gap(12.0)
                .align(AlignItems::Center)
                .boxed(),
        );
        column(parts).gap(8.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// Where a listing stands with the chief.
#[derive(Clone, PartialEq, Debug)]
enum Stand {
    /// They sell it.
    Mine,
    /// Settled into the home at `version`; `newer` is out.
    Kept {
        version: String,
        newer: Option<String>,
    },
    /// Bought and not settled into the home.
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
            .deal
            .as_ref()
            .filter(|(order, _)| order.id == id)
            .map(|(_, deal)| deal.clone());
        let places = targets(world, kind);
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
        None => nothing(),
    })
    .within(bare())
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
                either(moved::<Places>(), hamlets_moved()),
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
                note(format!(
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
        let mut all = Vec::new();
        all.extend(place);
        all.push(parts::deal(None, buttons, notes));
        parts::deal(price, Vec::new(), all)
    })
    .within(bare())
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
            notes.push(outcome(words.clone(), false));
        }
        Some(Deal::Failed(why)) => notes.push(problem(why.clone())),
        Some(Deal::Paying { url, .. }) => {
            let url = url.clone();
            buttons.push(action(
                Glyph::Coins,
                "OPEN THE PAYMENT PAGE",
                move |world| market::browse(world, &url),
            ));
            notes.push(
                note(format!(
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
        Some(Deal::Fetching) => "Downloading it...",
        Some(Deal::Settling) => "Setting it up...",
        _ => return None,
    };
    Some(note(going).boxed())
}

/// The choice of where an agent goes, as the Inspector has the choice
/// of an agent's model: its name over it.
fn place_choice(now: &Now) -> AnyView {
    column((
        note("Goes"),
        wide(
            dropdown(
                now.places.clone(),
                now.chosen,
                Glyph::Chevron.image(ICON),
                |world, at| {
                    world.resource_mut::<Places>().hamlet = at;
                },
            )
            .width(percent(100.0))
            .max_width(percent(100.0)),
        ),
    ))
    .gap(6.0)
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
    let choice = (now.places.len() > 1).then(|| place_choice(now));
    match &now.stand {
        Stand::Mine => {
            buttons.push(action(
                Glyph::Rename,
                "CHANGE THIS LISTING",
                move |world| desk::open_edit(world, &id),
            ));
            None
        }
        Stand::Kept { version, newer } => {
            notes.push(
                note(format!("Version {version} is installed."))
                    .boxed(),
            );
            if let Some(newer) = newer {
                buttons.push(action(
                    Glyph::Restore,
                    format!("UPDATE TO {newer}"),
                    move |world| update(world, &id),
                ));
                notes.push(
                    note(format!(
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
            notes.push(note(nowhere(kind)).boxed());
            None
        }
        Stand::Owned => {
            buttons.push(action(
                Glyph::Plus,
                "INSTALL",
                move |world| {
                    install(world, &id);
                },
            ));
            choice
        }
        Stand::ForSale => {
            match (&now.latest, now.places.is_empty()) {
                (None, _) => {
                    notes.push(
                        note(
                            "Not ready yet: its seller has not \
                             uploaded a version.",
                        )
                        .boxed(),
                    );
                    None
                }
                (Some(_), true) => {
                    notes.push(note(nowhere(kind)).boxed());
                    None
                }
                (Some(_), false) if !now.signed_in => {
                    buttons.push(action(
                        Glyph::Signpost,
                        "SIGN IN",
                        sign_in,
                    ));
                    notes.push(
                        note("Sign in with GitHub to get it.")
                            .boxed(),
                    );
                    None
                }
                (Some(_), false) => {
                    let (which, words) = match cents {
                        0 => (
                            Glyph::Plus,
                            "GET IT FOR FREE".to_string(),
                        ),
                        _ => (
                            Glyph::Coins,
                            format!("BUY FOR {}", price_words(cents)),
                        ),
                    };
                    buttons.push(action(
                        which,
                        words,
                        move |world| {
                            buy(world, &id);
                        },
                    ));
                    choice
                }
            }
        }
    }
}

fn nowhere(kind: Kind) -> &'static str {
    match kind {
        Kind::Agent => "There is no hamlet for it to move into yet.",
        Kind::Plugin => "There is nowhere for the tools to go.",
        Kind::Hamlet => "There is nowhere for it to go.",
    }
}
