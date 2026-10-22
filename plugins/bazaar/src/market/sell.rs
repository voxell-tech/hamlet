//! Selling at the bazaar: an agent, a hamlet or tools packed by the
//! agents service, the listing put up or changed with its words and
//! price, the package streamed a part at a time from the agents
//! service to the server and made its new version, and the listing
//! put on the shelves; and a listing of the chief's put on the
//! shelves or taken off. One sale goes at a time, each step sent once
//! the one before is answered, so no more than a part of a package is
//! held.

use agents_ui::{AgentsWorldExt, FailureExt, Heard};
use hamlet_engine_bevy::api::agents::{
    AgentsReply, AgentsRequest, HousedTool, Packing,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Failure, Reply};

use crate::market::client::{Refusal, done, read};
use crate::market::deal::{PART_BYTES, parts_of};
use crate::market::lot::{Draft, Lot, Status, Upload};
use crate::market::{Market, ask_account, ask_shelves, send};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Selling>().init_resource::<Housed>();
}

/// The tools in the agents' houses that can be sold, as the agents
/// service last told, or why it could not.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Housed(pub Option<Result<Vec<HousedTool>, String>>);

/// Something put up for sale, or a listing of the chief's changed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Offer {
    /// The listing changed; `None` for a new one.
    pub id: Option<String>,
    pub draft: Draft,
    /// What is packed as its new version, if one is uploaded.
    pub wares: Option<Packing>,
    pub version: String,
    pub changes: String,
    /// A picture for its cover and its type. Nothing chooses one
    /// yet: a plugin reads no files, and its views show no
    /// picture.
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
    /// The package packed and held by the agents service, once it
    /// is: its id there and its size.
    pack: Option<(u64, u64)>,
    /// The listing, once it is put up or changed.
    listing: Option<Lot>,
}

/// A step of a sale.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    /// The agents service packs it.
    Packing,
    /// The listing is put up or changed.
    Saving,
    Covering,
    /// The package goes up in parts: the upload's id and the part
    /// sent.
    Uploading(Option<String>, usize),
    /// The version is made of the parts.
    Releasing,
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
    StartUpload,
    Part(usize),
    Release,
    Publish,
    /// The listing of this id put on the shelves, or taken off.
    Shelve(String, bool),
}

/// Where the part `index` of a package of `size` bytes begins, and
/// how many bytes it holds; none past the end.
pub(crate) fn part_at(size: u64, index: usize) -> (u64, u64) {
    let from = index as u64 * PART_BYTES;
    (from, size.saturating_sub(from).min(PART_BYTES))
}

/// Asks the agents service for the tools that can be sold.
pub(crate) fn ask_tools(world: &mut World) {
    world.ask(AgentsRequest::Tools);
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
    let first = match (
        &offer.wares,
        world.resource::<Market>().signed_in(),
    ) {
        (_, false) => {
            Step::Failed("Sign in with GitHub first.".into())
        }
        (Some(_), true) => Step::Packing,
        (None, true) => Step::Saving,
    };
    let wares = offer.wares.clone();
    world.resource_mut::<Selling>().0 = Some(Sale {
        offer,
        step: first.clone(),
        pack: None,
        listing: None,
    });
    match (first, wares) {
        (Step::Packing, Some(wares)) => {
            world.ask(AgentsRequest::Pack(wares))
        }
        (Step::Saving, _) => ask(world, Call::Save),
        _ => {}
    }
}

fn ask(world: &mut World, call: Call) {
    ask_with(world, call, Vec::new());
}

/// Asks the server `call`, with `bytes` for the part it uploads.
fn ask_with(world: &mut World, call: Call, bytes: Vec<u8>) {
    let server = world.resource::<Market>().server.clone();
    let sale = world.resource::<Selling>().0.clone();
    let id = sale
        .as_ref()
        .and_then(|sale| sale.listing.as_ref())
        .map(|lot| lot.id.clone())
        .unwrap_or_default();
    let request = match (&call, &sale) {
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
        (Call::StartUpload, Some(_)) => server.start_upload(&id),
        (Call::Part(index), Some(sale)) => {
            let Step::Uploading(Some(upload), _) = &sale.step else {
                return;
            };
            server.upload_part(&id, upload, *index, bytes)
        }
        (Call::Release, Some(sale)) => {
            let (Step::Uploading(Some(upload), _), Some((_, size))) =
                (&sale.step, sale.pack)
            else {
                return;
            };
            server.release(
                &id,
                &sale.offer.version,
                &sale.offer.changes,
                upload,
                parts_of(size) as usize,
            )
        }
        (Call::Publish, Some(_)) => server.publish(&id),
    };
    send(world, crate::market::Call::Sell(call), request);
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
    let Some(sale) = world.resource::<Selling>().0.clone() else {
        return;
    };
    if !covered && sale.offer.cover.is_some() {
        now_on(world, Step::Covering);
        return ask(world, Call::Cover);
    }
    if sale.pack.is_some() {
        now_on(world, Step::Uploading(None, 0));
        return ask(world, Call::StartUpload);
    }
    after_release(world);
}

/// Goes on from the version made, or none, to the shelves or to the
/// end.
fn after_release(world: &mut World) {
    let Some(sale) = world.resource::<Selling>().0.clone() else {
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
    now_on(world, step);
    ask_account(world);
    ask_shelves(world);
}

fn fail(world: &mut World, why: Refusal) {
    now_on(world, Step::Failed(why.words()));
    ask_account(world);
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    reply: Result<Reply, Failure>,
) {
    if let Call::Shelve(_, on) = call {
        let said = match read::<Lot>(reply) {
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
        Call::Save => match read::<Lot>(reply) {
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
        Call::Cover => match done(reply) {
            Ok(()) => after_saving(world, true),
            Err(why) => fail(world, why),
        },
        Call::StartUpload => match read::<Upload>(reply) {
            Ok(upload) => {
                now_on(world, Step::Uploading(Some(upload.id), 0));
                read_part(world, 0);
            }
            Err(why) => fail(world, why),
        },
        Call::Part(index) => match done(reply) {
            Ok(()) => {
                let Some(sale) =
                    world.resource::<Selling>().0.clone()
                else {
                    return;
                };
                let (Step::Uploading(upload, _), Some((_, size))) =
                    (sale.step, sale.pack)
                else {
                    return;
                };
                now_on(world, Step::Uploading(upload, index + 1));
                match index + 1 < parts_of(size) as usize {
                    true => read_part(world, index + 1),
                    false => ask(world, Call::Release),
                }
            }
            Err(why) => fail(world, why),
        },
        Call::Release => match done(reply) {
            Ok(()) => {
                now_on(world, Step::Releasing);
                after_release(world);
            }
            Err(why) => fail(world, why),
        },
        Call::Publish => match read::<Lot>(reply) {
            Ok(lot) => finish(world, Some(lot)),
            Err(why) => fail(world, why),
        },
        Call::Shelve(..) => {}
    }
}

/// Takes in what the agents service packed, and the tools it told of.
pub(super) fn heard(world: &mut World, told: &Heard) {
    match told {
        Heard::Reply(_, AgentsReply::Tools(tools)) => {
            world.insert_resource(Housed(Some(Ok(tools.clone()))));
        }
        Heard::Failed(Some(AgentsRequest::Tools), why) => {
            world.insert_resource(Housed(Some(Err(why.plain()))));
        }
        Heard::Reply(
            Some(AgentsRequest::Pack(_)),
            AgentsReply::Packed { id, size },
        ) => packed(world, Ok((*id, *size))),
        Heard::Failed(Some(AgentsRequest::Pack(_)), why) => packed(
            world,
            Err(format!("It could not be packed: {}", why.plain())),
        ),
        Heard::Reply(
            Some(AgentsRequest::PackedPart { id, offset }),
            AgentsReply::Part(bytes),
        ) => came_part(world, *id, *offset, Ok(bytes.clone())),
        Heard::Failed(
            Some(AgentsRequest::PackedPart { id, offset }),
            why,
        ) => came_part(world, *id, *offset, Err(why.plain())),
        _ => {}
    }
}

fn packed(world: &mut World, pack: Result<(u64, u64), String>) {
    let packing = world
        .resource::<Selling>()
        .0
        .as_ref()
        .is_some_and(|sale| sale.step == Step::Packing);
    if !packing {
        return;
    }
    match pack {
        Ok(pack) => {
            if let Some(sale) = &mut world.resource_mut::<Selling>().0
            {
                sale.pack = Some(pack);
                sale.step = Step::Saving;
            }
            ask(world, Call::Save);
        }
        Err(why) => now_on(world, Step::Failed(why)),
    }
}

/// Asks the agents service for the part `index` of the package it
/// holds, to upload it.
fn read_part(world: &mut World, index: usize) {
    let Some((id, size)) = world
        .resource::<Selling>()
        .0
        .as_ref()
        .and_then(|sale| sale.pack)
    else {
        return;
    };
    let (offset, _) = part_at(size, index);
    world.ask(AgentsRequest::PackedPart { id, offset });
}

/// Uploads the part of the package held as `id` from `offset`, as it
/// came from the agents service.
fn came_part(
    world: &mut World,
    id: u64,
    offset: u64,
    bytes: Result<Vec<u8>, String>,
) {
    let Some(sale) = world.resource::<Selling>().0.clone() else {
        return;
    };
    let (Step::Uploading(Some(_), index), Some((held, size))) =
        (&sale.step, sale.pack)
    else {
        return;
    };
    let (from, wanted) = part_at(size, *index);
    // Only the part asked for last moves the sale.
    if held != id || from != offset {
        return;
    }
    match bytes {
        Ok(bytes) if bytes.len() as u64 == wanted && wanted > 0 => {
            ask_with(world, Call::Part(*index), bytes)
        }
        Ok(bytes) => now_on(
            world,
            Step::Failed(format!(
                "The package came wrong from the agents service: {} \
                 bytes from byte {from}, where {wanted} were due.",
                bytes.len()
            )),
        ),
        Err(why) => now_on(
            world,
            Step::Failed(format!("It could not be read: {why}")),
        ),
    }
}
