//! The chief's Hamlet account at the bazaar's server: signing up with
//! a code sent to their email, signing in and out, a forgotten
//! password, their username and password changed, who they are,
//! what they own and sell, and how they are paid.

use bevy::ecs::world::World;
use serde::Deserialize;

use crate::account::{Form, Page, Place, shut, turn};
use crate::bazaar::errand::Fetched;
use crate::bazaar::market::client::{Refusal, done, read};
use crate::bazaar::market::lot::{
    Agreed, Link, Lot, Owned, Page as Listed, Payouts, Signed, Terms,
    User,
};
use crate::bazaar::market::{
    Market, ask_shelves, send, session, vault,
};
use crate::welcome::{Step, Welcome};

/// The fewest characters of a password.
const PASSWORD: usize = 10;

/// The chief at the bazaar.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum Account {
    #[default]
    Out,
    In(User),
}

/// A call of the server about the account.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    SignUp,
    Verify,
    Resend,
    SignIn,
    Forgot,
    Reset,
    SignOut,
    /// The username typed, chosen.
    Rename(String),
    Password,
    Me,
    Library,
    Mine,
    /// Stripe's page to set up payouts on.
    Payouts,
    /// Whether Stripe lets the chief be paid.
    Paid,
    Terms,
    /// The seller terms of this version agreed to.
    Agree(String),
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

fn ask(world: &mut World, call: Call) {
    let request = {
        let server = &world.resource::<Market>().server;
        let form = world.resource::<Form>();
        let email = form.email.trim();
        match &call {
            Call::SignUp => server.sign_up(
                email,
                &form.password,
                form.username.trim(),
            ),
            Call::Verify => server.verify(email, form.code.trim()),
            Call::Resend => server.resend(email),
            Call::SignIn => server.sign_in(email, &form.password),
            Call::Forgot => server.forgot(email),
            Call::Reset => {
                server.reset(email, form.code.trim(), &form.fresh)
            }
            Call::SignOut => server.sign_out(),
            Call::Rename(username) => server.rename(username),
            Call::Password => {
                server.change_password(&form.password, &form.fresh)
            }
            Call::Me => server.me(),
            Call::Library => server.library(),
            Call::Mine => server.mine(),
            Call::Payouts => server.payouts(),
            Call::Paid => server.payouts_state(),
            Call::Terms => server.seller_terms(),
            Call::Agree(version) => server.agree(version),
        }
    };
    send(world, crate::bazaar::market::Call::Account(call), request);
}

/// Opens `url` in the browser.
pub(crate) fn browse(_world: &mut World, url: &str) {
    hamlet_desktop::open::browse(url);
}

/// Asks `call` of the form's fields, or says why the form `checks`
/// they will not do.
fn asked(
    world: &mut World,
    checks: fn(&Form) -> Result<(), &'static str>,
    call: Call,
) {
    let mut form = world.resource_mut::<Form>();
    if let Err(why) = checks(&form) {
        form.problem = Some(why.into());
        form.said = None;
        return;
    }
    form.asking = true;
    form.problem = None;
    form.said = None;
    ask(world, call);
}

fn has_email(form: &Form) -> Result<(), &'static str> {
    match form.email.trim().contains('@') {
        true => Ok(()),
        false => Err("Type your email."),
    }
}

/// The username typed is one the server takes.
fn username(form: &Form) -> Result<(), &'static str> {
    let name = form.username.trim();
    let fits = (3..=24).contains(&name.len())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'-'
                || b == b'_'
        });
    match fits {
        true => Ok(()),
        false => {
            Err("Use 3 to 24 lowercase letters, digits, - or _.")
        }
    }
}

/// `password` is long enough, and typed the same `again`.
fn new_password(
    password: &str,
    again: &str,
) -> Result<(), &'static str> {
    if password.chars().count() < PASSWORD {
        return Err("Make the password at least 10 characters.");
    }
    match password == again {
        true => Ok(()),
        false => Err("The two passwords are not the same."),
    }
}

/// Makes a Hamlet account of the email and password typed.
pub(crate) fn sign_up(world: &mut World) {
    asked(
        world,
        |form| {
            has_email(form)?;
            username(form)?;
            new_password(&form.password, &form.again)
        },
        Call::SignUp,
    );
}

/// Sends the code typed, to verify the email.
pub(crate) fn verify(world: &mut World) {
    asked(
        world,
        |form| match form.code.trim().len() {
            6 => Ok(()),
            _ => Err("Type the 6 digits of the code."),
        },
        Call::Verify,
    );
}

/// Asks for another code.
pub(crate) fn resend(world: &mut World) {
    asked(world, has_email, Call::Resend);
}

pub(crate) fn sign_in(world: &mut World) {
    asked(
        world,
        |form| {
            has_email(form)?;
            match form.password.is_empty() {
                true => Err("Type your password."),
                false => Ok(()),
            }
        },
        Call::SignIn,
    );
}

/// Asks for a code to choose a new password with.
pub(crate) fn forgot(world: &mut World) {
    asked(world, has_email, Call::Forgot);
}

/// Chooses the new password typed, with its code.
pub(crate) fn reset(world: &mut World) {
    asked(
        world,
        |form| match form.code.trim().len() {
            6 => new_password(&form.fresh, &form.again),
            _ => Err("Type the 6 digits of the code."),
        },
        Call::Reset,
    );
}

/// Chooses the username typed.
pub(crate) fn choose_username(world: &mut World) {
    let name = world.resource::<Form>().username.trim().to_string();
    asked(world, username, Call::Rename(name));
}

/// Changes the password from the one typed to the new one.
pub(crate) fn change_password(world: &mut World) {
    asked(
        world,
        |form| match form.password.is_empty() {
            true => Err("Type your password."),
            false => new_password(&form.fresh, &form.again),
        },
        Call::Password,
    );
}

/// Signs out of the account. The home keeps its OpenRouter key.
pub(crate) fn sign_out(world: &mut World) {
    if world.resource::<Market>().signed_in() {
        // The server ends the session too; nothing waits for it.
        ask(world, Call::SignOut);
    }
    forget(world);
    shut(world);
}

/// Forgets the one signed in, here and for the next run.
fn forget(world: &mut World) {
    let mut market = world.resource_mut::<Market>();
    market.server.token = None;
    market.data_key = None;
    market.account = Account::Out;
    market.terms = None;
    market.library.clear();
    market.mine.clear();
    for detail in market.details.values_mut() {
        detail.owned = false;
    }
    session::keep(world);
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

/// Asks Stripe, through the server, whether the chief can be paid.
pub(crate) fn check_payouts(world: &mut World) {
    if world.resource::<Market>().signed_in() {
        ask(world, Call::Paid);
    }
}

/// Opens Stripe's page where the chief sets up how they are paid,
/// once they have agreed to the seller terms.
pub(crate) fn set_up_payouts(world: &mut World) {
    let market = world.resource::<Market>();
    let Some(user) = market.user() else {
        return world
            .resource_mut::<Market>()
            .say("Sign in to your Hamlet account first.", true);
    };
    match user.seller_terms.is_some() {
        true => ask(world, Call::Payouts),
        false => {
            world.resource_mut::<Market>().paying = true;
            ask(world, Call::Terms);
        }
    }
}

/// Shows the seller terms to agree to before anything is published.
pub(crate) fn read_terms(world: &mut World) {
    if world.resource::<Market>().signed_in() {
        world.resource_mut::<Market>().paying = false;
        ask(world, Call::Terms);
    }
}

/// Agrees to the seller terms shown, then sets up payouts.
pub(crate) fn agree(world: &mut World) {
    let version = world
        .resource::<Market>()
        .terms
        .as_ref()
        .map(|terms| terms.version.clone());
    if let Some(version) = version {
        ask(world, Call::Agree(version));
    }
}

/// Puts the seller terms away unagreed.
pub(crate) fn disagree(world: &mut World) {
    world.resource_mut::<Market>().terms = None;
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    reply: Fetched,
) {
    let email = world.resource::<Form>().email.trim().to_string();
    match call {
        Call::SignUp => match done(reply) {
            Ok(()) => turn(
                world,
                Page::Code,
                Some(format!("We sent a code to {email}.")),
            ),
            Err(why) => failed(world, &why),
        },
        Call::Verify => match read::<Signed>(reply) {
            Ok(signed) => signed_in(world, signed),
            Err(why) => failed(world, &why),
        },
        Call::SignIn => match read::<Signed>(reply) {
            Ok(signed) => signed_in(world, signed),
            Err(why) if why.code == "EMAIL_NOT_VERIFIED" => turn(
                world,
                Page::Code,
                Some(format!(
                    "Your email is not verified yet. We sent a code \
                     to {email}."
                )),
            ),
            Err(why) => failed(world, &why),
        },
        Call::Resend => match done(reply) {
            Ok(()) => turn(
                world,
                Page::Code,
                Some(format!("We sent a new code to {email}.")),
            ),
            Err(why) => failed(world, &why),
        },
        Call::Forgot => match done(reply) {
            Ok(()) => turn(
                world,
                Page::Reset,
                Some(format!(
                    "If {email} has a Hamlet account, we sent a \
                     code to it."
                )),
            ),
            Err(why) => failed(world, &why),
        },
        Call::Reset => match done(reply) {
            Ok(()) => {
                let mut form = world.resource_mut::<Form>();
                form.password = std::mem::take(&mut form.fresh);
                ask(world, Call::SignIn);
            }
            Err(why) => failed(world, &why),
        },
        Call::SignOut => {}
        Call::Rename(username) => match done(reply) {
            Ok(()) => {
                if let Account::In(user) =
                    &mut world.resource_mut::<Market>().account
                {
                    user.username = Some(username.clone());
                }
                session::keep(world);
                let renamed =
                    world.resource::<Form>().page == Page::Rename;
                finished(
                    world,
                    renamed.then(|| {
                        format!("Your username is {username}.")
                    }),
                );
            }
            Err(why) => failed(world, &why),
        },
        Call::Password => match read::<Changed>(reply) {
            Ok(changed) => {
                if let Some(token) = changed.token {
                    world.resource_mut::<Market>().server.token =
                        Some(token);
                    session::keep(world);
                }
                let fresh = std::mem::take(
                    &mut world.resource_mut::<Form>().fresh,
                );
                vault::relock(world, fresh);
                finished(
                    world,
                    Some("Your password is changed.".into()),
                );
            }
            Err(why) => failed(world, &why),
        },
        Call::Me => match read::<User>(reply) {
            Ok(user) => {
                world.resource_mut::<Market>().account =
                    Account::In(user);
                session::keep(world);
            }
            Err(why) => refused(world, why),
        },
        Call::Library => match read::<Listed<Owned>>(reply) {
            Ok(page) => {
                world.resource_mut::<Market>().library = page
                    .items
                    .into_iter()
                    .map(|owned| owned.listing)
                    .collect();
            }
            Err(why) => refused(world, why),
        },
        Call::Mine => match read::<Listed<Lot>>(reply) {
            Ok(page) => {
                world.resource_mut::<Market>().mine = page.items;
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
        Call::Paid => match read::<Payouts>(reply) {
            Ok(payouts) => {
                if let Account::In(user) =
                    &mut world.resource_mut::<Market>().account
                {
                    user.payouts = Some(payouts);
                }
            }
            Err(why) => refused(world, why),
        },
        Call::Terms => match read::<Terms>(reply) {
            Ok(terms) => {
                world.resource_mut::<Market>().terms = Some(terms);
            }
            Err(why) => refused(world, why),
        },
        Call::Agree(version) => match done(reply) {
            Ok(()) => {
                let mut market = world.resource_mut::<Market>();
                market.terms = None;
                if let Account::In(user) = &mut market.account {
                    user.seller_terms = Some(Agreed { version });
                }
                match std::mem::take(&mut market.paying) {
                    true => ask(world, Call::Payouts),
                    false => market.say(
                        "You agreed to the seller terms. You can \
                         publish now.",
                        false,
                    ),
                }
            }
            Err(why) => refused(world, why),
        },
    }
}

/// What changing the password answers.
#[derive(Deserialize)]
struct Changed {
    #[serde(default)]
    token: Option<String>,
}

/// Takes in a sign-in that went through: keeps its token, opens the
/// account's vault with the password typed, and asks for a username
/// if the account has none yet.
fn signed_in(world: &mut World, signed: Signed) {
    let Signed { token, user } = signed;
    let named = user.username.is_some();
    {
        let mut market = world.resource_mut::<Market>();
        market.server.token = Some(token);
        market.data_key = None;
        market.account = Account::In(user);
        market.said = None;
    }
    session::keep(world);
    let password =
        std::mem::take(&mut world.resource_mut::<Form>().password);
    vault::open(world, password);
    ask_account(world);
    ask_shelves(world);
    match named {
        true => finished(world, None),
        false => turn(world, Page::Username, None),
    }
}

/// Shuts the account's pages once what they were for is done, and
/// moves first-run setup on to its models; `said` stays to be read.
fn finished(world: &mut World, said: Option<String>) {
    let at = world.resource::<Form>().at;
    shut(world);
    world.resource_mut::<Form>().said = said;
    if at == Place::Welcome
        && let Some(mut welcome) = world.get_resource_mut::<Welcome>()
        && welcome.step == Step::Account
    {
        welcome.step = Step::Choice;
    }
}

/// Says on the form why what it asked did not work.
fn failed(world: &mut World, why: &Refusal) {
    let mut form = world.resource_mut::<Form>();
    form.asking = false;
    form.said = None;
    form.problem = Some(why.words());
}

/// Takes in a refusal of the account's: a token refused signs out.
/// A server not reached is said at the foot of the shelves.
pub(crate) fn refused(world: &mut World, why: Refusal) {
    if why.status.is_none() {
        return;
    }
    if why.signed_out() {
        forget(world);
        return world.resource_mut::<Market>().say(
            "Your sign-in to your Hamlet account ran out. Sign in \
             again.",
            true,
        );
    }
    world.resource_mut::<Market>().say(why.words(), true);
}
