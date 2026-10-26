//! The bazaar's server, where chiefs buy and sell
//! agents, hamlets and tools: who is signed in with their Hamlet
//! account, what is on its shelves, what the chief owns, sells and
//! has settled into the home. Where it cannot be reached, the stalls
//! show the samples.

mod account;
pub(crate) mod check;
pub(crate) mod client;
pub(crate) mod deal;
#[cfg(test)]
mod live;
pub(crate) mod lot;
pub(crate) mod sell;
mod session;
#[cfg(test)]
mod tests;
mod vault;

use std::collections::BTreeMap;

use bevy::app::{App, Startup};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;

pub(crate) use self::account::{
    Account, agree, ask_account, browse, change_password,
    check_payouts, choose_username, disagree, forgot, read_terms,
    resend, reset, set_up_payouts, sign_in, sign_out, sign_up,
    verify,
};
pub(crate) use self::check::{
    Reporting, ask_human, report, write_report,
};
use self::client::{Request, Server, read};
use self::deal::Dealing;
pub(crate) use self::deal::{Deal, buy, install, update};
use self::lot::{Detail, Kind, Lot, Page};
pub(crate) use self::sell::{
    Housed, Offer, Selling, ask_tools, sell, shelve,
};
pub(crate) use self::vault::{disconnect, home_has_key};
use crate::bazaar::errand::{self, Errand, Fetched};
use crate::bazaar::listings::{Agents, Hamlets, Sells, Tools, Wares};
use crate::bazaar::shelf::{self, Stock, now};
use crate::bazaar::{Stall, wardrobe};

/// The most pages of listings asked for.
const PAGES: u32 = 5;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        shelf::plugin::<Wares<Agents>>,
        shelf::plugin::<Wares<Tools>>,
        shelf::plugin::<Wares<Hamlets>>,
        deal::plugin,
        sell::plugin,
        vault::plugin,
        check::plugin,
    ))
    .insert_resource(Market::new(client::server_of(
        std::env::var(client::SERVER_VARIABLE).ok(),
    )))
    .add_systems(Startup, session::restore)
    .add_observer(answered_by_home);
}

/// The bazaar's server and all the chief does there.
#[derive(Resource)]
pub(crate) struct Market {
    pub server: Server,
    pub account: Account,
    /// The data key of the vault of the one signed in, once it is
    /// opened.
    pub data_key: Option<hamlet_protocol::vault::DataKey>,
    /// The seller terms, shown to be agreed to.
    pub terms: Option<lot::Terms>,
    /// Payouts are set up once the seller terms are agreed to.
    pub paying: bool,
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
    /// What was bought and settled into the home, by listing.
    pub bought: BTreeMap<String, protocol::Bought>,
    /// What came of the last thing done with the account or the
    /// listings sold, and whether it went wrong.
    pub said: Option<(String, bool)>,
    /// The listings are being asked for.
    asking: bool,
    /// The pages of listings come so far, while more are asked for.
    paging: Vec<Lot>,
    /// The read of what was bought, while it is asked.
    reading: Option<protocol::RequestId>,
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
    pub(crate) fn new(base: String) -> Self {
        Market {
            server: Server { base, token: None },
            account: Account::Out,
            data_key: None,
            terms: None,
            paying: false,
            reach: Reach::Unasked,
            lots: Vec::new(),
            details: BTreeMap::new(),
            library: Vec::new(),
            mine: Vec::new(),
            bought: BTreeMap::new(),
            said: None,
            asking: false,
            paging: Vec::new(),
            reading: None,
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

/// What a request of the bazaar's server is for.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    /// The listings on the shelves from this place on.
    Shelves(u32),
    Detail(String),
    Account(account::Call),
    Vault(vault::Call),
    Deal(deal::Call),
    Sell(sell::Call),
    Check(check::Call),
    /// A listing's cover, or an asset's picture, at its address on
    /// the server.
    Picture(String),
    /// A fetch of the LOOKS stall's.
    Wardrobe(wardrobe::Call),
}

/// Asks the bazaar's server `request`, for `call`. Where the window
/// has no runtime to ask on, it is answered at once that the bazaar
/// cannot be reached.
pub(crate) fn send(world: &mut World, call: Call, request: Request) {
    if !errand::send_request(
        world,
        request,
        Errand::Market(call.clone()),
    ) {
        answered(
            world,
            call,
            Err("nothing here can reach it".into()),
        );
    }
}

/// Takes in the answer to `call`.
pub(crate) fn answered(
    world: &mut World,
    call: Call,
    answer: Fetched,
) {
    match call {
        Call::Shelves(offset) => shelved(world, offset, answer),
        Call::Detail(id) => detailed(world, id, answer),
        Call::Account(call) => account::answered(world, call, answer),
        Call::Vault(call) => vault::answered(world, call, answer),
        Call::Deal(call) => deal::answered(world, call, answer),
        Call::Sell(call) => sell::answered(world, call, answer),
        Call::Check(call) => check::answered(world, call, answer),
        Call::Picture(url) => {
            crate::bazaar::covers::came(world, url, answer);
        }
        Call::Wardrobe(call) => {
            wardrobe::answered(world, call, answer);
        }
    }
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

/// Asks for the shelves, and what was bought, the first time a stall
/// of the bazaar's is shown.
pub(crate) fn stocked(world: &mut World) {
    if world.resource::<Market>().reach == Reach::Unasked {
        ask_shelves(world);
        ask_bought(world);
    }
}

fn asking<K: Sells>(world: &mut World) {
    world.resource_mut::<Stock<Wares<K>>>().ask();
}

fn shelved(world: &mut World, offset: u32, answer: Fetched) {
    let page = read::<Page<Lot>>(answer);
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
pub(crate) fn restock(world: &mut World, came: Result<(), String>) {
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

fn detailed(world: &mut World, id: String, answer: Fetched) {
    // Without it, the page shows what the shelves said.
    if let Ok(detail) = read::<Detail>(answer) {
        world.resource_mut::<Market>().details.insert(id, detail);
    }
}

/// Asks the server what was bought and settled into the home.
pub(crate) fn ask_bought(world: &mut World) {
    let request = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Bought { id })
    });
    world.resource_mut::<Market>().reading = Some(request);
}

/// Takes in what the server says of what was bought, of a package
/// settled and of one packed.
fn answered_by_home(response: On<Response>, mut commands: Commands) {
    let response = response.event().clone();
    commands.queue(move |world: &mut World| {
        if let protocol::ServerMessage::Read(
            protocol::ReadResult::Bought { bought, to },
        ) = &response.message
            && world.resource::<Market>().reading.as_ref() == Some(to)
        {
            let bought = bought
                .iter()
                .map(|one| (one.listing.clone(), one.clone()))
                .collect::<BTreeMap<_, _>>();
            let mut market = world.resource_mut::<Market>();
            market.reading = None;
            if market.bought != bought {
                market.bought = bought;
            }
            return;
        }
        deal::heard(world, &response);
        sell::heard(world, &response);
        vault::heard(world, &response);
        wardrobe::heard(world, &response);
    });
}

/// The stall that sells listings of `kind`.
pub(crate) fn stall_of(kind: Kind) -> Stall {
    match kind {
        Kind::Agent => Stall::Community,
        Kind::Plugin => Stall::Plugins,
        Kind::Hamlet => Stall::Hamlets,
    }
}

/// The words of a refusal of the home's server, if `message` is one.
pub(crate) fn refused(
    message: &protocol::ServerMessage,
) -> Option<String> {
    match message {
        protocol::ServerMessage::Fault(fault) => {
            Some(fault.text.clone())
        }
        protocol::ServerMessage::Reply(reply)
            if reply.kind == protocol::ReplyKind::Error =>
        {
            Some(reply.text.clone())
        }
        _ => None,
    }
}

/// A deal held at one of its steps, for a still.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dealt {
    /// Waiting for Stripe to say it was paid.
    Paying,
    /// Its package is set up in the home.
    Installing,
}

/// The bazaar's server as a still shows it, its answers given by
/// hand, so nothing is asked of the network.
#[derive(Clone, Default)]
pub struct Staged {
    /// A page of listings, as `GET /v1/listings` answers.
    pub listings: serde_json::Value,
    /// The one signed in, as `GET /v1/me` answers; none when nobody
    /// is.
    pub user: Option<serde_json::Value>,
    /// The listings the one signed in sells and owns, by id.
    pub mine: Vec<String>,
    pub owned: Vec<String>,
    /// What was settled into the home.
    pub bought: Vec<protocol::Bought>,
    /// Covers, by their address on the server, as the bytes of a
    /// picture.
    pub covers: Vec<(String, Vec<u8>)>,
    /// A deal held at a step, of the listing of this id.
    pub deal: Option<(String, Dealt)>,
    /// The LOOKS stall's catalogue, as `GET /v1/assets` answers.
    pub assets: serde_json::Value,
    /// The official assets the one signed in owns, by id.
    pub owned_assets: Vec<String>,
    /// The files of official assets, by id, as they are viewed once
    /// unveiled.
    pub views: Vec<(String, Vec<u8>)>,
    /// The seller terms shown to be agreed to, as
    /// `GET /v1/seller-terms` answers.
    pub terms: Option<serde_json::Value>,
}

/// Puts the bazaar's server in the state `staged` gives.
pub fn stage(world: &mut World, staged: Staged) {
    let lots = serde_json::from_value::<Page<Lot>>(staged.listings)
        .map(|page| page.items)
        .unwrap_or_default();
    let pick = |ids: &[String]| {
        lots.iter()
            .filter(|lot| ids.contains(&lot.id))
            .cloned()
            .collect::<Vec<_>>()
    };
    let (mine, library) = (pick(&staged.mine), pick(&staged.owned));
    let user = staged.user.and_then(|user| {
        serde_json::from_value::<lot::User>(user).ok()
    });
    let deal = staged.deal.and_then(|(id, dealt)| {
        let lot = lots.iter().find(|lot| lot.id == id)?;
        let order = deal::Order {
            id,
            kind: lot.kind,
            title: lot.title.clone(),
            version: lot.version().to_string(),
            size: lot.latest.as_ref().map_or(0, |latest| latest.size),
            into: None,
        };
        let step = match dealt {
            Dealt::Paying => Deal::Paying {
                url: "https://checkout.stripe.com".into(),
                purchase: "p_still".into(),
                asking: account::Asking::new(
                    now(),
                    u64::MAX / 4,
                    u64::MAX / 4,
                ),
            },
            Dealt::Installing => Deal::Settling,
        };
        Some((order, step))
    });
    {
        let mut market = world.resource_mut::<Market>();
        market.reach = Reach::Reached;
        market.lots = lots;
        market.mine = mine;
        market.library = library;
        market.bought = staged
            .bought
            .into_iter()
            .map(|one| (one.listing.clone(), one))
            .collect();
        market.server.token =
            user.as_ref().map(|_| "still-token".into());
        market.account = match user {
            Some(user) => Account::In(user),
            None => Account::Out,
        };
        market.terms = staged
            .terms
            .and_then(|terms| serde_json::from_value(terms).ok());
    }
    if let Some(deal) = deal {
        world.resource_mut::<Dealing>().deal = Some(deal);
    }
    for (url, bytes) in staged.covers {
        crate::bazaar::covers::came(world, url, Ok((200, bytes)));
    }
    restock(world, Ok(()));
    wardrobe::viewing::stage(world, staged.views);
    if !staged.assets.is_null() {
        wardrobe::stage(world, staged.assets, &staged.owned_assets);
    }
}
