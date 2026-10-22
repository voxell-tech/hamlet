//! The bazaar's server, hamlet-server, where chiefs buy and sell
//! agents, hamlets and tools: who is signed in with GitHub, what is
//! on its shelves, what the chief owns, sells and has written into
//! the home, and the line its fetches wait in. Where it cannot be
//! reached, the stalls show the samples.

mod account;
pub(crate) mod client;
mod deal;
pub(crate) mod lot;
pub(crate) mod sell;
mod session;

use std::collections::{BTreeMap, VecDeque};

use agents_ui::{AgentsWorldExt, Heard};
use hamlet_engine_bevy::api::agents::{
    AgentsReply, AgentsRequest, Bought,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Failure, FetchRequest, Reply};

use crate::errand::{Errand, ErrandWorldExt};
use crate::listings::{
    AgentWares, Agents, HamletWares, Hamlets, Sells, ToolWares,
    Tools, Wares,
};
pub(crate) use crate::market::account::{
    Account, ask_account, browse, set_up_payouts, sign_in, sign_out,
};
use crate::market::client::{Server, read};
pub(crate) use crate::market::deal::{
    Deal, Dealing, Places, buy, install, targets, update,
};
use crate::market::lot::{Detail, Kind, Lot, Page};
pub(crate) use crate::market::sell::{
    Housed, Offer, Selling, ask_tools, sell, shelve,
};
use crate::shelf::{Stock, now};

/// The most fetches of the bazaar's server under way at once, leaving
/// the rest of the engine's to the other stalls.
const AT_ONCE: usize = 2;

/// The wait before a fetch the engine held back is sent again, in
/// milliseconds.
const HELD_BACK: u64 = 5_000;

/// The most pages of listings asked for.
const PAGES: u32 = 5;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        crate::shelf::plugin::<AgentWares>,
        crate::shelf::plugin::<ToolWares>,
        crate::shelf::plugin::<HamletWares>,
        account::plugin,
        deal::plugin,
        sell::plugin,
        session::plugin,
    ))
    .insert_resource(Market::new(client::server_of(
        std::env::var(client::SERVER_VARIABLE).ok(),
    )))
    .init_resource::<Line>()
    .add_systems(Startup, |world: &mut World| {
        world.ask(AgentsRequest::Bought);
    })
    .add_systems(Update, pump.run_if(|line: Res<Line>| line.due()));
}

/// The bazaar's server and all the chief does there.
#[derive(Resource)]
pub(crate) struct Market {
    pub server: Server,
    pub account: Account,
    /// How asking for the shelves went.
    pub reach: Reach,
    /// Every listing on the shelves, as they last came.
    pub lots: Vec<Lot>,
    /// The listings on their own pages, by id.
    pub details: BTreeMap<String, Detail>,
    /// What the one signed in owns.
    pub library: Vec<Lot>,
    /// What the one signed in sells, in every state.
    pub mine: Vec<Lot>,
    /// What was bought and written into the home, by listing.
    pub bought: BTreeMap<String, Bought>,
    /// What came of the last thing done with the account or the
    /// listings sold, and whether it went wrong.
    pub said: Option<(String, bool)>,
    /// The listings are being asked for.
    asking: bool,
    /// The pages of listings come so far, while more are asked for.
    paging: Vec<Lot>,
}

/// How asking the bazaar's server for its shelves went. Once its
/// listings came, the stalls show them, and asking again, or failing
/// to, is said over them.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) enum Reach {
    #[default]
    Unasked,
    /// Asked for the first time, or again after it failed.
    Asking,
    /// Its listings came.
    Reached,
    /// It could not be asked, for this reason.
    Unreached(String),
}

impl Market {
    fn new(base: String) -> Self {
        Market {
            server: Server { base, token: None },
            account: Account::Out,
            reach: Reach::Unasked,
            lots: Vec::new(),
            details: BTreeMap::new(),
            library: Vec::new(),
            mine: Vec::new(),
            bought: BTreeMap::new(),
            said: None,
            asking: false,
            paging: Vec::new(),
        }
    }

    /// The one signed in, if anyone is.
    pub fn user(&self) -> Option<&lot::User> {
        match &self.account {
            Account::In(user) => Some(user),
            _ => None,
        }
    }

    pub fn signed_in(&self) -> bool {
        self.server.token.is_some()
    }

    /// The listing `id`, wherever it was seen.
    pub fn lot(&self, id: &str) -> Option<&Lot> {
        self.details
            .get(id)
            .map(|detail| &detail.listing)
            .into_iter()
            .chain(&self.lots)
            .chain(&self.library)
            .chain(&self.mine)
            .find(|lot| lot.id == id)
    }

    pub fn owns(&self, id: &str) -> bool {
        self.library.iter().any(|lot| lot.id == id)
            || self.details.get(id).is_some_and(|detail| detail.owned)
    }

    pub fn sells(&self, id: &str) -> bool {
        self.mine.iter().any(|lot| lot.id == id)
    }

    fn say(&mut self, words: impl Into<String>, wrong: bool) {
        self.said = Some((words.into(), wrong));
    }
}

/// What a fetch of the bazaar's server is for.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    /// The listings on the shelves from this place on.
    Shelves(u32),
    Detail(String),
    Account(account::Call),
    Deal(deal::Call),
    Sell(sell::Call),
}

/// The fetches of the bazaar's server waiting to be sent, and how
/// many are under way.
#[derive(Resource, Default)]
pub(crate) struct Line {
    waiting: VecDeque<(Call, FetchRequest)>,
    out: usize,
    /// Nothing is sent before then, in milliseconds since 1970,
    /// after the engine held a fetch back.
    not_before: u64,
}

impl Line {
    /// True when a fetch waits and may be sent.
    fn due(&self) -> bool {
        !self.waiting.is_empty()
            && self.out < AT_ONCE
            && now() >= self.not_before
    }
}

/// Asks the bazaar's server `request`, for `call`, once the line
/// lets it.
pub(crate) fn send(
    world: &mut World,
    call: Call,
    request: FetchRequest,
) {
    world
        .resource_mut::<Line>()
        .waiting
        .push_back((call, request));
    pump(world);
}

/// Sends what waits in the line, as many as may be under way.
fn pump(world: &mut World) {
    while world.resource::<Line>().due() {
        let Some((call, request)) =
            world.resource_mut::<Line>().waiting.pop_front()
        else {
            return;
        };
        let errand =
            Errand::Market(Box::new((call.clone(), request.clone())));
        if world.send_request(&request, errand) {
            world.resource_mut::<Line>().out += 1;
        } else {
            // Not taken in this call: it goes first in the next.
            let mut line = world.resource_mut::<Line>();
            line.waiting.push_front((call, request));
            line.not_before = now() + HELD_BACK;
            return;
        }
    }
}

/// Takes in the answer to `call`, and sends what waits after it.
pub(crate) fn answered(
    world: &mut World,
    call: Call,
    request: FetchRequest,
    reply: Result<Reply, Failure>,
) {
    {
        let mut line = world.resource_mut::<Line>();
        line.out = line.out.saturating_sub(1);
        if matches!(reply, Err(Failure::OutOfBudget(_))) {
            line.waiting.push_front((call, request));
            line.not_before = now() + HELD_BACK;
            return;
        }
    }
    match call {
        Call::Shelves(offset) => shelved(world, offset, reply),
        Call::Detail(id) => detailed(world, id, reply),
        Call::Account(call) => account::answered(world, call, reply),
        Call::Deal(call) => deal::answered(world, call, reply),
        Call::Sell(call) => sell::answered(world, call, reply),
    }
    pump(world);
}

/// Asks for the listings on the shelves, unless they are being asked
/// for.
pub(crate) fn ask_shelves(world: &mut World) {
    let mut market = world.resource_mut::<Market>();
    if market.asking {
        return;
    }
    market.asking = true;
    if market.reach != Reach::Reached {
        market.reach = Reach::Asking;
    }
    market.paging.clear();
    let request = market.server.listings(0);
    asking::<Agents>(world);
    asking::<Tools>(world);
    asking::<Hamlets>(world);
    send(world, Call::Shelves(0), request);
}

/// Asks for the shelves the first time a stall of the bazaar's is
/// shown.
pub(crate) fn stocked(world: &mut World) {
    if world.resource::<Market>().reach == Reach::Unasked {
        ask_shelves(world);
    }
}

fn asking<K: Sells>(world: &mut World) {
    world.resource_mut::<Stock<Wares<K>>>().ask();
}

fn shelved(
    world: &mut World,
    offset: u32,
    reply: Result<Reply, Failure>,
) {
    let page = read::<Page<Lot>>(reply);
    let mut market = world.resource_mut::<Market>();
    let page = match page {
        Ok(page) => page,
        Err(why) => {
            market.asking = false;
            if market.reach != Reach::Reached {
                market.reach = Reach::Unreached(why.words());
            }
            market.paging.clear();
            return restock(world, Err(why.words()));
        }
    };
    let last = page.items.len() < client::PAGE as usize;
    market.paging.extend(page.items);
    let next = offset + client::PAGE;
    let total = page.total.unwrap_or(0) as usize;
    if !last
        && market.paging.len() < total
        && next / client::PAGE < PAGES
    {
        let request = market.server.listings(next);
        return send(world, Call::Shelves(next), request);
    }
    market.lots = std::mem::take(&mut market.paging);
    market.reach = Reach::Reached;
    market.asking = false;
    restock(world, Ok(()));
}

/// Puts what is on the market's shelves on each stall's, or tells
/// each why nothing came.
fn restock(world: &mut World, came: Result<(), String>) {
    restock_of::<Agents>(world, &came);
    restock_of::<Tools>(world, &came);
    restock_of::<Hamlets>(world, &came);
}

fn restock_of<K: Sells>(
    world: &mut World,
    came: &Result<(), String>,
) {
    let items = came.clone().map(|()| {
        world
            .resource::<Market>()
            .lots
            .iter()
            .filter(|lot| lot.kind == K::KIND)
            .cloned()
            .map(Wares::new)
            .collect()
    });
    world.resource_mut::<Stock<Wares<K>>>().came(items, now());
}

/// Asks for all of the listing `id`, for its own page.
pub(crate) fn ask_detail(world: &mut World, id: &str) {
    let request = world.resource::<Market>().server.detail(id);
    send(world, Call::Detail(id.to_string()), request);
}

fn detailed(
    world: &mut World,
    id: String,
    reply: Result<Reply, Failure>,
) {
    // Without it, the page shows what the shelves said.
    if let Ok(detail) = read::<Detail>(reply) {
        world.resource_mut::<Market>().details.insert(id, detail);
    }
}

/// Takes in what the agents service says of what was bought, and
/// what came of writing a purchase into the home.
pub(crate) fn heard(world: &mut World, told: &Heard) {
    if let Heard::Reply(_, AgentsReply::Bought(bought)) = told
        && world.resource::<Market>().bought != *bought
    {
        world.resource_mut::<Market>().bought = bought.clone();
    }
    deal::heard(world, told);
    sell::heard(world, told);
}

/// The kind of what a stall of the bazaar's sells.
pub(crate) fn stall_of(kind: Kind) -> crate::Stall {
    match kind {
        Kind::Agent => crate::Stall::Community,
        Kind::Plugin => crate::Stall::Plugins,
        Kind::Hamlet => crate::Stall::Hamlets,
    }
}

#[cfg(test)]
mod tests;
