//! Selling at the bazaar: an agent, a hamlet or tools packed by the
//! home's server and handed to the window in parts, the listing put
//! up or changed with its words and price, its cover sent, the
//! package uploaded as its new version, and the listing put on the
//! shelves; and a listing of the chief's put on the shelves or taken
//! off. One sale goes at a time.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;

use crate::bazaar::errand::Fetched;
use crate::bazaar::market::client::{Refusal, done, read};
use crate::bazaar::market::lot::{Draft, Lot, Status};
use crate::bazaar::market::{
    Market, ask_account, ask_shelves, refused, send,
};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Selling>().init_resource::<Housed>();
}

/// The tools added by hand that can be sold, as the home's server
/// last told, or why it could not; and the read asking for them.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Housed {
    pub tools: Option<Result<Vec<protocol::PackableTool>, String>>,
    asked: Option<protocol::RequestId>,
}

/// Something put up for sale, or a listing of the chief's changed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Offer {
    /// The listing changed; `None` for a new one.
    pub id: Option<String>,
    pub draft: Draft,
    /// What is packed as its new version, if one is uploaded.
    pub wares: Option<protocol::Packing>,
    pub version: String,
    pub changes: String,
    /// A picture for its cover and its media type.
    pub cover: Option<(Vec<u8>, String)>,
    /// Put on the shelves once done.
    pub publish: bool,
}

/// How putting something up for sale goes.
#[derive(Resource, Clone, Debug, PartialEq, Default)]
pub(crate) struct Selling(pub Option<Sale>);

/// A sale and the step it is on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sale {
    pub offer: Offer,
    pub step: Step,
    /// The package packed by the home's server, as far as it came.
    package: Option<Vec<u8>>,
    /// The listing, once it is put up or changed.
    listing: Option<Lot>,
}

/// A step of a sale.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    /// The home's server packs it, answering this read.
    Packing(protocol::RequestId),
    /// The listing is put up or changed.
    Saving,
    Covering,
    /// The package goes up as its new version.
    Uploading,
    Publishing,
    /// Saved, as the listing of this title, now in this state.
    Done(String, Status),
    Failed(String),
}

impl Step {
    pub fn going(&self) -> bool {
        !matches!(self, Step::Done(..) | Step::Failed(_))
    }
}

/// A call of the server for the sale going on, or a listing put on
/// the shelves or taken off.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    Save,
    Cover,
    Release,
    Publish,
    /// The listing of this id put on the shelves, or taken off.
    Shelve(String, bool),
}

/// Asks the home's server for the tools that can be sold.
pub(crate) fn ask_tools(world: &mut World) {
    let request = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Tools { id })
    });
    world.resource_mut::<Housed>().asked = Some(request);
}

fn going(world: &World) -> bool {
    world
        .resource::<Selling>()
        .0
        .as_ref()
        .is_some_and(|sale| sale.step.going())
}

/// Packs `offer`, then sends it to the bazaar.
pub(crate) fn sell(world: &mut World, offer: Offer) {
    if going(world) {
        return;
    }
    let signed = world.resource::<Market>().signed_in();
    let mut package = None;
    let first = match (&offer.wares, signed) {
        (_, false) => {
            Step::Failed("Sign in with GitHub first.".into())
        }
        (Some(wares), true) => {
            let wares = wares.clone();
            let request = world.resource_mut::<Outbox>().send(|id| {
                protocol::ClientMessage::Read(
                    protocol::Read::Package { id, of: wares },
                )
            });
            package = Some(Vec::new());
            Step::Packing(request)
        }
        (None, true) => Step::Saving,
    };
    let save = first == Step::Saving;
    world.resource_mut::<Selling>().0 = Some(Sale {
        offer,
        step: first,
        package,
        listing: None,
    });
    if save {
        ask(world, Call::Save);
    }
}

fn ask(world: &mut World, call: Call) {
    let server = &world.resource::<Market>().server;
    let sale = &world.resource::<Selling>().0;
    let id = sale
        .as_ref()
        .and_then(|sale| sale.listing.as_ref())
        .map(|lot| lot.id.clone())
        .unwrap_or_default();
    let request = match (&call, sale) {
        (Call::Shelve(id, true), _) => server.publish(id),
        (Call::Shelve(id, false), _) => server.unlist(id),
        (_, None) => return,
        (Call::Save, Some(sale)) => match &sale.offer.id {
            Some(id) => server.change(id, &sale.offer.draft),
            None => server.create(&sale.offer.draft),
        },
        (Call::Cover, Some(sale)) => {
            let Some((bytes, kind)) = sale.offer.cover.clone() else {
                return;
            };
            server.cover(&id, bytes, &kind)
        }
        (Call::Release, Some(sale)) => {
            let Some(package) = &sale.package else {
                return;
            };
            server.release(
                &id,
                &sale.offer.version,
                &sale.offer.changes,
                package,
            )
        }
        (Call::Publish, Some(_)) => server.publish(&id),
    };
    send(world, crate::bazaar::market::Call::Sell(call), request);
}

/// Puts the listing `id` of the chief's on the shelves, or takes it
/// off.
pub(crate) fn shelve(world: &mut World, id: &str, on: bool) {
    ask(world, Call::Shelve(id.to_string(), on));
}

/// Sets the step of the sale going on.
fn now_on(world: &mut World, step: Step) {
    if let Some(sale) = &mut world.resource_mut::<Selling>().0 {
        sale.step = step;
    }
}

/// Goes on from the listing put up or changed, or its cover set, to
/// what comes after.
fn after_saving(world: &mut World, covered: bool) {
    let Some(sale) = &world.resource::<Selling>().0 else {
        return;
    };
    let (cover, wares) =
        (sale.offer.cover.is_some(), sale.offer.wares.is_some());
    if !covered && cover {
        now_on(world, Step::Covering);
        return ask(world, Call::Cover);
    }
    if wares {
        now_on(world, Step::Uploading);
        return ask(world, Call::Release);
    }
    after_release(world);
}

/// Goes on from the version made, or none, to the shelves or to the
/// end.
fn after_release(world: &mut World) {
    let Some(sale) = &world.resource::<Selling>().0 else {
        return;
    };
    if sale.offer.publish {
        now_on(world, Step::Publishing);
        return ask(world, Call::Publish);
    }
    let lot = sale.listing.clone();
    finish(world, lot);
}

/// Ends the sale with the listing as it is now.
fn finish(world: &mut World, lot: Option<Lot>) {
    let step = match lot {
        Some(lot) => Step::Done(lot.title, lot.status),
        None => Step::Failed("The bazaar said nothing of it.".into()),
    };
    if let Some(sale) = &mut world.resource_mut::<Selling>().0 {
        sale.package = None;
    }
    now_on(world, step);
    ask_account(world);
    ask_shelves(world);
}

fn fail(world: &mut World, why: Refusal) {
    if let Some(sale) = &mut world.resource_mut::<Selling>().0 {
        sale.package = None;
    }
    now_on(world, Step::Failed(why.words()));
    ask_account(world);
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    answer: Fetched,
) {
    if let Call::Shelve(_, on) = call {
        let said = match read::<Lot>(answer) {
            Ok(lot) if on => {
                (format!("{} is on the shelves.", lot.title), false)
            }
            Ok(lot) => (
                format!(
                    "{} is off the shelves. Those who own it keep \
                     it.",
                    lot.title
                ),
                false,
            ),
            Err(why) => (why.words(), true),
        };
        world.resource_mut::<Market>().said = Some(said);
        ask_account(world);
        return ask_shelves(world);
    }
    if !going(world) {
        return;
    }
    match call {
        Call::Save => match read::<Lot>(answer) {
            Ok(lot) => {
                if let Some(sale) =
                    &mut world.resource_mut::<Selling>().0
                {
                    sale.listing = Some(lot);
                }
                after_saving(world, false);
            }
            Err(why) => fail(world, why),
        },
        Call::Cover => match done(answer) {
            Ok(()) => after_saving(world, true),
            Err(why) => fail(world, why),
        },
        Call::Release => match done(answer) {
            Ok(()) => after_release(world),
            Err(why) => fail(world, why),
        },
        Call::Publish => match read::<Lot>(answer) {
            Ok(lot) => finish(world, Some(lot)),
            Err(why) => fail(world, why),
        },
        Call::Shelve(..) => {}
    }
}

/// Takes in the tools the home's server told of, and the parts of the
/// package it packed.
pub(super) fn heard(world: &mut World, response: &Response) {
    let mut housed = world.resource_mut::<Housed>();
    if housed.asked.as_ref() == Some(&response.to) {
        housed.asked = None;
        housed.tools = Some(match &response.message {
            protocol::ServerMessage::Read(
                protocol::ReadResult::Tools { tools, .. },
            ) => Ok(tools.clone()),
            other => Err(refused(other)
                .unwrap_or_else(|| "it answered oddly".into())),
        });
        return;
    }
    let packing = match &world.resource::<Selling>().0 {
        Some(Sale {
            step: Step::Packing(request),
            ..
        }) => *request == response.to,
        _ => false,
    };
    if !packing {
        return;
    }
    let part = match &response.message {
        protocol::ServerMessage::Read(
            protocol::ReadResult::Package { package, .. },
        ) => package.clone(),
        other => {
            let why = refused(other)
                .unwrap_or_else(|| "it answered oddly".into());
            return now_on(
                world,
                Step::Failed(format!(
                    "It could not be packed: {why}"
                )),
            );
        }
    };
    let mut selling = world.resource_mut::<Selling>();
    let Some(sale) = &mut selling.0 else {
        return;
    };
    let Some(bytes) = &mut sale.package else {
        return;
    };
    let decoded = STANDARD.decode(&part.data).ok();
    match decoded {
        Some(data) if part.offset == bytes.len() as u64 => {
            bytes.extend(data);
        }
        _ => {
            sale.step = Step::Failed(
                "The package came wrong from the home's server."
                    .into(),
            );
            return;
        }
    }
    if (bytes.len() as u64) < part.size {
        return;
    }
    sale.step = Step::Saving;
    ask(world, Call::Save);
}
