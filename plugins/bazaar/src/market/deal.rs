//! Buying a listing and writing it into the home: paid for on
//! Stripe's page when it has a price, its package streamed a part at
//! a time from the server to the agents service, which settles it
//! where the chief chose, and brought up to a newer version when they
//! ask. One deal goes at a time, each step sent once the one before
//! is answered, so no more than a part of a package is held.

use agents_ui::{AgentsWorldExt, FailureExt, Heard, Rostered};
use hamlet_engine_bevy::api::agents::{
    AgentsReply, AgentsRequest, Delivery, Destination, PACKAGE_PART,
    PackageKind,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Failure, Reply};

use crate::BazaarWorldExt;
use crate::covers::Reveal;
use crate::hamlets::{self, full};
use crate::market::account::{Asking, browse};
use crate::market::client::{Refusal, bytes, read};
use crate::market::lot::{Checkout, Kind, Paying, Purchase};
use crate::market::{Market, ask_account, send, stall_of};
use crate::shelf::now;

/// The time between asks whether a payment went through.
pub(crate) const PAYMENT_POLL: u64 = 5_000;

/// The longest a payment is waited for.
pub(crate) const PAYMENT_PATIENCE: u64 = 30 * 60 * 1000;

/// The bytes of a package downloaded, settled, packed or uploaded at
/// once: the agents service's part, within what one fetch may bring
/// (`[budgets] fetch_bytes`).
pub(crate) const PART_BYTES: u64 = PACKAGE_PART as u64;

/// How many parts a package of `size` bytes crosses in; one at least.
pub(crate) fn parts_of(size: u64) -> u32 {
    size.div_ceil(PART_BYTES).max(1) as u32
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Dealing>()
        .init_resource::<Places>()
        .add_systems(
            Update,
            tick.run_if(|dealing: Res<Dealing>| {
                matches!(dealing.0, Some((_, Deal::Paying { .. })))
            }),
        );
}

/// A listing bought or installed: which version goes where.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Order {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub version: String,
    /// The bytes of its package.
    pub size: u64,
    pub to: Destination,
}

/// How the buying or installing of a listing goes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Deal {
    /// Asked of the server.
    Buying,
    /// Paid for on Stripe's page, at this address, as this purchase,
    /// which is asked after until it is paid.
    Paying {
        url: String,
        purchase: String,
        asking: Asking,
    },
    /// The part of its package from this byte on is downloaded.
    Fetching(u64),
    /// The agents service takes the part before this byte; the last
    /// part settles it in the home.
    Settling(u64),
    Done(String),
    Failed(String),
}

impl Deal {
    /// True while it goes on.
    pub fn going(&self) -> bool {
        !matches!(self, Deal::Done(_) | Deal::Failed(_))
    }
}

/// The deal going on, or the last one.
#[derive(Resource, Default)]
pub(crate) struct Dealing(pub Option<(Order, Deal)>);

/// What happened to a deal.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    Bought(Result<Checkout, Refusal>),
    Paid(Result<Paying, Refusal>),
    Part(Result<Vec<u8>, Refusal>),
    /// The agents service took a part, not the last.
    Stored,
    /// The agents service settled the last part, or failed a part.
    Settled(Result<String, String>),
    /// The time now.
    Tick(u64),
}

/// What a deal asks to be done next.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    /// Download its package from this byte on.
    Download(u64),
    /// Ask whether this purchase was paid.
    Poll(String),
    /// Open this page in the browser.
    Open(String),
    /// Hand this part of the package to the agents service.
    Settle {
        part: u32,
        parts: u32,
        bytes: Vec<u8>,
    },
}

impl Deal {
    /// The deal after `event` of `order` at `now`, and what it asks
    /// to be done.
    pub fn next(
        self,
        order: &Order,
        event: Event,
        now: u64,
    ) -> (Deal, Option<Act>) {
        match (self, event) {
            (deal @ (Deal::Buying | Deal::Paying { .. }), event) => {
                match pay(deal, event, now) {
                    Paid::Owned => {
                        (Deal::Fetching(0), Some(Act::Download(0)))
                    }
                    Paid::Going(deal, act) => (deal, act),
                }
            }
            (Deal::Fetching(from), Event::Part(Ok(bytes))) => {
                let wanted =
                    order.size.saturating_sub(from).min(PART_BYTES);
                if bytes.len() as u64 != wanted || wanted == 0 {
                    return failed(format!(
                        "The package came wrong: {} bytes from byte \
                         {from}, where {wanted} were due.",
                        bytes.len()
                    ));
                }
                let settle = Act::Settle {
                    part: (from / PART_BYTES) as u32,
                    parts: parts_of(order.size),
                    bytes,
                };
                (Deal::Settling(from + wanted), Some(settle))
            }
            (Deal::Fetching(_), Event::Part(Err(why))) => {
                failed(format!(
                    "The package could not be had: {}",
                    why.words()
                ))
            }
            (Deal::Settling(next), Event::Stored)
                if next < order.size =>
            {
                (Deal::Fetching(next), Some(Act::Download(next)))
            }
            (Deal::Settling(next), Event::Settled(Ok(words)))
                if next >= order.size =>
            {
                (Deal::Done(words), None)
            }
            (Deal::Settling(_), Event::Settled(Err(why))) => {
                failed(why)
            }
            // What does not belong to the step it is on changes
            // nothing.
            (deal, _) => (deal, None),
        }
    }
}

/// What came of a step of buying, until what is bought is the
/// chief's.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Paid {
    /// Still buying, or failed: the deal now, and what it asks.
    Going(Deal, Option<Act>),
    /// It is the chief's now.
    Owned,
}

/// `deal`, while it is [`Deal::Buying`] or [`Deal::Paying`], moved by
/// `event` at `now`: asked of the server, then paid for on Stripe's
/// page when it has a price. The market's deals and the Asset stall's
/// share it.
pub(crate) fn pay(deal: Deal, event: Event, now: u64) -> Paid {
    let going = |(deal, act)| Paid::Going(deal, act);
    match (deal, event) {
        (Deal::Buying, Event::Bought(Ok(checkout))) => match checkout
        {
            Checkout { owned: true, .. } => Paid::Owned,
            Checkout {
                purchase_id: Some(purchase),
                checkout_url: Some(url),
                ..
            } => Paid::Going(
                Deal::Paying {
                    url: url.clone(),
                    purchase,
                    asking: Asking::new(
                        now,
                        PAYMENT_POLL,
                        PAYMENT_PATIENCE,
                    ),
                },
                Some(Act::Open(url)),
            ),
            _ => going(failed("The bazaar gave no page to pay on.")),
        },
        // Owned already.
        (Deal::Buying, Event::Bought(Err(why)))
            if why.code == "conflict" =>
        {
            Paid::Owned
        }
        (Deal::Buying, Event::Bought(Err(why))) => {
            going(failed(why.words()))
        }
        (Deal::Paying { .. }, Event::Paid(Ok(Paying::Paid))) => {
            Paid::Owned
        }
        (Deal::Paying { .. }, Event::Paid(Ok(Paying::Expired))) => {
            going(failed(
                "The payment was not made. Nothing was charged.",
            ))
        }
        // Not yet, or a blip of the network: asked again later.
        (
            Deal::Paying {
                url,
                purchase,
                mut asking,
            },
            Event::Paid(_),
        ) => {
            asking.heard(now);
            let paying = Deal::Paying {
                url,
                purchase,
                asking,
            };
            Paid::Going(paying, None)
        }
        (
            Deal::Paying {
                url,
                purchase,
                mut asking,
            },
            Event::Tick(now),
        ) => {
            if asking.over(now) {
                return going(failed(
                    "The payment was not finished in time.",
                ));
            }
            let act = asking.due(now).then(|| {
                asking.out = true;
                Act::Poll(purchase.clone())
            });
            let paying = Deal::Paying {
                url,
                purchase,
                asking,
            };
            Paid::Going(paying, act)
        }
        (deal, _) => Paid::Going(deal, None),
    }
}

fn failed(why: impl Into<String>) -> (Deal, Option<Act>) {
    (Deal::Failed(why.into()), None)
}

/// A call of the server for the deal going on.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    Purchase(String),
    Paying(String),
    /// A part of the package of the listing, from this byte on.
    Part(String, u64),
}

/// Where what is bought can go, as the realm stands, and the places
/// the chief chose by their place among them.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Places {
    pub hamlet: usize,
    pub house: usize,
}

/// Where a listing of `kind` can go, in words, as the roster stands.
pub(crate) fn targets(
    roster: &Rostered,
    kind: Kind,
) -> Vec<(String, Destination)> {
    let roster = &roster.0;
    let title = |hamlet: &str| {
        roster
            .hamlets
            .iter()
            .find(|one| one.id == hamlet)
            .map_or(hamlet.to_string(), |one| one.title.clone())
    };
    match kind {
        Kind::Hamlet => {
            vec![("the realm".into(), Destination::Realm)]
        }
        Kind::Agent => roster
            .hamlets
            .iter()
            .map(|hamlet| {
                (
                    format!("into {}", hamlet.title),
                    Destination::Hamlet(hamlet.id.clone()),
                )
            })
            .collect(),
        Kind::Plugin => roster
            .agents
            .iter()
            .filter(|agent| !agent.gone)
            .map(|agent| {
                (
                    format!(
                        "for {} of {}",
                        agent.name,
                        title(&agent.hamlet)
                    ),
                    Destination::House {
                        hamlet: agent.hamlet.clone(),
                        agent: agent.name.clone(),
                    },
                )
            })
            .collect(),
    }
}

impl Places {
    /// The place chosen among `count` for a listing of `kind`.
    pub fn at(&self, kind: Kind, count: usize) -> usize {
        let at = match kind {
            Kind::Agent => self.hamlet,
            Kind::Plugin => self.house,
            Kind::Hamlet => 0,
        };
        at.min(count.saturating_sub(1))
    }
}

/// The place chosen for a listing of `kind`.
fn chosen(world: &World, kind: Kind) -> Option<Destination> {
    let all = targets(world.resource::<Rostered>(), kind);
    let at = world.resource::<Places>().at(kind, all.len());
    all.into_iter().nth(at).map(|(_, to)| to)
}

/// Where the purchase `bought` of `kind` was written, to write its
/// newer version over it.
fn kept(
    kind: PackageKind,
    hamlet: Option<&str>,
    house: Option<&str>,
) -> Option<Destination> {
    match (kind, hamlet, house) {
        (PackageKind::Hamlet, ..) => Some(Destination::Realm),
        (PackageKind::Agent, Some(hamlet), _) => {
            Some(Destination::Hamlet(hamlet.into()))
        }
        (PackageKind::Tools, Some(hamlet), Some(agent)) => {
            Some(Destination::House {
                hamlet: hamlet.into(),
                agent: agent.into(),
            })
        }
        _ => None,
    }
}

fn going(world: &World) -> bool {
    world
        .resource::<Dealing>()
        .0
        .as_ref()
        .is_some_and(|(_, deal)| deal.going())
}

/// The order of the newest version of the listing `id`, to `to`.
fn order(world: &World, id: &str, to: Destination) -> Option<Order> {
    let lot = world.resource::<Market>().lot(id)?;
    Some(Order {
        id: id.to_string(),
        kind: lot.kind,
        title: lot.title.clone(),
        version: lot.version().to_string(),
        size: lot.latest.as_ref().map_or(0, |latest| latest.size),
        to,
    })
}

/// Why `order` cannot be had now, if it cannot.
fn refusal(world: &World, order: &Order) -> Option<String> {
    if !world.resource::<Market>().signed_in() {
        Some("Sign in with GitHub to buy.".into())
    } else if order.version.is_empty() {
        Some("It has no version to download yet.".into())
    } else if order.size == 0 {
        Some("The bazaar says nothing of its package's size.".into())
    } else if order.kind == Kind::Hamlet {
        full(world)
    } else {
        None
    }
}

/// Starts `order` with `first`, unless it cannot be had.
fn start(world: &mut World, order: Order, first: Deal) {
    if going(world) {
        return;
    }
    let deal = match refusal(world, &order) {
        Some(why) => Deal::Failed(why),
        None => first,
    };
    let act = match &deal {
        Deal::Buying => Some(Call::Purchase(order.id.clone())),
        Deal::Fetching(from) => {
            Some(Call::Part(order.id.clone(), *from))
        }
        _ => None,
    };
    world.resource_mut::<Dealing>().0 = Some((order.clone(), deal));
    if let Some(call) = act {
        ask(world, &order, call);
    }
}

/// Buys the listing `id`, then writes its newest version where the
/// chief chose.
pub(crate) fn buy(world: &mut World, id: &str) {
    let Some(kind) =
        world.resource::<Market>().lot(id).map(|lot| lot.kind)
    else {
        return;
    };
    let Some(to) = chosen(world, kind) else {
        return;
    };
    if let Some(order) = order(world, id, to) {
        start(world, order, Deal::Buying);
    }
}

/// Writes the newest version of the listing `id`, owned, where the
/// chief chose.
pub(crate) fn install(world: &mut World, id: &str) {
    let Some(kind) =
        world.resource::<Market>().lot(id).map(|lot| lot.kind)
    else {
        return;
    };
    let Some(to) = chosen(world, kind) else {
        return;
    };
    if let Some(order) = order(world, id, to) {
        start(world, order, Deal::Fetching(0));
    }
}

/// Writes the newest version of the listing `id` over the one written
/// before, where that went.
pub(crate) fn update(world: &mut World, id: &str) {
    let market = world.resource::<Market>();
    let Some(bought) = market.bought.get(id) else {
        return;
    };
    let Some(to) = kept(
        bought.kind,
        bought.hamlet.as_deref(),
        bought.house.as_deref(),
    ) else {
        return;
    };
    if let Some(order) = order(world, id, to) {
        start(world, order, Deal::Fetching(0));
    }
}

fn ask(world: &mut World, order: &Order, call: Call) {
    let server = &world.resource::<Market>().server;
    let request = match &call {
        Call::Purchase(id) => server.purchase(id),
        Call::Paying(purchase) => server.purchase_state(purchase),
        Call::Part(id, from) => {
            server.package_part(id, &order.version, *from, PART_BYTES)
        }
    };
    send(world, crate::market::Call::Deal(call), request);
}

/// Moves the deal going on by `event`, and does what it asks.
fn step(world: &mut World, event: Event) {
    let Some((order, deal)) =
        world.resource_mut::<Dealing>().0.take()
    else {
        return;
    };
    let (deal, act) = deal.next(&order, event, now());
    let ended = !deal.going();
    let done = matches!(deal, Deal::Done(_));
    world.resource_mut::<Dealing>().0 = Some((order.clone(), deal));
    match act {
        Some(Act::Download(from)) => {
            ask(world, &order, Call::Part(order.id.clone(), from))
        }
        Some(Act::Poll(purchase)) => {
            ask(world, &order, Call::Paying(purchase))
        }
        Some(Act::Open(url)) => browse(world, &url),
        Some(Act::Settle { part, parts, bytes }) => {
            if let Some(reveal) = world.get_resource::<Reveal>() {
                reveal.spend(bytes.len());
            }
            world.ask(AgentsRequest::Settle(Delivery {
                listing: order.id.clone(),
                title: order.title.clone(),
                version: order.version.clone(),
                package: bytes,
                part,
                parts,
                to: order.to.clone(),
            }))
        }
        None => {}
    }
    if ended && done {
        settled(world, &order);
    }
}

/// Takes in what was written into the home, and shows it.
fn settled(world: &mut World, order: &Order) {
    world.ask(AgentsRequest::Bought);
    world.traded(stall_of(order.kind));
    ask_account(world);
    if order.kind == Kind::Hamlet
        && let Some(name) = world
            .resource::<Market>()
            .lot(&order.id)
            .map(|lot| {
                lot.meta::<crate::market::lot::HamletMeta>().name
            })
            .filter(|name| !name.is_empty())
    {
        hamlets::visit(world, &name);
    }
}

fn tick(world: &mut World) {
    let now = now();
    // Read first, so that the views are not looked at again for
    // nothing.
    let due = matches!(
        &world.resource::<Dealing>().0,
        Some((_, Deal::Paying { asking, .. }))
            if asking.over(now) || asking.due(now)
    );
    if due {
        step(world, Event::Tick(now));
    }
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    reply: Result<Reply, Failure>,
) {
    let mine = world.resource::<Dealing>().0.as_ref().is_some_and(
        |(order, deal)| {
            deal.going()
                && match &call {
                    Call::Purchase(id) | Call::Part(id, _) => {
                        order.id == *id
                    }
                    Call::Paying(_) => true,
                }
        },
    );
    if !mine {
        return;
    }
    let event = match call {
        Call::Purchase(_) => Event::Bought(read::<Checkout>(reply)),
        Call::Paying(_) => Event::Paid(
            read::<Purchase>(reply).map(|state| state.status),
        ),
        Call::Part(..) => Event::Part(bytes(reply)),
    };
    step(world, event);
}

/// Takes in what came of settling a package.
pub(super) fn heard(world: &mut World, told: &Heard) {
    let event = match told {
        Heard::Reply(
            Some(AgentsRequest::Settle(delivery)),
            AgentsReply::Done,
        ) => Some((delivery, Event::Stored)),
        Heard::Reply(
            Some(AgentsRequest::Settle(delivery)),
            AgentsReply::Settled(words),
        ) => Some((delivery, Event::Settled(Ok(words.clone())))),
        Heard::Failed(Some(AgentsRequest::Settle(delivery)), why) => {
            Some((
                delivery,
                Event::Settled(Err(format!(
                    "It could not be set up: {}",
                    why.plain()
                ))),
            ))
        }
        _ => None,
    };
    let Some((delivery, event)) = event else {
        return;
    };
    // Only what came of the part just handed over moves the deal.
    let end = u64::from(delivery.part) * PART_BYTES
        + delivery.package.len() as u64;
    let ours = world.resource::<Dealing>().0.as_ref().is_some_and(
        |(order, deal)| {
            order.id == delivery.listing
                && *deal == Deal::Settling(end)
        },
    );
    if ours {
        step(world, event);
    }
}
