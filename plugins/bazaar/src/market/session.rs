//! The sign-in kept for the next run, in the plugin's asset store:
//! the engine gives a plugin no keychain, and a saved resource lasts
//! only across a reload. The asset holds the server and the session
//! token as JSON, and a token is used only with the server it came
//! from.

use hamlet_engine_bevy::api::AssetId;
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Reply, Ticket, warn};
use serde::{Deserialize, Serialize};

use crate::market::{Market, ask_account};

/// The type of the asset, made by the bazaar alone.
pub(crate) const SESSION: &str = "bazaar.session";

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Kept>()
        .add_systems(Startup, look)
        .add_systems(PreUpdate, came);
}

/// What the asset holds.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub(crate) struct Session {
    pub server: String,
    /// Empty once signed out.
    pub token: String,
}

/// The asset kept, once known, and the requests of the store waiting
/// for their replies.
#[derive(Resource, Default)]
struct Kept {
    id: Option<AssetId>,
    /// The asset being made, so that a second is not.
    making: bool,
    asked: Vec<(Ticket, Asked)>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Asked {
    List,
    Read,
    Make,
    Write,
}

fn look(world: &mut World) {
    let ticket = world.list_assets(SESSION);
    remember(world, ticket, Asked::List);
}

fn remember(world: &mut World, ticket: Ticket, asked: Asked) {
    if ticket != 0 {
        world.resource_mut::<Kept>().asked.push((ticket, asked));
    }
}

/// Keeps `token` of the server signed in to for the next run, or
/// forgets the one kept.
pub(crate) fn keep(world: &mut World, token: Option<&str>) {
    let session = Session {
        server: world.resource::<Market>().server.base.clone(),
        token: token.unwrap_or_default().to_string(),
    };
    let Ok(bytes) = serde_json::to_vec(&session) else {
        return;
    };
    let kept = world.resource::<Kept>();
    match (kept.id, kept.making) {
        (Some(id), _) => {
            let ticket = world.update_asset(id, &bytes);
            remember(world, ticket, Asked::Write);
        }
        (None, false) => {
            world.resource_mut::<Kept>().making = true;
            let ticket =
                world.create_asset(SESSION, "Bazaar sign-in", &bytes);
            remember(world, ticket, Asked::Make);
        }
        // The one being made is written once it is.
        (None, true) => {}
    }
}

/// The token of `session` for the server `base`, if it has one.
pub(crate) fn token_for(
    session: &Session,
    base: &str,
) -> Option<String> {
    (session.server == base && !session.token.is_empty())
        .then(|| session.token.clone())
}

fn came(
    world: &mut World,
    mut replies: Local<MessageCursor<Replied>>,
) {
    let replied = replies
        .read(world.resource::<Messages<Replied>>())
        .cloned()
        .collect::<Vec<_>>();
    for Replied { ticket, reply } in replied {
        let mut kept = world.resource_mut::<Kept>();
        let Some(at) =
            kept.asked.iter().position(|(t, _)| *t == ticket)
        else {
            continue;
        };
        let (_, asked) = kept.asked.remove(at);
        match (asked, reply) {
            (Asked::List, Ok(Reply::Listed(found))) => {
                if let Some(found) = found.first() {
                    kept.id = Some(found.id);
                    let ticket = world.read_asset(found.id);
                    remember(world, ticket, Asked::Read);
                }
            }
            (Asked::Read, Ok(Reply::Read(asset))) => {
                let session =
                    serde_json::from_slice::<Session>(&asset.bytes)
                        .unwrap_or_default();
                let mut market = world.resource_mut::<Market>();
                let token = token_for(&session, &market.server.base);
                // A sign-in begun meanwhile is the one that counts.
                if token.is_some() && market.server.token.is_none() {
                    market.server.token = token;
                    ask_account(world);
                }
            }
            (Asked::Make, Ok(Reply::Created(id))) => {
                kept.id = Some(id);
                kept.making = false;
                // What was kept meanwhile goes in now.
                let token =
                    world.resource::<Market>().server.token.clone();
                keep(world, token.as_deref());
            }
            (Asked::Make, _) => {
                kept.making = false;
                warn("the bazaar's sign-in could not be kept");
            }
            (_, Err(why)) => {
                warn(&format!("the bazaar's sign-in store: {why}"));
            }
            _ => {}
        }
    }
}
