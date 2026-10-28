//! Selling at the bazaar: an agent or a hamlet packed by the home's
//! server and handed to the window in parts, then put up as a new
//! listing with it as its draft first version, or uploaded as the
//! next draft version of a listing of the chief's. One sale goes at a
//! time.

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
use crate::bazaar::market::lot::{Kind, Lot};
use crate::bazaar::market::{Market, ask_mine, refused, send};

/// The version a new listing's first draft is uploaded as.
pub(crate) const FIRST_VERSION: &str = "0.0.1";

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Selling>();
}

/// What is packed and uploaded.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Offer {
    /// The listing it is a new version of; `None` for a new listing.
    pub id: Option<String>,
    pub kind: Kind,
    /// A new listing's first title.
    pub title: String,
    pub wares: protocol::Packing,
    pub version: String,
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
    /// The listing, once it is made.
    listing: Option<String>,
}

/// A step of a sale.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    /// The home's server packs it, answering this read.
    Packing(protocol::RequestId),
    /// The listing is made, with the package as its first draft.
    Creating,
    /// The package goes up as a draft version.
    Uploading,
    /// Uploaded, as this version of the listing of this id.
    Done(String, String),
    Failed(String),
}

impl Step {
    pub fn going(&self) -> bool {
        !matches!(self, Step::Done(..) | Step::Failed(_))
    }
}

/// A call of the server for the sale going on.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    Create,
    Upload,
}

fn going(world: &World) -> bool {
    world
        .resource::<Selling>()
        .0
        .as_ref()
        .is_some_and(|sale| sale.step.going())
}

/// Packs `offer`, then uploads it to the bazaar.
pub(crate) fn sell(world: &mut World, offer: Offer) {
    if going(world) {
        return;
    }
    let first = match world.resource::<Market>().signed_in() {
        false => Step::Failed(
            "Sign in to your Hamlet account first.".into(),
        ),
        true => {
            let wares = offer.wares.clone();
            let request = world.resource_mut::<Outbox>().send(|id| {
                protocol::ClientMessage::Read(
                    protocol::Read::Package { id, of: wares },
                )
            });
            Step::Packing(request)
        }
    };
    let listing = offer.id.clone();
    world.resource_mut::<Selling>().0 = Some(Sale {
        offer,
        step: first,
        package: Some(Vec::new()),
        listing,
    });
}

fn ask(world: &mut World, call: Call) {
    let server = &world.resource::<Market>().server;
    let Some(sale) = &world.resource::<Selling>().0 else {
        return;
    };
    let request = match call {
        Call::Create => {
            let Some(package) = &sale.package else {
                return;
            };
            server.create_with(
                sale.offer.kind,
                &sale.offer.title,
                package,
            )
        }
        Call::Upload => {
            let (Some(id), Some(package)) =
                (&sale.listing, &sale.package)
            else {
                return;
            };
            server.upload(id, &sale.offer.version, package)
        }
    };
    send(world, crate::bazaar::market::Call::Sell(call), request);
}

/// Sets the step of the sale going on.
fn now_on(world: &mut World, step: Step) {
    if let Some(sale) = &mut world.resource_mut::<Selling>().0 {
        sale.step = step;
    }
}

/// Goes on from the package packed, to the listing made if it is new,
/// else to its upload.
fn packed(world: &mut World) {
    let new = world
        .resource::<Selling>()
        .0
        .as_ref()
        .is_some_and(|sale| sale.listing.is_none());
    match new {
        true => {
            now_on(world, Step::Creating);
            ask(world, Call::Create);
        }
        false => {
            now_on(world, Step::Uploading);
            ask(world, Call::Upload);
        }
    }
}

fn end(world: &mut World, step: Step) {
    if let Some(sale) = &mut world.resource_mut::<Selling>().0 {
        sale.package = None;
    }
    now_on(world, step);
    ask_mine(world);
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    answer: Fetched,
) {
    if !going(world) {
        return;
    }
    match call {
        Call::Create => match read::<Lot>(answer) {
            Ok(lot) => {
                end(world, Step::Done(lot.id, FIRST_VERSION.into()));
            }
            Err(why) => fail(world, why),
        },
        Call::Upload => match done(answer) {
            Ok(()) => {
                let sale = world.resource::<Selling>().0.clone();
                let Some(Sale {
                    listing: Some(id),
                    offer,
                    ..
                }) = sale
                else {
                    return;
                };
                end(world, Step::Done(id, offer.version));
            }
            Err(why) => fail(world, why),
        },
    }
}

fn fail(world: &mut World, why: Refusal) {
    let words = match why.code.as_str() {
        "conflict" => "Finish the draft first: publish, verify or \
                       discard it."
            .into(),
        "kind_unavailable" => "Plugins can't be sold yet.".into(),
        _ => why.words(),
    };
    end(world, Step::Failed(words));
}

/// Takes in the parts of the package the home's server packed.
pub(super) fn heard(world: &mut World, response: &Response) {
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
    packed(world);
}
