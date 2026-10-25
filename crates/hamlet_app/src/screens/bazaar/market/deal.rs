//! Buying a listing and settling it into the home: paid for on
//! Stripe's page when it has a price, its package downloaded whole
//! from the bazaar's server and handed to the home's server in parts,
//! which settles it where the chief chose, and brought up to a newer
//! version when they ask. One deal goes at a time.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bevy::app::{App, Update};
use bevy::ecs::name::Name;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use bevy::ecs::world::World;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{HOME, HamletId};
use hamlet_protocol::client as protocol;
use sha2::{Digest, Sha256};

use crate::bazaar::errand::Fetched;
use crate::bazaar::hamlets::{self, full};
use crate::bazaar::market::account::{Asking, browse};
use crate::bazaar::market::client::{Refusal, bytes, read};
use crate::bazaar::market::lot::{
    Checkout, HamletMeta, Kind, Paying, Purchase,
};
use crate::bazaar::market::{
    Market, ask_account, ask_bought, refused, send,
};
use crate::bazaar::shelf::now;
use crate::bazaar::traded;

/// The time between asks whether a payment went through.
pub(crate) const PAYMENT_POLL: u64 = 5_000;

/// The longest a payment is waited for.
pub(crate) const PAYMENT_PATIENCE: u64 = 30 * 60 * 1000;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Dealing>()
        .init_resource::<Places>()
        .add_systems(
            Update,
            tick.run_if(|dealing: Res<Dealing>| {
                matches!(dealing.deal, Some((_, Deal::Paying { .. })))
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
    /// The bytes of its package, as the bazaar says.
    pub size: u64,
    /// The hamlet an agent moves into.
    pub into: Option<String>,
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
    /// Its package is downloaded.
    Fetching,
    /// The home's server takes its package and settles it.
    Settling,
    Done(String),
    Failed(String),
}

impl Deal {
    /// True while it goes on.
    pub fn going(&self) -> bool {
        !matches!(self, Deal::Done(_) | Deal::Failed(_))
    }
}

/// The deal going on, or the last one, and the requests of the home
/// that settle its package.
#[derive(Resource, Default)]
pub(crate) struct Dealing {
    pub deal: Option<(Order, Deal)>,
    /// The `settle`, then the last of its parts, whose answer is the
    /// deal's.
    waiting: Option<protocol::RequestId>,
    /// The package, from its download until it is handed over.
    package: Vec<u8>,
}

/// What happened to a deal.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    Bought(Result<Checkout, Refusal>),
    Paid(Result<Paying, Refusal>),
    /// Its package came, as many bytes as this, or why it did not.
    Package(Result<u64, Refusal>),
    /// The home's server settled it, or refused to.
    Settled(Result<String, String>),
    /// The time now.
    Tick(u64),
}

/// What a deal asks to be done next.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    /// Download its package.
    Download,
    /// Ask whether this purchase was paid.
    Poll(String),
    /// Open this page in the browser.
    Open(String),
    /// Hand the package to the home's server.
    Settle,
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
                        (Deal::Fetching, Some(Act::Download))
                    }
                    Paid::Going(deal, act) => (deal, act),
                }
            }
            (Deal::Fetching, Event::Package(Ok(size))) => {
                if size == 0 || (order.size > 0 && size != order.size)
                {
                    return failed(format!(
                        "The package came wrong: {size} bytes, \
                         where {} were due.",
                        order.size
                    ));
                }
                (Deal::Settling, Some(Act::Settle))
            }
            (Deal::Fetching, Event::Package(Err(why))) => {
                failed(format!(
                    "The package could not be had: {}",
                    why.words()
                ))
            }
            (Deal::Settling, Event::Settled(Ok(words))) => {
                (Deal::Done(words), None)
            }
            (Deal::Settling, Event::Settled(Err(why))) => {
                failed(format!("It could not be set up: {why}"))
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
/// page when it has a price. The market's deals and the LOOKS stall's
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
    /// The package of the listing.
    Package(String),
}

/// The hamlet the chief chose for an agent to move into, by its place
/// among the hamlets.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Places {
    pub hamlet: usize,
}

/// The hamlets of the realm, home first, by id and the name each is
/// shown by.
pub(crate) fn hamlets_of(world: &World) -> Vec<(String, String)> {
    let Some(mut query) =
        world.try_query::<(&HamletId, Option<&Name>)>()
    else {
        return Vec::new();
    };
    let mut all = query
        .iter(world)
        .map(|(id, name)| {
            let shown = name
                .map(|name| name.as_str().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| id.0.clone());
            (id.0.clone(), shown)
        })
        .collect::<Vec<_>>();
    all.sort_by(|a, b| (a.0 != HOME, &a.0).cmp(&(b.0 != HOME, &b.0)));
    all
}

/// Where a listing of `kind` can go, in words, and the hamlet an
/// agent moves into.
pub(crate) fn targets(
    world: &World,
    kind: Kind,
) -> Vec<(String, Option<String>)> {
    match kind {
        Kind::Hamlet => vec![("the realm".into(), None)],
        Kind::Plugin => vec![("every agent".into(), None)],
        Kind::Agent => hamlets_of(world)
            .into_iter()
            .map(|(id, shown)| (format!("into {shown}"), Some(id)))
            .collect(),
    }
}

impl Places {
    /// The place chosen among `count` for a listing of `kind`.
    pub fn at(&self, kind: Kind, count: usize) -> usize {
        let at = match kind {
            Kind::Agent => self.hamlet,
            _ => 0,
        };
        at.min(count.saturating_sub(1))
    }
}

/// The place chosen for a listing of `kind`.
fn chosen(world: &World, kind: Kind) -> Option<Option<String>> {
    let all = targets(world, kind);
    let at = world.resource::<Places>().at(kind, all.len());
    all.into_iter().nth(at).map(|(_, to)| to)
}

fn going(world: &World) -> bool {
    world
        .resource::<Dealing>()
        .deal
        .as_ref()
        .is_some_and(|(_, deal)| deal.going())
}

/// The order of the newest version of the listing `id`, into `into`.
fn order(
    world: &World,
    id: &str,
    into: Option<String>,
) -> Option<Order> {
    let lot = world.resource::<Market>().lot(id)?;
    Some(Order {
        id: id.to_string(),
        kind: lot.kind,
        title: lot.title.clone(),
        version: lot.version().to_string(),
        size: lot.latest.as_ref().map_or(0, |latest| latest.size),
        into,
    })
}

/// Why `order` cannot be had now, if it cannot.
fn refusal(world: &World, order: &Order) -> Option<String> {
    if !world.resource::<Market>().signed_in() {
        Some("Sign in with GitHub to buy.".into())
    } else if order.version.is_empty() {
        Some("It has no version to download yet.".into())
    } else if order.kind == Kind::Hamlet
        && !world.resource::<Market>().bought.contains_key(&order.id)
    {
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
    let call = match &deal {
        Deal::Buying => Some(Call::Purchase(order.id.clone())),
        Deal::Fetching => Some(Call::Package(order.id.clone())),
        _ => None,
    };
    let mut dealing = world.resource_mut::<Dealing>();
    dealing.deal = Some((order.clone(), deal));
    dealing.waiting = None;
    dealing.package = Vec::new();
    if let Some(call) = call {
        ask(world, &order, call);
    }
}

/// Buys the listing `id`, then settles its newest version where the
/// chief chose.
pub(crate) fn buy(world: &mut World, id: &str) {
    let Some(kind) =
        world.resource::<Market>().lot(id).map(|lot| lot.kind)
    else {
        return;
    };
    let Some(into) = chosen(world, kind) else {
        return;
    };
    if let Some(order) = order(world, id, into) {
        start(world, order, Deal::Buying);
    }
}

/// Settles the newest version of the listing `id`, owned, where the
/// chief chose.
pub(crate) fn install(world: &mut World, id: &str) {
    let Some(kind) =
        world.resource::<Market>().lot(id).map(|lot| lot.kind)
    else {
        return;
    };
    let Some(into) = chosen(world, kind) else {
        return;
    };
    if let Some(order) = order(world, id, into) {
        start(world, order, Deal::Fetching);
    }
}

/// Settles the newest version of the listing `id` over the one
/// settled before, where that went.
pub(crate) fn update(world: &mut World, id: &str) {
    let Some(bought) =
        world.resource::<Market>().bought.get(id).cloned()
    else {
        return;
    };
    let into = match bought.kind {
        protocol::PackageKind::Agent => bought.hamlet.clone(),
        _ => None,
    };
    if let Some(order) = order(world, id, into) {
        start(world, order, Deal::Fetching);
    }
}

fn ask(world: &mut World, order: &Order, call: Call) {
    let server = &world.resource::<Market>().server;
    let request = match &call {
        Call::Purchase(id) => server.purchase(id),
        Call::Paying(purchase) => server.purchase_state(purchase),
        Call::Package(id) => server.package(id, &order.version),
    };
    send(world, crate::bazaar::market::Call::Deal(call), request);
}

/// Moves the deal going on by `event`, and does what it asks.
fn step(world: &mut World, event: Event) {
    let Some((order, deal)) =
        world.resource_mut::<Dealing>().deal.take()
    else {
        return;
    };
    let (deal, act) = deal.next(&order, event, now());
    let done = matches!(deal, Deal::Done(_));
    if !deal.going() {
        world.resource_mut::<Dealing>().package = Vec::new();
    }
    world.resource_mut::<Dealing>().deal =
        Some((order.clone(), deal));
    match act {
        Some(Act::Download) => {
            ask(world, &order, Call::Package(order.id.clone()));
        }
        Some(Act::Poll(purchase)) => {
            ask(world, &order, Call::Paying(purchase));
        }
        Some(Act::Open(url)) => browse(world, &url),
        Some(Act::Settle) => settle(world, &order),
        None => {}
    }
    if done {
        settled(world, &order);
    }
}

/// Asks the home's server to settle the package downloaded.
fn settle(world: &mut World, order: &Order) {
    let package = &world.resource::<Dealing>().package;
    let settle = |id| protocol::Settle {
        id,
        into: order.into.clone(),
        listing: order.id.clone(),
        sha256: format!("{:x}", Sha256::digest(package)),
        size: package.len() as u64,
        title: order.title.clone(),
        version: order.version.clone(),
    };
    let settle = settle(protocol::RequestId::new());
    let request = settle.id.clone();
    world
        .resource_mut::<Outbox>()
        .send(|_| protocol::ClientMessage::Settle(settle));
    world.resource_mut::<Dealing>().waiting = Some(request);
}

/// Sends the package in parts to `upload`, the last answering for
/// the deal.
fn hand_over(world: &mut World, upload: String) {
    let package =
        std::mem::take(&mut world.resource_mut::<Dealing>().package);
    let mut last = None;
    let mut outbox = world.resource_mut::<Outbox>();
    for (at, part) in
        package.chunks(protocol::PACKAGE_PART).enumerate()
    {
        let offset = (at * protocol::PACKAGE_PART) as u64;
        last = Some(outbox.send(|id| {
            protocol::ClientMessage::SettlePart(
                protocol::SettlePart {
                    data: STANDARD.encode(part),
                    id,
                    offset,
                    upload: upload.clone(),
                },
            )
        }));
    }
    world.resource_mut::<Dealing>().waiting = last;
}

/// Takes in what was settled into the home, and shows it.
fn settled(world: &mut World, order: &Order) {
    ask_bought(world);
    traded(world);
    ask_account(world);
    if order.kind == Kind::Hamlet
        && let Some(name) = world
            .resource::<Market>()
            .lot(&order.id)
            .map(|lot| lot.meta::<HamletMeta>().name)
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
        &world.resource::<Dealing>().deal,
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
    answer: Fetched,
) {
    let mine = world.resource::<Dealing>().deal.as_ref().is_some_and(
        |(order, deal)| {
            deal.going()
                && match &call {
                    Call::Purchase(id) | Call::Package(id) => {
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
        Call::Purchase(_) => Event::Bought(read::<Checkout>(answer)),
        Call::Paying(_) => Event::Paid(
            read::<Purchase>(answer).map(|state| state.status),
        ),
        Call::Package(_) => {
            Event::Package(bytes(answer).map(|package| {
                let size = package.len() as u64;
                world.resource_mut::<Dealing>().package = package;
                size
            }))
        }
    };
    step(world, event);
}

/// Takes in what the home's server said of the package handed to it.
pub(super) fn heard(world: &mut World, response: &Response) {
    let dealing = world.resource::<Dealing>();
    if dealing.waiting.as_ref() != Some(&response.to)
        || !matches!(dealing.deal, Some((_, Deal::Settling)))
    {
        return;
    }
    if let Some(why) = refused(&response.message) {
        world.resource_mut::<Dealing>().waiting = None;
        return step(world, Event::Settled(Err(why)));
    }
    let protocol::ServerMessage::Reply(reply) = &response.message
    else {
        return;
    };
    match &reply.upload {
        Some(upload) => hand_over(world, upload.clone()),
        None => {
            world.resource_mut::<Dealing>().waiting = None;
            step(world, Event::Settled(Ok(reply.text.clone())));
        }
    }
}
