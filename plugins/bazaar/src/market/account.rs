//! The chief at the bazaar: signing in with GitHub through the server
//! and out, who they are, what they own and sell, and the page where
//! they are paid.

use hamlet_engine_bevy::api::DesktopRequest;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Failure, Reply};

use crate::market::client::read;
use crate::market::lot::{
    Approved, Device, Link, Lot, Owned, Page, User,
};
use crate::market::{Market, ask_shelves, send, session};
use crate::shelf::now;

/// The seconds added between asks when GitHub says to slow down.
const SLOWER: u64 = 5;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        poll.run_if(|market: Res<Market>| {
            matches!(market.account, Account::Code { .. })
        }),
    );
}

/// The chief at the bazaar.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum Account {
    #[default]
    Out,
    /// Signing in has begun.
    Asking,
    /// The chief enters the code at GitHub, which is asked after
    /// until they have.
    Code {
        device: Device,
        asking: Asking,
    },
    In(User),
    Failed(String),
}

/// Asking the server after something again and again, until it says
/// or the time runs out, in milliseconds since 1970.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Asking {
    pub next: u64,
    pub every: u64,
    pub until: u64,
    /// An ask is on its way.
    pub out: bool,
}

impl Asking {
    pub fn new(now: u64, every: u64, lasting: u64) -> Asking {
        Asking {
            next: now + every,
            every,
            until: now + lasting,
            out: false,
        }
    }

    /// True when the next ask is due at `now`.
    pub fn due(&self, now: u64) -> bool {
        !self.out && now >= self.next
    }

    /// The answer came at `now`; the next ask is due in a while.
    pub fn heard(&mut self, now: u64) {
        self.out = false;
        self.next = now + self.every;
    }

    pub fn over(&self, now: u64) -> bool {
        now > self.until
    }
}

/// A call of the server about the account.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    Device,
    /// Whether the sign-in of this device code was approved.
    Poll(String),
    SignOut,
    Me,
    Library,
    Mine,
    Payouts,
}

/// What came of asking after a sign-in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Polled {
    /// Not approved yet.
    Pending,
    /// Asked too often.
    SlowDown,
    Approved(Approved),
    Failed(String),
}

/// What the answer to asking after a sign-in comes to.
pub(crate) fn polled(reply: Result<Reply, Failure>) -> Polled {
    match read::<Approved>(reply) {
        Ok(approved) => Polled::Approved(approved),
        Err(refusal) => match refusal.code.as_str() {
            "pending" => Polled::Pending,
            "slow_down" => Polled::SlowDown,
            "expired" => Polled::Failed(
                "The code ran out. Sign in again.".into(),
            ),
            "denied" => Polled::Failed(
                "GitHub was told no. Sign in again to retry.".into(),
            ),
            _ => Polled::Failed(refusal.words()),
        },
    }
}

fn ask(world: &mut World, call: Call) {
    let server = &world.resource::<Market>().server;
    let request = match &call {
        Call::Device => server.device(),
        Call::Poll(code) => server.poll(code),
        Call::SignOut => server.sign_out(),
        Call::Me => server.me(),
        Call::Library => server.library(),
        Call::Mine => server.mine(),
        Call::Payouts => server.payouts(),
    };
    send(world, crate::market::Call::Account(call), request);
}

/// Opens `url` in the browser.
pub(crate) fn browse(world: &mut World, url: &str) {
    world.desktop(&DesktopRequest::Open(url.to_string()));
}

/// Begins signing in to the bazaar with GitHub.
pub(crate) fn sign_in(world: &mut World) {
    world.resource_mut::<Market>().account = Account::Asking;
    ask(world, Call::Device);
}

/// Signs out of the bazaar, or stops a sign-in that has begun.
pub(crate) fn sign_out(world: &mut World) {
    let signed = world.resource::<Market>().signed_in();
    if signed {
        // The server forgets the token too; nothing waits for it.
        ask(world, Call::SignOut);
    }
    forget(world);
    crate::wardrobe::signed_out(world);
}

/// Forgets the one signed in, here and for the next run.
fn forget(world: &mut World) {
    let mut market = world.resource_mut::<Market>();
    market.server.token = None;
    market.account = Account::Out;
    market.library.clear();
    market.mine.clear();
    for detail in market.details.values_mut() {
        detail.owned = false;
    }
    session::keep(world, None);
}

/// Asks who is signed in, what they own and what they sell.
pub(crate) fn ask_account(world: &mut World) {
    if !world.resource::<Market>().signed_in() {
        return;
    }
    ask(world, Call::Me);
    ask(world, Call::Library);
    ask(world, Call::Mine);
}

/// Opens Stripe's page where the chief sets up how they are paid.
pub(crate) fn set_up_payouts(world: &mut World) {
    if !world.resource::<Market>().signed_in() {
        return world
            .resource_mut::<Market>()
            .say("Sign in with GitHub first.", true);
    }
    ask(world, Call::Payouts);
}

/// Asks whether the code was entered, when that is due.
fn poll(world: &mut World) {
    let now = now();
    // Read first, so that the views are not looked at again for
    // nothing.
    let Account::Code { asking, .. } =
        &world.resource::<Market>().account
    else {
        return;
    };
    if !asking.over(now) && !asking.due(now) {
        return;
    }
    let mut market = world.resource_mut::<Market>();
    let Account::Code { device, asking } = &mut market.account else {
        return;
    };
    if asking.over(now) {
        market.account = Account::Failed(
            "The code ran out. Sign in again.".into(),
        );
        return;
    }
    if !asking.due(now) {
        return;
    }
    asking.out = true;
    let code = device.device_code.clone();
    ask(world, Call::Poll(code));
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    reply: Result<Reply, Failure>,
) {
    match call {
        Call::Device => match read::<Device>(reply) {
            Ok(device) => {
                browse(world, &device.verification_uri);
                let asking = Asking::new(
                    now(),
                    device.interval.max(1) * 1000,
                    device.expires_in * 1000,
                );
                world.resource_mut::<Market>().account =
                    Account::Code { device, asking };
            }
            Err(why) => {
                world.resource_mut::<Market>().account =
                    Account::Failed(why.words())
            }
        },
        Call::Poll(code) => signing(world, &code, polled(reply)),
        Call::SignOut => {}
        Call::Me => match read::<User>(reply) {
            Ok(user) => {
                world.resource_mut::<Market>().account =
                    Account::In(user)
            }
            Err(why) => refused(world, why),
        },
        Call::Library => match read::<Page<Owned>>(reply) {
            Ok(page) => {
                world.resource_mut::<Market>().library = page
                    .items
                    .into_iter()
                    .map(|owned| owned.listing)
                    .collect()
            }
            Err(why) => refused(world, why),
        },
        Call::Mine => match read::<Page<Lot>>(reply) {
            Ok(page) => {
                world.resource_mut::<Market>().mine = page.items
            }
            Err(why) => refused(world, why),
        },
        Call::Payouts => {
            let said = match read::<Link>(reply) {
                Ok(link) => {
                    browse(world, &link.url);
                    (
                        "Stripe opened in your browser. Come back \
                         once you have finished there, and choose \
                         CHECK AGAIN."
                            .to_string(),
                        false,
                    )
                }
                Err(why) => (why.words(), true),
            };
            world.resource_mut::<Market>().said = Some(said);
        }
    }
}

/// Takes in what came of asking after the sign-in of `code`.
fn signing(world: &mut World, code: &str, polled: Polled) {
    let now = now();
    let mut market = world.resource_mut::<Market>();
    // A sign-in stopped, or begun again, is not heard.
    let Account::Code { device, asking } = &mut market.account else {
        return;
    };
    if device.device_code != code {
        return;
    }
    asking.heard(now);
    match polled {
        Polled::Pending => {}
        Polled::SlowDown => asking.every += SLOWER * 1000,
        Polled::Failed(why) => market.account = Account::Failed(why),
        Polled::Approved(Approved { token, user }) => {
            market.server.token = Some(token.clone());
            market.account = Account::In(user);
            session::keep(world, Some(&token));
            ask_account(world);
            ask_shelves(world);
        }
    }
}

/// Takes in a refusal of the account's: a token refused signs out.
fn refused(world: &mut World, why: crate::market::client::Refusal) {
    if why.signed_out() {
        forget(world);
        return world.resource_mut::<Market>().say(
            "Your sign-in at the bazaar ran out. Sign in again.",
            true,
        );
    }
    world.resource_mut::<Market>().say(why.words(), true);
}
