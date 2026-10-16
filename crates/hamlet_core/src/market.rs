//! The bazaar's server as a resource: the chief signed in with
//! GitHub, the listings on its shelves, what the chief owns and
//! sells, and buying, installing and selling in the background.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use bevy_app::{App, Startup, Update};
use bevy_ecs::prelude::*;
use hamlet_model::market::account;
use hamlet_model::market::client::{
    Client, Poll, refused, signed_out,
};
use hamlet_model::market::lot::{
    Detail, Device, Draft, Kind, Lot, Owned, Paying, Status, User,
};
use hamlet_model::market::package::{Contents, Package};
use hamlet_rules::hamlet::HOME;
use hamlet_rules::market::{
    Keeping, Kept, bring_up, hire, install_tools, pack_agent,
    pack_hamlet, pack_tools, refresh,
};
use hamlet_rules::realm::Realm;
use tokio::task::AbortHandle;

use crate::core::{
    HomeDir, Model, RefreshLists, Rounds, Settings, Tokio, answered,
    browse,
};

/// The time between asks whether a payment went through.
const PAYMENT_POLL: Duration = Duration::from_secs(3);

/// The longest a payment is waited for.
const PAYMENT_PATIENCE: Duration = Duration::from_secs(30 * 60);

/// The biggest cover the server takes.
const COVER_BYTES: usize = 2 * 1024 * 1024;

/// Why something was refused, in words for the chief.
type Why = String;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Market>()
        .init_resource::<Rounds<Signing>>()
        .init_resource::<Rounds<Accounted>>()
        .init_resource::<Rounds<Stocked>>()
        .init_resource::<Rounds<Opened>>()
        .init_resource::<Rounds<Dealt>>()
        .init_resource::<Rounds<Arrival>>()
        .init_resource::<Rounds<Sold>>()
        .init_resource::<Rounds<Done>>()
        .add_observer(sign_in)
        .add_observer(sign_out)
        .add_observer(ask_account)
        .add_observer(ask_shelves)
        .add_observer(ask_detail)
        .add_observer(set_up_payouts)
        .add_observer(buy)
        .add_observer(install)
        .add_observer(sell)
        .add_observer(shelve)
        .add_observer(review)
        .add_systems(Startup, open)
        .add_systems(
            Update,
            (
                signing.run_if(answered::<Signing>),
                accounted.run_if(answered::<Accounted>),
                stocked.run_if(answered::<Stocked>),
                opened.run_if(answered::<Opened>),
                (
                    dealt.run_if(answered::<Dealt>),
                    arrived.run_if(answered::<Arrival>),
                )
                    .chain(),
                sold.run_if(answered::<Sold>),
                done.run_if(answered::<Done>),
            ),
        );
}

/// The bazaar's server and all the chief does there.
#[derive(Resource, Default)]
pub struct Market {
    /// The server's address.
    pub server: String,
    token: Option<String>,
    pub account: Account,
    pub shelves: Shelves,
    /// The listing last opened on a page of its own, by its id.
    /// `None` while it is asked for.
    pub detail: Option<(String, Option<Result<Detail, Why>>)>,
    /// What the chief owns, once they signed in.
    pub library: Vec<Owned>,
    /// What the chief sells, in every state.
    pub mine: Vec<Lot>,
    /// What was bought and written into the home.
    pub kept: Kept,
    /// The listing being bought or installed, or last so, by its id.
    pub deal: Option<(String, Deal)>,
    /// How putting something up for sale goes.
    pub selling: Option<Selling>,
    /// What came of the last thing done with the account or the
    /// listings sold, and whether it went wrong.
    pub said: Option<(String, bool)>,
    signing_task: Option<AbortHandle>,
    paying_task: Option<AbortHandle>,
    /// The token is kept in the keychain. Not in a recorded session.
    keychain: bool,
}

impl Market {
    /// Keeps `token` for the next run, or forgets the one kept.
    fn keep(
        &self,
        home: &std::path::Path,
        token: Option<&str>,
    ) -> anyhow::Result<()> {
        match self.keychain {
            true => account::keep_token(home, token),
            false => Ok(()),
        }
    }

    pub(crate) fn client(&self) -> Client {
        Client::new(&self.server, self.token.clone())
    }

    /// True when the chief has a token from signing in.
    pub(crate) fn signed(&self) -> bool {
        self.token.is_some()
    }

    /// Talks to `server` as signed in with `token`.
    #[cfg(test)]
    pub(crate) fn sign_for_test(
        &mut self,
        server: String,
        token: &str,
    ) {
        self.server = server;
        self.token = Some(token.to_string());
    }

    /// The one signed in, if anyone is.
    pub fn user(&self) -> Option<&User> {
        match &self.account {
            Account::In(user) => Some(user),
            _ => None,
        }
    }

    /// True when the chief owns the listing `id`.
    pub fn owns(&self, id: &str) -> bool {
        self.library.iter().any(|owned| owned.listing.id == id)
            || matches!(
                &self.detail,
                Some((open, Some(Ok(detail)))) if open == id && detail.owned
            )
    }

    /// The listings of `kind` on the shelves.
    pub fn lots(&self, kind: Kind) -> &[Lot] {
        self.shelves.lots.get(&kind).map_or(&[], Vec::as_slice)
    }

    /// The listing of the id `id`, wherever it was seen.
    pub fn lot(&self, id: &str) -> Option<&Lot> {
        let detail = match &self.detail {
            Some((open, Some(Ok(detail)))) if open == id => {
                Some(&detail.listing)
            }
            _ => None,
        };
        detail
            .or_else(|| {
                self.shelves
                    .lots
                    .values()
                    .flatten()
                    .find(|l| l.id == id)
            })
            .or_else(|| {
                self.library
                    .iter()
                    .map(|owned| &owned.listing)
                    .find(|l| l.id == id)
            })
            .or_else(|| self.mine.iter().find(|l| l.id == id))
    }

    /// The detail of the listing `id`, once it came.
    pub fn detail_of(&self, id: &str) -> Option<&Detail> {
        match &self.detail {
            Some((open, Some(Ok(detail)))) if open == id => {
                Some(detail)
            }
            _ => None,
        }
    }

    /// True while a listing is being bought or installed.
    pub fn dealing(&self) -> bool {
        matches!(
            self.deal,
            Some((
                _,
                Deal::Buying | Deal::Paying(_) | Deal::Fetching
            ))
        )
    }

    /// True while something is being put up for sale.
    pub fn busy_selling(&self) -> bool {
        matches!(self.selling, Some(Selling::Going(_)))
    }

    /// The version of the listing `id` that was written into the
    /// home, if one was.
    pub fn kept_version(&self, id: &str) -> Option<&str> {
        self.kept.get(id).map(|keeping| keeping.version.as_str())
    }
}

/// The chief at the bazaar.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Account {
    #[default]
    Out,
    /// Signing in has begun.
    Asking,
    /// The chief enters the code at GitHub.
    Code(Device),
    In(User),
    Failed(Why),
}

/// The listings on the shelves.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Shelves {
    pub lots: BTreeMap<Kind, Vec<Lot>>,
    /// The server is being asked.
    pub asking: bool,
    /// They were asked for at least once.
    pub asked: bool,
    pub problem: Option<Why>,
}

/// How the buying or installing of a listing goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Deal {
    /// Asked of the server.
    Buying,
    /// Paid for on Stripe's page, at this address.
    Paying(String),
    /// Its package is downloaded.
    Fetching,
    Done(String),
    Failed(Why),
}

/// Where a package bought is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A hamlet founded in the realm.
    Realm,
    /// An agent moved into the hamlet of this name.
    Hamlet(String),
    /// Tools added to the house of `agent` of `hamlet`.
    House { hamlet: String, agent: String },
}

/// How putting something up for sale goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Selling {
    /// Doing this.
    Going(&'static str),
    /// Saved, as the listing of this title, now in this state.
    Done(String, Status),
    Failed(Why),
}

/// What is packed to be sold.
#[derive(Clone, Debug, PartialEq)]
pub enum Wares {
    Agent {
        hamlet: String,
        agent: String,
        notes: bool,
    },
    Hamlet {
        name: String,
        kind: String,
        notes: bool,
    },
    /// The tools in these folders.
    Tools(Vec<PathBuf>),
}

/// Begins signing in to the bazaar with GitHub.
#[derive(Event)]
pub struct SignInBazaar;

/// Signs out of the bazaar, or stops a sign-in that has begun.
#[derive(Event)]
pub struct SignOutBazaar;

/// Asks who is signed in, what they own and what they sell.
#[derive(Event)]
pub struct AskAccount;

/// Asks for the listings on the shelves.
#[derive(Event)]
pub struct AskShelves;

/// Asks for the listing of this id on a page of its own.
#[derive(Event)]
pub struct AskDetail(pub String);

/// Opens Stripe's page where the chief sets up how they are paid.
#[derive(Event)]
pub struct SetUpPayouts;

/// Buys the listing `id`, then writes its newest version to
/// `target`. `by` is the chief, as the realm names them.
#[derive(Event, Clone)]
pub struct Buy {
    pub id: String,
    pub target: Target,
    pub by: String,
}

/// Writes `version` of the listing `id`, owned, to `target`; over
/// what it wrote before when it was written already.
#[derive(Event, Clone)]
pub struct Install {
    pub id: String,
    pub version: String,
    pub target: Target,
    pub by: String,
}

/// Puts something up for sale, or changes a listing of the chief's.
#[derive(Event, Clone)]
pub struct Sell {
    /// The listing changed; `None` for a new one.
    pub id: Option<String>,
    pub draft: Draft,
    /// What is packed as its new version, if one is uploaded.
    pub wares: Option<Wares>,
    pub version: String,
    pub changes: String,
    /// A PNG, JPEG or WebP picture for its cover.
    pub cover: Option<PathBuf>,
    /// Put on the shelves once done.
    pub publish: bool,
}

/// Puts the listing `id` of the chief's on the shelves, or takes it
/// off.
#[derive(Event)]
pub struct Shelve {
    pub id: String,
    pub on: bool,
}

/// Reviews the listing `id`.
#[derive(Event)]
pub struct Review {
    pub id: String,
    pub stars: u8,
    pub words: String,
}

/// A sign-in's progress.
enum Signing {
    Code(Device),
    In(String, User),
    Failed(Why),
}

/// Who is signed in, what they own and sell; or that the token was
/// refused.
type Accounted = Result<(User, Vec<Owned>, Vec<Lot>), (Why, bool)>;

type Stocked = Result<BTreeMap<Kind, Vec<Lot>>, Why>;

type Opened = (String, Result<Detail, Why>);

/// A deal's progress, by listing id.
type Dealt = (String, Deal);

/// A package downloaded, to be written into the home.
struct Arrival {
    id: String,
    version: String,
    package: Package,
    target: Target,
    by: String,
}

type Sold = Selling;

/// What came of something done with the account or a listing; and
/// whether the account should be asked again.
type Done = (Result<String, Why>, bool);

/// Words for the chief from what went wrong.
fn words(e: anyhow::Error) -> Why {
    format!("{e:#}")
}

/// Reads who was signed in, and asks the server about them.
fn open(world: &mut World) {
    let config = world.resource::<Settings>().config.clone();
    let server = account::server(&config);
    world.resource_mut::<Market>().server = server;
    let home = world.resource::<HomeDir>().0.root.clone();
    world.resource_mut::<Market>().kept = Kept::load(&home);
    // A recorded session asks nothing of a network, nor of the
    // keychain.
    if world.resource::<Model>().replay {
        return;
    }
    let token = account::token(&home);
    let signed = token.is_some();
    let mut market = world.resource_mut::<Market>();
    market.token = token;
    market.keychain = true;
    if signed {
        world.trigger(AskAccount);
    }
    world.trigger(AskShelves);
}

fn sign_in(
    _: On<SignInBazaar>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Signing>>,
    mut market: ResMut<Market>,
) {
    if let Some(task) = market.signing_task.take() {
        task.abort();
    }
    market.account = Account::Asking;
    let client = Client::new(&market.server, None);
    let tell = rounds.begin();
    let task = tokio.0.spawn(async move {
        let device = match client.start_sign_in().await {
            Ok(device) => device,
            Err(e) => return tell(Signing::Failed(words(e))),
        };
        let _ = browse(&device.verification_uri);
        tell(Signing::Code(device.clone()));
        let mut every = Duration::from_secs(device.interval.max(1));
        let until = tokio::time::Instant::now()
            + Duration::from_secs(device.expires_in);
        while tokio::time::Instant::now() < until {
            tokio::time::sleep(every).await;
            match client.poll_sign_in(&device.device_code).await {
                Ok(Poll::Pending) => {}
                Ok(Poll::SlowDown) => every += Duration::from_secs(5),
                Ok(Poll::Approved(approved)) => {
                    return tell(Signing::In(
                        approved.token,
                        approved.user,
                    ));
                }
                Err(e) => {
                    let why = match refused(&e) {
                        Some("expired") => {
                            "The code ran out. Sign in again.".into()
                        }
                        Some("denied") => "GitHub was told no. Sign \
                                           in again to retry."
                            .into(),
                        _ => words(e),
                    };
                    return tell(Signing::Failed(why));
                }
            }
        }
        tell(Signing::Failed(
            "The code ran out. Sign in again.".into(),
        ));
    });
    market.signing_task = Some(task.abort_handle());
}

fn signing(
    home: Res<HomeDir>,
    mut rounds: ResMut<Rounds<Signing>>,
    mut market: ResMut<Market>,
    mut commands: Commands,
) {
    while let Some(went) = rounds.next() {
        match went {
            Signing::Code(device) => {
                market.account = Account::Code(device)
            }
            Signing::Failed(why) => {
                market.account = Account::Failed(why)
            }
            Signing::In(token, user) => {
                market.signing_task = None;
                if let Err(e) =
                    market.keep(&home.0.root, Some(&token))
                {
                    market.said = Some((
                        format!(
                            "Signed in, but the sign-in could not \
                             be kept for next time ({e:#})."
                        ),
                        true,
                    ));
                }
                market.token = Some(token);
                market.account = Account::In(user);
                commands.trigger(AskAccount);
                commands.trigger(AskShelves);
            }
        }
    }
}

fn sign_out(
    _: On<SignOutBazaar>,
    tokio: Res<Tokio>,
    home: Res<HomeDir>,
    mut market: ResMut<Market>,
) {
    if let Some(task) = market.signing_task.take() {
        task.abort();
    }
    if market.token.is_some() {
        let client = market.client();
        // The server forgets the token too; nothing waits for it.
        tokio.0.spawn(async move {
            let _ = client.sign_out().await;
        });
    }
    let _ = market.keep(&home.0.root, None);
    market.token = None;
    market.account = Account::Out;
    market.library.clear();
    market.mine.clear();
}

fn ask_account(
    _: On<AskAccount>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Accounted>>,
    market: Res<Market>,
) {
    if market.token.is_none() {
        return;
    }
    let client = market.client();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let asked = async {
            let user = client.me().await?;
            let library = client.library().await?;
            let mine = client.mine().await?;
            anyhow::Ok((user, library, mine))
        };
        tell(asked.await.map_err(|e| {
            let out = signed_out(&e);
            (words(e), out)
        }));
    });
}

fn accounted(
    home: Res<HomeDir>,
    mut rounds: ResMut<Rounds<Accounted>>,
    mut market: ResMut<Market>,
) {
    while let Some(asked) = rounds.next() {
        match asked {
            Ok((user, library, mine)) => {
                market.account = Account::In(user);
                market.library = library;
                market.mine = mine;
            }
            // The token ran out or was revoked.
            Err((_, true)) => {
                let _ = market.keep(&home.0.root, None);
                market.token = None;
                market.account = Account::Out;
            }
            Err((why, false)) => {
                market.said = Some((
                    format!(
                        "The bazaar could not be reached ({why})."
                    ),
                    true,
                ));
            }
        }
    }
}

fn ask_shelves(
    _: On<AskShelves>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Stocked>>,
    mut market: ResMut<Market>,
) {
    market.shelves.asking = true;
    let client = market.client();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let asked = async {
            let (agents, hamlets, plugins) = tokio::try_join!(
                client.lots(Kind::Agent),
                client.lots(Kind::Hamlet),
                client.lots(Kind::Plugin),
            )?;
            anyhow::Ok(BTreeMap::from([
                (Kind::Agent, agents),
                (Kind::Hamlet, hamlets),
                (Kind::Plugin, plugins),
            ]))
        };
        tell(asked.await.map_err(words));
    });
}

fn stocked(
    mut rounds: ResMut<Rounds<Stocked>>,
    mut market: ResMut<Market>,
) {
    while let Some(asked) = rounds.next() {
        let shelves = &mut market.shelves;
        shelves.asking = false;
        shelves.asked = true;
        match asked {
            Ok(lots) => {
                shelves.lots = lots;
                shelves.problem = None;
            }
            Err(why) => shelves.problem = Some(why),
        }
    }
}

fn ask_detail(
    asked: On<AskDetail>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Opened>>,
    mut market: ResMut<Market>,
) {
    let id = asked.event().0.clone();
    market.detail = Some((id.clone(), None));
    let client = market.client();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let detail = client.detail(&id).await.map_err(words);
        tell((id, detail));
    });
}

fn opened(
    mut rounds: ResMut<Rounds<Opened>>,
    mut market: ResMut<Market>,
) {
    while let Some((id, detail)) = rounds.next() {
        market.detail = Some((id, Some(detail)));
    }
}

fn set_up_payouts(
    _: On<SetUpPayouts>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Done>>,
    mut market: ResMut<Market>,
) {
    if market.token.is_none() {
        market.said =
            Some(("Sign in with GitHub first.".into(), true));
        return;
    }
    let client = market.client();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let went = match client.payouts().await {
            Ok(url) => match browse(&url) {
                Ok(()) => Ok("Stripe opened in your browser. Come \
                              back once you have finished there."
                    .to_string()),
                Err(_) => {
                    Ok(format!("Open {url} to set up payouts."))
                }
            },
            Err(e) => Err(words(e)),
        };
        tell((went, false));
    });
}

fn buy(
    asked: On<Buy>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Dealt>>,
    mut arrivals: ResMut<Rounds<Arrival>>,
    mut market: ResMut<Market>,
) {
    let Buy { id, target, by } = asked.event().clone();
    if market.dealing() {
        return;
    }
    if market.token.is_none() {
        market.deal = Some((
            id,
            Deal::Failed("Sign in with GitHub to buy.".into()),
        ));
        return;
    }
    let Some(version) = market
        .lot(&id)
        .and_then(|lot| lot.latest.as_ref())
        .map(|latest| latest.version.clone())
    else {
        market.deal = Some((
            id,
            Deal::Failed("It has no version to download yet.".into()),
        ));
        return;
    };
    if let Some(task) = market.paying_task.take() {
        task.abort();
    }
    market.deal = Some((id.clone(), Deal::Buying));
    let client = market.client();
    let (tell, arrive) = (rounds.begin(), arrivals.begin());
    let task = tokio.0.spawn(async move {
        let failed =
            |id: &str, why: Why| (id.to_string(), Deal::Failed(why));
        let bought = match client.buy(&id).await {
            Ok(bought) => bought,
            // Owned already: it only needs installing.
            Err(e) if refused(&e) == Some("conflict") => {
                hamlet_model::market::lot::Bought {
                    owned: true,
                    purchase_id: None,
                    checkout_url: None,
                }
            }
            Err(e) => return tell(failed(&id, words(e))),
        };
        if !bought.owned {
            let (Some(purchase), Some(url)) =
                (bought.purchase_id, bought.checkout_url)
            else {
                return tell(failed(
                    &id,
                    "The bazaar gave no page to pay on.".into(),
                ));
            };
            let _ = browse(&url);
            tell((id.clone(), Deal::Paying(url)));
            let until =
                tokio::time::Instant::now() + PAYMENT_PATIENCE;
            loop {
                if tokio::time::Instant::now() > until {
                    return tell(failed(
                        &id,
                        "The payment was not finished in time."
                            .into(),
                    ));
                }
                tokio::time::sleep(PAYMENT_POLL).await;
                match client.paying(&purchase).await {
                    Ok(Paying::Paid) => break,
                    Ok(Paying::Pending) => {}
                    Ok(Paying::Expired) => {
                        return tell(failed(
                            &id,
                            "The payment was not made. Nothing was \
                             charged."
                                .into(),
                        ));
                    }
                    // A blip of the network is waited out.
                    Err(_) => {}
                }
            }
        }
        tell((id.clone(), Deal::Fetching));
        match client.package(&id, &version).await {
            Ok(package) => arrive(Arrival {
                id,
                version,
                package,
                target,
                by,
            }),
            Err(e) => tell(failed(&id, words(e))),
        }
    });
    market.paying_task = Some(task.abort_handle());
}

fn install(
    asked: On<Install>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Dealt>>,
    mut arrivals: ResMut<Rounds<Arrival>>,
    mut market: ResMut<Market>,
) {
    let Install {
        id,
        version,
        target,
        by,
    } = asked.event().clone();
    if market.dealing() {
        return;
    }
    market.deal = Some((id.clone(), Deal::Fetching));
    let client = market.client();
    let (tell, arrive) = (rounds.begin(), arrivals.begin());
    tokio.0.spawn(async move {
        match client.package(&id, &version).await {
            Ok(package) => arrive(Arrival {
                id,
                version,
                package,
                target,
                by,
            }),
            Err(e) => tell((id, Deal::Failed(words(e)))),
        }
    });
}

/// Follows a deal.
fn dealt(
    mut rounds: ResMut<Rounds<Dealt>>,
    mut market: ResMut<Market>,
) {
    while let Some(dealt) = rounds.next() {
        market.deal = Some(dealt);
    }
}

/// Writes a package downloaded into the home.
fn arrived(world: &mut World) {
    loop {
        let next = world.resource_mut::<Rounds<Arrival>>().next();
        let Some(Arrival {
            id,
            version,
            package,
            target,
            by,
        }) = next
        else {
            return;
        };
        let went =
            settle(world, &id, &version, &package, &target, &by);
        let deal = match went {
            Ok(words) => Deal::Done(words),
            Err(e) => Deal::Failed(words(e)),
        };
        crate::agents::sync(world);
        world.trigger(RefreshLists);
        world.trigger(AskAccount);
        world.resource_mut::<Market>().deal = Some((id, deal));
    }
}

/// Writes `package`, `version` of the listing `id`, into the home at
/// `target`, and records it. Returns what came of it.
fn settle(
    world: &mut World,
    id: &str,
    version: &str,
    package: &Package,
    target: &Target,
    by: &str,
) -> anyhow::Result<String> {
    let home = world.resource::<HomeDir>().0.root.clone();
    let realm = Realm::new(home.clone());
    let mut kept = Kept::load(&home);
    let before = kept.get(id).cloned();
    let title = world
        .resource::<Market>()
        .lot(id)
        .map_or_else(|| id.to_string(), |lot| lot.title.clone());
    let (keeping, words) = match (&package.contents, target) {
        (Contents::Agent { agent }, Target::Hamlet(hamlet)) => {
            match before.as_ref().and_then(|b| {
                Some((b.hamlet.clone()?, b.names.first()?.clone()))
            }) {
                Some((hamlet, name))
                    if realm.hamlet_of(&name).as_deref()
                        == Some(hamlet.as_str()) =>
                {
                    let house =
                        realm.hamlet(&hamlet)?.house(&name)?;
                    refresh(&house, agent)?;
                    let words =
                        format!("{name} is up to version {version}.");
                    (
                        keeping(
                            package,
                            &title,
                            version,
                            vec![name],
                            Some(hamlet),
                            None,
                        ),
                        words,
                    )
                }
                _ => {
                    let at = realm.hamlet(hamlet)?;
                    let name = hire(
                        &at,
                        agent,
                        "bought at the Agent Bazaar",
                    )?;
                    let words =
                        format!("{name} moved into {hamlet}.");
                    (
                        keeping(
                            package,
                            &title,
                            version,
                            vec![name],
                            Some(hamlet.clone()),
                            None,
                        ),
                        words,
                    )
                }
            }
        }
        (Contents::Hamlet { hamlet }, Target::Realm) => {
            match realm
                .hamlets()
                .iter()
                .any(|place| place.name == hamlet.name)
            {
                true if before.is_some() => {
                    bring_up(&realm.hamlet(&hamlet.name)?, hamlet)?;
                    let words = format!(
                        "{} is up to version {version}.",
                        hamlet.title
                    );
                    (
                        keeping(
                            package,
                            &title,
                            version,
                            vec![hamlet.name.clone()],
                            None,
                            None,
                        ),
                        words,
                    )
                }
                true => anyhow::bail!(
                    "the realm has a hamlet named `{}` already",
                    hamlet.name
                ),
                false => {
                    realm.found_bought(hamlet, by)?;
                    let words = format!(
                        "{} stands in your realm, with {} and all \
                         they know.",
                        hamlet.title, hamlet.mayor.name
                    );
                    (
                        keeping(
                            package,
                            &title,
                            version,
                            vec![hamlet.name.clone()],
                            None,
                            None,
                        ),
                        words,
                    )
                }
            }
        }
        (
            Contents::Plugin { plugin },
            Target::House { hamlet, agent },
        ) => {
            let replacing = before
                .as_ref()
                .filter(|b| {
                    b.hamlet.as_ref() == Some(hamlet)
                        && b.house.as_ref() == Some(agent)
                })
                .map(|b| b.names.clone())
                .unwrap_or_default();
            let house = realm.hamlet(hamlet)?.house(agent)?;
            let names = install_tools(&house, plugin, &replacing)?;
            let words =
                format!("{agent} can use {} now.", names.join(", "));
            (
                keeping(
                    package,
                    &title,
                    version,
                    names,
                    Some(hamlet.clone()),
                    Some(agent.clone()),
                ),
                words,
            )
        }
        _ => anyhow::bail!("that cannot be installed there"),
    };
    kept.0.insert(id.to_string(), keeping);
    kept.save(&home)?;
    world.resource_mut::<Market>().kept = kept;
    Ok(words)
}

fn keeping(
    package: &Package,
    title: &str,
    version: &str,
    names: Vec<String>,
    hamlet: Option<String>,
    house: Option<String>,
) -> Keeping {
    Keeping {
        kind: package.kind(),
        title: title.to_string(),
        version: version.to_string(),
        names,
        hamlet,
        house,
    }
}

/// Where a listing that is not yet installed goes by default.
pub fn default_target(world: &World, kind: Kind) -> Target {
    match kind {
        Kind::Hamlet => Target::Realm,
        Kind::Agent => Target::Hamlet(HOME.to_string()),
        Kind::Plugin => {
            let home = world.resource::<HomeDir>().0.root.clone();
            let mayor = Realm::new(home)
                .hamlet(HOME)
                .map(|hamlet| hamlet.mayor().name)
                .unwrap_or_default();
            Target::House {
                hamlet: HOME.to_string(),
                agent: mayor,
            }
        }
    }
}

/// Where the listing `id` was written before, as a target to write
/// it to again.
pub fn kept_target(market: &Market, id: &str) -> Option<Target> {
    let kept = market.kept.get(id)?;
    Some(match kept.kind {
        Kind::Hamlet => Target::Realm,
        Kind::Agent => Target::Hamlet(kept.hamlet.clone()?),
        Kind::Plugin => Target::House {
            hamlet: kept.hamlet.clone()?,
            agent: kept.house.clone()?,
        },
    })
}

fn sell(
    asked: On<Sell>,
    tokio: Res<Tokio>,
    home: Res<HomeDir>,
    mut rounds: ResMut<Rounds<Sold>>,
    mut market: ResMut<Market>,
) {
    let offer = asked.event().clone();
    if market.busy_selling() {
        return;
    }
    if market.token.is_none() {
        market.selling = Some(Selling::Failed(
            "Sign in with GitHub first.".into(),
        ));
        return;
    }
    let realm = Realm::new(home.0.root.clone());
    let packed = offer.wares.as_ref().map(|wares| {
        let contents = match wares {
            Wares::Agent {
                hamlet,
                agent,
                notes,
            } => Contents::Agent {
                agent: pack_agent(
                    &realm.hamlet(hamlet)?,
                    agent,
                    *notes,
                )?,
            },
            Wares::Hamlet { name, kind, notes } => Contents::Hamlet {
                hamlet: pack_hamlet(&realm, name, kind, *notes)?,
            },
            Wares::Tools(dirs) => Contents::Plugin {
                plugin: pack_tools(dirs)?,
            },
        };
        let package = Package::new(contents);
        package.check()?;
        package.bytes()?;
        anyhow::Ok(package)
    });
    let package = match packed.transpose() {
        Ok(package) => package,
        Err(e) => {
            market.selling = Some(Selling::Failed(format!(
                "It could not be packed: {e:#}."
            )));
            return;
        }
    };
    let cover = match offer
        .cover
        .as_ref()
        .map(|path| cover(path))
        .transpose()
    {
        Ok(cover) => cover,
        Err(why) => {
            market.selling = Some(Selling::Failed(why));
            return;
        }
    };
    market.selling = Some(Selling::Going("Sending it to the bazaar"));
    let client = market.client();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let went = async {
            let lot = match &offer.id {
                Some(id) => client.change(id, &offer.draft).await?,
                None => client.create(&offer.draft).await?,
            };
            tell(Selling::Going("Uploading"));
            if let Some((bytes, kind)) = cover {
                client.cover(&lot.id, bytes, kind).await?;
            }
            if let Some(package) = &package {
                client
                    .release(
                        &lot.id,
                        &offer.version,
                        &offer.changes,
                        package,
                    )
                    .await?;
            }
            let lot = match offer.publish {
                true => {
                    tell(Selling::Going("Putting it on the shelves"));
                    client.publish(&lot.id).await?
                }
                false => client.detail(&lot.id).await?.listing,
            };
            anyhow::Ok(lot)
        };
        tell(match went.await {
            Ok(lot) => Selling::Done(lot.title, lot.status),
            Err(e) => Selling::Failed(words(e)),
        });
    });
}

/// The bytes of the picture at `path` and its type, if the server
/// takes it.
fn cover(
    path: &std::path::Path,
) -> Result<(Vec<u8>, &'static str), Why> {
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let kind = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => {
            return Err(
                "A cover is a PNG, JPEG or WebP picture.".into()
            );
        }
    };
    let bytes = std::fs::read(path)
        .map_err(|e| format!("The cover could not be read ({e})."))?;
    if bytes.len() > COVER_BYTES {
        return Err("A cover is at most 2 MB.".into());
    }
    Ok((bytes, kind))
}

fn sold(
    mut rounds: ResMut<Rounds<Sold>>,
    mut market: ResMut<Market>,
    mut commands: Commands,
) {
    while let Some(selling) = rounds.next() {
        if matches!(selling, Selling::Done(..) | Selling::Failed(_)) {
            commands.trigger(AskAccount);
            commands.trigger(AskShelves);
        }
        market.selling = Some(selling);
    }
}

fn shelve(
    asked: On<Shelve>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Done>>,
    market: Res<Market>,
) {
    let Shelve { id, on } = asked.event();
    let (id, on) = (id.clone(), *on);
    let client = market.client();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let went = match on {
            true => client.publish(&id).await,
            false => client.unlist(&id).await,
        };
        let went = went.map(|lot| match on {
            true => format!("{} is on the shelves.", lot.title),
            false => format!(
                "{} is off the shelves. Those who own it keep it.",
                lot.title
            ),
        });
        tell((went.map_err(words), true));
    });
}

fn review(
    asked: On<Review>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Done>>,
    market: Res<Market>,
) {
    let Review {
        id,
        stars,
        words: said,
    } = asked.event();
    let (id, stars, said) = (id.clone(), *stars, said.clone());
    let client = market.client();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let went = client
            .review(&id, stars, &said)
            .await
            .map(|_| "Thank you for your review.".to_string());
        tell((went.map_err(words), false));
    });
}

fn done(
    mut rounds: ResMut<Rounds<Done>>,
    mut market: ResMut<Market>,
    mut commands: Commands,
) {
    while let Some((went, again)) = rounds.next() {
        if again {
            commands.trigger(AskAccount);
            commands.trigger(AskShelves);
        }
        if let Some((open, _)) = &market.detail {
            commands.trigger(AskDetail(open.clone()));
        }
        market.said = Some(match went {
            Ok(words) => (words, false),
            Err(why) => (why, true),
        });
    }
}

#[cfg(test)]
mod tests {
    use hamlet_model::market::package::{Agent, File, NOTES};
    use hamlet_model::market::samples;
    use hamlet_sandbox::Unsandboxed;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::runtime::Runtime;

    use super::*;
    use crate::testing::{self, place, until};

    fn scribe(character: &str) -> Package {
        let file = |path: &str, text: &str| File {
            path: path.into(),
            data: text.as_bytes().to_vec(),
            executable: false,
        };
        Package::new(Contents::Agent {
            agent: Agent {
                name: "scribe".into(),
                purpose: "Writes letters.".into(),
                model: None,
                files: vec![
                    file("CHARACTER.md", character),
                    file(NOTES, "# Notes\nfrom the seller"),
                ],
            },
        })
    }

    /// A stand-in for hamlet-server that answers the routes buying
    /// takes: a free listing bought, and its versions downloaded.
    async fn serve(listener: TcpListener) {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut asked = Vec::new();
                let mut buf = [0u8; 4096];
                while !asked.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => asked.extend_from_slice(&buf[..n]),
                    }
                }
                let head =
                    String::from_utf8_lossy(&asked).into_owned();
                let line = head.lines().next().unwrap_or_default();
                let (status, body) = match line {
                    l if l.starts_with("POST /v1/listings/l_scribe/purchase") => {
                        ("200 OK", json!({"owned": true}).to_string())
                    }
                    l if l.starts_with("GET /v1/listings/l_scribe/versions/1.0.0/package") => {
                        ("200 OK", String::from_utf8(scribe("Careful.").bytes().unwrap()).unwrap())
                    }
                    l if l.starts_with("GET /v1/listings/l_scribe/versions/1.1.0/package") => {
                        ("200 OK", String::from_utf8(scribe("Bolder.").bytes().unwrap()).unwrap())
                    }
                    _ => (
                        "401 Unauthorized",
                        json!({"error": {"code": "unauthorized", "message": "no"}}).to_string(),
                    ),
                };
                let answer = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: \
                     application/json\r\ncontent-length: \
                     {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(answer.as_bytes()).await;
            });
        }
    }

    #[test]
    fn an_agent_bought_moves_in_and_is_brought_up_to_a_new_version() {
        let rt = Runtime::new().unwrap();
        let listener =
            rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let at = listener.local_addr().unwrap();
        rt.spawn(serve(listener));
        let home = place("market-buy");
        let mut app = testing::app(&home, &rt, &Unsandboxed, vec![]);
        app.update();
        let mut lot = samples::lots(Kind::Agent).remove(0);
        lot.id = "l_scribe".into();
        lot.latest.as_mut().unwrap().version = "1.0.0".into();
        let mut market = app.world_mut().resource_mut::<Market>();
        market.server = format!("http://{at}");
        market.token = Some("hms_test".into());
        market.shelves.lots.insert(Kind::Agent, vec![lot]);

        app.world_mut().trigger(Buy {
            id: "l_scribe".into(),
            target: Target::Hamlet(HOME.into()),
            by: "alice".into(),
        });
        until(&mut app, "the agent to move in", |app| {
            matches!(
                &app.world().resource::<Market>().deal,
                Some((_, Deal::Done(_) | Deal::Failed(_)))
            )
        });
        let market = app.world().resource::<Market>();
        assert!(
            matches!(&market.deal, Some((_, Deal::Done(_)))),
            "{:?}",
            market.deal
        );
        assert_eq!(market.kept_version("l_scribe"), Some("1.0.0"));
        let realm = Realm::new(home.root.clone());
        let house =
            realm.hamlet(HOME).unwrap().house("scribe").unwrap();
        let read = |path| std::fs::read_to_string(path).unwrap();
        assert_eq!(read(house.character()), "Careful.");
        std::fs::write(house.notes(), "# Notes\nits own").unwrap();
        // The 401 the account asks for after signs the chief out.
        until(&mut app, "the token to be refused", |app| {
            app.world().resource::<Market>().token.is_none()
        });
        app.world_mut().resource_mut::<Market>().token =
            Some("hms_test".into());

        let target =
            kept_target(app.world().resource::<Market>(), "l_scribe");
        app.world_mut().trigger(Install {
            id: "l_scribe".into(),
            version: "1.1.0".into(),
            target: target.unwrap(),
            by: "alice".into(),
        });
        until(&mut app, "the agent to be brought up", |app| {
            app.world().resource::<Market>().kept_version("l_scribe")
                == Some("1.1.0")
        });
        assert_eq!(read(house.character()), "Bolder.");
        assert_eq!(read(house.notes()), "# Notes\nits own");
        assert_eq!(realm.hamlet(HOME).unwrap().residents().len(), 1);
        assert_eq!(Kept::load(&home.root).0.len(), 1);
    }
}
