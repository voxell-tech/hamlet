//! The bazaar end to end against a local bazaar server, driven as the
//! window drives it, with the home's server played by hand: two
//! accounts, the vault, the seller terms, an agent uploaded as a
//! draft and published unverified, bought, downloaded and reported,
//! and the LOOKS stall's free and paid looks. Nothing is paid out:
//! payouts need Stripe Connect.
//!
//! The bazaar's server is a separate service. Start it locally with
//! its moderation faux, its looks loaded and its log written to a
//! file, then name its checkout, where the test makes an account old
//! enough to sell:
//! `HAMLET_SERVER_DIR=<checkout> HAMLET_E2E_LOG=<server.log> cargo
//! test -p hamlet_app --lib live -- --ignored`
//! Without `HAMLET_SERVER_DIR` the test says so and passes.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bevy::ecs::world::World;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Providers;
use hamlet_client::{Tokio, ViewFolder};
use hamlet_protocol::client as protocol;
use hamlet_protocol::vault::DataKey;

use super::check::{Reporting, Watch};
use super::deal::{Dealing, Places};
use super::lot::{Checkout, Kind, Lot};
use super::sell::{Offer, Selling, Step};
use super::*;
use crate::account::{Form, Page};
use crate::bazaar::errand::{self, Errands, fetch};
use crate::bazaar::listings::{Agents, Hamlets, Tools, Wares};
use crate::bazaar::shelf::Stock;

const SERVER: &str = "http://localhost:8787";

/// The longest anything is waited for, the checks among them.
const PATIENCE: Duration = Duration::from_secs(90);

const PASSWORD: &str = "a long password";

/// The OpenRouter key the first home has.
const KEY: &str = "sk-or-v1-e2e";

#[test]
#[ignore = "needs the bazaar's server on localhost:8787"]
#[expect(clippy::print_stderr, reason = "it says why it is skipped")]
fn the_bazaar_works_end_to_end_against_a_local_server() {
    if server_checkout().is_none() {
        eprintln!(
            "skipped: set HAMLET_SERVER_DIR to the bazaar server's \
             checkout to run this test"
        );
        return;
    }
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let run = now();
    let seller_email = format!("seller-{run}@e2e.example.com");
    let buyer_email = format!("buyer-{run}@e2e.example.com");

    // A seller signs up, verifies, chooses a username and signs in
    // again; their home seals its OpenRouter key into the vault.
    let mut seller = chief(&runtime, "seller", true);
    sign_up(&mut seller, &seller_email, &format!("s{run}"));
    rename(&mut seller, &format!("seller-{run}"));
    let data = seller.resource::<Market>().data_key.clone().unwrap();
    let sealed = play_home_sealing(&mut seller);
    assert_eq!(data.unseal(&sealed).unwrap(), KEY);
    sign_out(&mut seller);
    sign_in(&mut seller, &seller_email);

    // On another machine with no key, the vault opens with the
    // password and the home is handed the key, unsealed as it does.
    let mut other = chief(&runtime, "other", false);
    sign_in(&mut other, &seller_email);
    let unseal = outgoing(&mut other, |message| match message {
        protocol::ClientMessage::Login(protocol::Login::Unseal {
            unseal,
            ..
        }) => Some(unseal),
        _ => None,
    });
    let opened = DataKey::from_base64(&unseal.data_key).unwrap();
    assert_eq!(opened.unseal(&unseal.sealed).unwrap(), KEY);
    // The window's next run signs in with the session it kept.
    let mut again = chief(&runtime, "other", false);
    session::restore(&mut again);
    assert!(again.resource::<Market>().signed_in());
    assert!(again.resource::<Market>().data_key.is_some());

    // The seller agrees to the seller terms.
    read_terms(&mut seller);
    pump(&mut seller, "the seller terms", |world| {
        world.resource::<Market>().terms.is_some()
    });
    agree(&mut seller);
    pump(&mut seller, "the terms agreed to", |world| {
        world
            .resource::<Market>()
            .user()
            .is_some_and(|user| user.seller_terms.is_some())
    });

    // An agent is packed and uploaded as a draft. Too new an account
    // cannot publish it, and is told from when it can.
    let title = format!("Clock {run}");
    sell_agent(&mut seller, &title);
    let Step::Done(id, version) = finished_sale(&mut seller) else {
        panic!("the agent was not uploaded");
    };
    assert_eq!(version, sell::FIRST_VERSION);
    let which = drafts::Which {
        id: id.clone(),
        version,
    };
    let said = publish(&mut seller, &which);
    assert!(
        said.contains("7 days old") && said.contains("sell from")
    );

    // Made 8 days ago, it publishes the draft unverified, and it is
    // on the shelves.
    backdate(&seller_email);
    let said = publish(&mut seller, &which);
    assert!(said.contains("live, unverified"), "{said}");
    let listed = mine(&mut seller, &id);
    assert!(
        listed
            .versions
            .iter()
            .any(|one| one.state == versions::State::Unverified)
    );
    pump_shelves(&mut seller, &id);
    let listed =
        seller.resource::<Market>().lot(&id).cloned().unwrap();

    // A buyer gets the free listing, downloads it and has it set up,
    // then reports it.
    let mut buyer = chief(&runtime, "buyer", false);
    sign_up(&mut buyer, &buyer_email, &format!("b{run}"));
    pump_shelves(&mut buyer, &id);
    buy(&mut buyer, &id);
    pump(&mut buyer, "the package", |world| {
        matches!(
            world.resource::<Dealing>().deal,
            Some((_, Deal::Settling))
        )
    });
    let settle = outgoing(&mut buyer, |message| match message {
        protocol::ClientMessage::Settle(settle) => Some(settle),
        _ => None,
    });
    let version = listed.latest.unwrap();
    assert_eq!(settle.size, version.size);
    deal::heard(
        &mut buyer,
        &Response {
            to: settle.id.clone(),
            message: protocol::ServerMessage::Reply(
                protocol::Reply {
                    kind: protocol::ReplyKind::Notice,
                    text: "clock joined every agent.".into(),
                    to: settle.id,
                    upload: None,
                },
            ),
        },
    );
    assert_eq!(
        buyer
            .resource::<Dealing>()
            .deal
            .as_ref()
            .map(|(_, deal)| deal),
        Some(&Deal::Done("clock joined every agent.".into()))
    );
    pump(&mut buyer, "the library", |world| {
        world.resource::<Market>().owns(&id)
    });
    write_report(&mut buyer, Some(&id));
    buyer.resource_mut::<Reporting>().reason =
        "It tells the wrong time.".into();
    report(&mut buyer);
    pump(&mut buyer, "the report", |world| {
        world.resource::<Reporting>().said.is_some()
    });
    let said = buyer.resource::<Reporting>().said.clone().unwrap();
    assert!(!said.2, "{}", said.1);

    // The buyer gets a free look, and Stripe's page for a paid one.
    let server = buyer.resource::<Market>().server.clone();
    let asked = |request| {
        runtime.block_on(async {
            fetch(&reqwest::Client::new(), request).await
        })
    };
    let free = read::<Checkout>(asked(
        server.asset_purchase("agent-emote-wave"),
    ))
    .unwrap();
    assert!(free.owned);
    let paid = read::<Checkout>(asked(
        server.asset_purchase("agent-emote-bow"),
    ))
    .unwrap();
    assert!(paid.checkout_url.is_some_and(|url| {
        url.starts_with("https://checkout.stripe.com")
    }));
}

/// A window of its own, its fetches made on `runtime`, whose home has
/// an OpenRouter key when `keyed`.
fn chief(
    runtime: &tokio::runtime::Runtime,
    name: &str,
    keyed: bool,
) -> World {
    let mut world = World::new();
    world.insert_resource(Market::new(SERVER.into()));
    world.insert_resource(Tokio(runtime.handle().clone()));
    let view = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/bazaar-e2e")
        .join(name);
    world.insert_resource(ViewFolder(view));
    world.init_resource::<Errands>();
    world.init_resource::<Outbox>();
    world.init_resource::<Form>();
    world.init_resource::<Stock<Wares<Agents>>>();
    world.init_resource::<Stock<Wares<Tools>>>();
    world.init_resource::<Stock<Wares<Hamlets>>>();
    world.init_resource::<Selling>();
    world.init_resource::<drafts::Drafts>();
    world.init_resource::<Dealing>();
    world.init_resource::<Places>();
    world.init_resource::<vault::Syncing>();
    world.init_resource::<Reporting>();
    world.init_resource::<Watch>();
    let mut providers = Providers::default();
    providers.0.push(protocol::ProviderView {
        configured: keyed,
        id: "openrouter".into(),
        label: String::new(),
        methods: Vec::new(),
        name: "OpenRouter".into(),
    });
    world.insert_resource(providers);
    world
}

/// Takes in the answers that came until `done` holds of `world`.
fn pump(
    world: &mut World,
    what: &str,
    done: impl Fn(&World) -> bool,
) {
    let start = Instant::now();
    loop {
        errand::answered(world);
        if done(world) {
            return;
        }
        assert!(start.elapsed() < PATIENCE, "no {what} came");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Takes in every answer, until none is waited for.
fn settle_down(world: &mut World) {
    pump(world, "answer", |world| {
        world.resource::<Errands>().is_idle()
    });
}

fn sign_up(world: &mut World, email: &str, username: &str) {
    {
        let mut form = world.resource_mut::<Form>();
        form.email = email.into();
        form.password = PASSWORD.into();
        form.again = PASSWORD.into();
        form.username = username.into();
    }
    account::sign_up(world);
    pump(world, "code page", |world| {
        world.resource::<Form>().page == Page::Code
    });
    let code = code_for(email);
    world.resource_mut::<Form>().code = code;
    verify(world);
    pump(world, "sign-in and vault", |world| {
        world.resource::<Market>().data_key.is_some()
    });
    settle_down(world);
}

fn rename(world: &mut World, username: &str) {
    world.resource_mut::<Form>().username = username.into();
    choose_username(world);
    pump(world, "username", |world| {
        world
            .resource::<Market>()
            .user()
            .and_then(|user| user.username.as_deref())
            == Some(username)
    });
}

fn sign_in(world: &mut World, email: &str) {
    {
        let mut form = world.resource_mut::<Form>();
        form.email = email.into();
        form.password = PASSWORD.into();
    }
    account::sign_in(world);
    pump(world, "sign-in and vault", |world| {
        world.resource::<Market>().data_key.is_some()
    });
    settle_down(world);
}

/// The 6-digit code the dev server last logged for `email`.
fn code_for(email: &str) -> String {
    let log = std::env::var("HAMLET_E2E_LOG")
        .expect("HAMLET_E2E_LOG names the dev server's log");
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let code = text
            .lines()
            .filter(|line| {
                line.contains(&format!("to={email} "))
                    && line.contains("kind=sign-up-code")
            })
            .filter_map(|line| line.split("code=").nth(1))
            .map(|code| {
                code.trim().chars().take(6).collect::<String>()
            })
            .next_back();
        if let Some(code) = code {
            return code;
        }
        assert!(start.elapsed() < PATIENCE, "no code for {email}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The first request the window queued for its home that `pick`
/// takes.
fn outgoing<T>(
    world: &mut World,
    pick: impl Fn(protocol::ClientMessage) -> Option<T>,
) -> T {
    let mut outbox = world.resource_mut::<Outbox>();
    outbox
        .drain()
        .collect::<Vec<_>>()
        .into_iter()
        .find_map(pick)
        .expect("the window asked its home nothing of the kind")
}

/// Seals the home's key with the data key the window hands it, as
/// the home does, and answers; the sealed key the vault keeps.
fn play_home_sealing(world: &mut World) -> String {
    let (id, data_key) = outgoing(world, |message| match message {
        protocol::ClientMessage::Read(protocol::Read::Sealed {
            id,
            data_key,
        }) => Some((id, data_key)),
        _ => None,
    });
    let sealed = DataKey::from_base64(&data_key).unwrap().seal(KEY);
    vault::heard(
        world,
        &Response {
            to: id.clone(),
            message: protocol::ServerMessage::Read(
                protocol::ReadResult::Sealed {
                    sealed: Some(sealed.clone()),
                    to: id,
                },
            ),
        },
    );
    settle_down(world);
    sealed
}

/// Packs an agent as the home packs one, and uploads it as a new
/// listing of `title`.
fn sell_agent(world: &mut World, title: &str) {
    sell(
        world,
        Offer {
            id: None,
            kind: Kind::Agent,
            title: title.into(),
            wares: protocol::Packing::Agent {
                agent: protocol::Address {
                    agent: "clock".into(),
                    hamlet: "home".into(),
                },
                notes: false,
            },
            version: sell::FIRST_VERSION.into(),
        },
    );
    let request = outgoing(world, |message| match message {
        protocol::ClientMessage::Read(protocol::Read::Package {
            id,
            ..
        }) => Some(id),
        _ => None,
    });
    let package = serde_json::to_vec(&serde_json::json!({
        "format": 1, "kind": "agent", "agent": {
            "name": "clock",
            "files": [{
                "path": "CHARACTER.md",
                "data": STANDARD.encode(format!("{title} tells the time.")),
            }],
        },
    }))
    .unwrap();
    sell::heard(
        world,
        &Response {
            to: request.clone(),
            message: protocol::ServerMessage::Read(
                protocol::ReadResult::Package {
                    package: protocol::PackagePart {
                        data: STANDARD.encode(&package),
                        offset: 0,
                        size: package.len() as u64,
                    },
                    to: request,
                },
            ),
        },
    );
}

/// Publishes the draft `which` unverified, and says what came of it.
fn publish(world: &mut World, which: &drafts::Which) -> String {
    drafts::go_on(world, which.clone(), drafts::Then::Publish);
    pump(world, "the draft published", |world| {
        world.resource::<drafts::Drafts>().job.is_none()
    });
    world.resource::<drafts::Drafts>().said.clone().unwrap().0
}

/// How the sale going on ended.
fn finished_sale(world: &mut World) -> Step {
    pump(world, "end of the sale", |world| {
        world
            .resource::<Selling>()
            .0
            .as_ref()
            .is_some_and(|sale| !sale.step.going())
    });
    settle_down(world);
    world.resource::<Selling>().0.clone().unwrap().step
}

/// The chief's listing `id`, as the server says now.
fn mine(world: &mut World, id: &str) -> Lot {
    ask_mine(world);
    settle_down(world);
    world
        .resource::<Market>()
        .mine
        .iter()
        .find(|lot| lot.id == id)
        .cloned()
        .unwrap_or_else(|| panic!("{id} is not the chief's"))
}

/// Asks for the shelves until the listing `id` is on them.
fn pump_shelves(world: &mut World, id: &str) {
    ask_shelves(world);
    pump(world, "the listing on the shelves", |world| {
        world.resource::<Market>().reach == Reach::Reached
            && !world.resource::<Market>().asking
    });
    assert!(
        world
            .resource::<Market>()
            .lots
            .iter()
            .any(|lot| lot.id == id),
        "{id} is not on the shelves"
    );
}

/// The bazaar server's checkout, from `HAMLET_SERVER_DIR`.
fn server_checkout() -> Option<PathBuf> {
    std::env::var_os("HAMLET_SERVER_DIR").map(PathBuf::from)
}

/// Makes the account of `email` 8 days old in the local database.
fn backdate(email: &str) {
    let server = server_checkout()
        .expect("HAMLET_SERVER_DIR names the checkout");
    let sql = format!(
        "UPDATE \"User\" SET createdAt = CASE typeof(createdAt) \
         WHEN 'integer' THEN createdAt - 8 * 86400000 ELSE \
         strftime('%Y-%m-%dT%H:%M:%fZ', createdAt, '-8 days') END \
         WHERE email = '{email}'"
    );
    let done = std::process::Command::new("pnpm")
        .args(["exec", "wrangler", "d1", "execute", "DB", "--local"])
        .args(["--command", &sql])
        .current_dir(server)
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
}
