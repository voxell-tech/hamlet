//! The OpenRouter key of a Hamlet account, kept in its vault at the
//! bazaar's server (`hamlet_protocol::vault`). At sign-in the window
//! unlocks the vault's data key with the password typed; the home,
//! which owns provider keys, seals its key with it for the vault and
//! opens the one the vault keeps.

use bevy::app::{App, PreUpdate};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::world::World;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Providers;
use hamlet_protocol::client as protocol;
use hamlet_protocol::vault::{DataKey, PasswordKey, Salt};
use serde::{Deserialize, Serialize};

use crate::PanelSystems;
use crate::bazaar::errand::{self, Errand, Fetched, fetch};
use crate::bazaar::market::client::{Server, done, read};
use crate::bazaar::market::{Call as MarketCall, Market, session};

/// The provider whose key an account keeps.
const OPENROUTER: &str = "openrouter";

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Syncing>().add_systems(
        PreUpdate,
        connected
            .run_if(resource_changed::<Providers>)
            .in_set(PanelSystems::Follow),
    );
}

/// A call of the bazaar's server about the vault.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    /// The vault opened, or made, with the password typed.
    Open,
    /// The data key locked again with a new password.
    Relock,
    /// The home's key, sealed, kept in the vault.
    Keep,
    /// The vault's key forgotten.
    Drop,
}

/// The home's requests about the account's key, while they are
/// answered, and whether the home had an OpenRouter key when its
/// providers last came.
#[derive(Resource, Default)]
pub(crate) struct Syncing {
    sealing: Option<protocol::RequestId>,
    unsealing: Option<protocol::RequestId>,
    had: Option<bool>,
}

/// What opening the vault comes to.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Opened {
    data_key: String,
    /// The OpenRouter key the vault keeps, sealed, if it does.
    sealed: Option<String>,
}

/// The vault as the bazaar's server keeps it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    salt: String,
    data_key: String,
    #[serde(default)]
    openrouter_key: Option<String>,
}

/// Opens the vault of the one signed in with `password`, or makes
/// one, then shares the OpenRouter key between it and the home.
pub(crate) fn open(world: &mut World, password: String) {
    let server = world.resource::<Market>().server.clone();
    let errand = Errand::Market(MarketCall::Vault(Call::Open));
    errand::run(world, errand, |client| async move {
        opened(&client, &server, password).await
    });
}

/// Locks the data key again with `password`, made new.
pub(crate) fn relock(world: &mut World, password: String) {
    let market = world.resource::<Market>();
    let (server, data) =
        (market.server.clone(), market.data_key.clone());
    let errand = Errand::Market(MarketCall::Vault(Call::Relock));
    errand::run(world, errand, |client| async move {
        match data {
            Some(data) => {
                keep(&client, &server, &password, &data).await
            }
            // Without the data key, the vault is opened as at a
            // sign-in, which makes a new one if the old does not
            // open.
            None => opened(&client, &server, password).await,
        }
    });
}

/// The vault's salt and data key locked with `password`, kept.
async fn keep(
    client: &reqwest::Client,
    server: &Server,
    password: &str,
    data: &DataKey,
) -> Fetched {
    let salt = Salt::new();
    let key = derived(password.to_string(), salt).await?;
    let request = server.keep_vault(&salt.text(), &key.lock(data));
    fetch(client, request).await
}

/// The password key of `password` and `salt`, made off the runtime's
/// threads, as it takes a while on purpose.
async fn derived(
    password: String,
    salt: Salt,
) -> Result<PasswordKey, String> {
    tokio::task::spawn_blocking(move || {
        PasswordKey::derive(&password, &salt)
    })
    .await
    .map_err(|why| why.to_string())
}

/// Opens the vault with `password`: its data key, and the key it
/// keeps. A vault there is none of is made; one that does not open,
/// as after a password was reset, is made again without its key.
async fn opened(
    client: &reqwest::Client,
    server: &Server,
    password: String,
) -> Fetched {
    let answer = fetch(client, server.vault()).await?;
    let kept = match answer.0 {
        404 => None,
        200..=299 => {
            match serde_json::from_slice::<Kept>(&answer.1) {
                Ok(kept) => Some(kept),
                Err(_) => return Ok(answer),
            }
        }
        _ => return Ok(answer),
    };
    if let Some(kept) = &kept
        && let Ok(salt) = Salt::read(&kept.salt)
    {
        let key = derived(password.clone(), salt).await?;
        if let Ok(data) = key.unlock(&kept.data_key) {
            return found(&data, kept.openrouter_key.clone());
        }
    }
    let data = DataKey::new();
    let answer = keep(client, server, &password, &data).await?;
    if !(200..300).contains(&answer.0) {
        return Ok(answer);
    }
    if kept.is_some() {
        fetch(client, server.drop_sealed()).await?;
    }
    found(&data, None)
}

fn found(data: &DataKey, sealed: Option<String>) -> Fetched {
    let opened = Opened {
        data_key: data.to_base64(),
        sealed,
    };
    serde_json::to_vec(&opened)
        .map(|body| (200, body))
        .map_err(|why| why.to_string())
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    answer: Fetched,
) {
    match call {
        Call::Open => match read::<Opened>(answer) {
            Ok(opened) => {
                let data =
                    DataKey::from_base64(&opened.data_key).ok();
                world.resource_mut::<Market>().data_key = data;
                session::keep(world);
                share(world, opened.sealed);
            }
            Err(why) => super::account::refused(world, why),
        },
        Call::Relock | Call::Keep | Call::Drop => {
            if let Err(why) = done(answer) {
                super::account::refused(world, why);
            }
        }
    }
}

/// Whether the home has an OpenRouter key.
pub(crate) fn home_has_key(world: &World) -> bool {
    world.get_resource::<Providers>().is_some_and(|providers| {
        providers.0.iter().any(|provider| {
            provider.id == OPENROUTER && provider.configured
        })
    })
}

/// Shares the OpenRouter key between the vault, which keeps `sealed`,
/// and the home: the vault's goes to a home without one, and the
/// home's to a vault without one.
fn share(world: &mut World, sealed: Option<String>) {
    let Some(data) = world.resource::<Market>().data_key.clone()
    else {
        return;
    };
    let data_key = data.to_base64();
    match (sealed, home_has_key(world)) {
        (Some(sealed), false) => {
            let id = world.resource_mut::<Outbox>().send(|id| {
                protocol::ClientMessage::Login(
                    protocol::Login::Unseal {
                        id,
                        unseal: protocol::Unseal { data_key, sealed },
                    },
                )
            });
            world.resource_mut::<Syncing>().unsealing = Some(id);
        }
        (None, true) => seal(world),
        _ => {}
    }
}

/// Asks the home to seal its OpenRouter key, for the vault.
fn seal(world: &mut World) {
    let Some(data) = world.resource::<Market>().data_key.clone()
    else {
        return;
    };
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Sealed {
            data_key: data.to_base64(),
            id,
        })
    });
    world.resource_mut::<Syncing>().sealing = Some(id);
}

/// Sends the home's OpenRouter key to the vault once the home has
/// one it did not have, as after CONNECT OPENROUTER.
fn connected(world: &mut World) {
    // Until the server's state comes, nothing is known of its keys.
    if world.resource::<Providers>().0.is_empty() {
        return;
    }
    let has = home_has_key(world);
    let had = world.resource_mut::<Syncing>().had.replace(has);
    let unsealing = world.resource::<Syncing>().unsealing.is_some();
    if had == Some(false) && has && !unsealing {
        seal(world);
    }
}

/// Takes in what the home says of the key it sealed or opened.
pub(super) fn heard(world: &mut World, response: &Response) {
    let mut syncing = world.resource_mut::<Syncing>();
    if syncing.unsealing.as_ref() == Some(&response.to) {
        // Its key coming in is not one to send back to the vault.
        syncing.unsealing = None;
        syncing.had = Some(true);
        return;
    }
    if syncing.sealing.as_ref() != Some(&response.to) {
        return;
    }
    syncing.sealing = None;
    if let protocol::ServerMessage::Read(
        protocol::ReadResult::Sealed {
            sealed: Some(sealed),
            ..
        },
    ) = &response.message
    {
        let request =
            world.resource::<Market>().server.keep_sealed(sealed);
        super::send(world, MarketCall::Vault(Call::Keep), request);
    }
}

/// Forgets the home's OpenRouter key and the one the account keeps.
/// OpenRouter itself keeps the key working until it is removed there.
pub(crate) fn disconnect(world: &mut World) {
    world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Login(protocol::Login::Forget {
            id,
            forget: OPENROUTER.into(),
        })
    });
    if world
        .get_resource::<Market>()
        .is_some_and(Market::signed_in)
    {
        let request = world.resource::<Market>().server.drop_sealed();
        super::send(world, MarketCall::Vault(Call::Drop), request);
    }
}
