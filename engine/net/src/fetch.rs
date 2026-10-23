//! Fetches, made off the frame. Each is a child of the plugin that
//! asked, so it goes with the plugin, and its reply goes to the
//! plugin once it is done.

use bevy::app::{App, PostUpdate};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use bevy::log::error;
use bevy::tasks::futures::check_ready;
use hamlet_engine_api::{
    Capability, Failure, FetchReply, FetchRequest, Method, Reply,
    Request, Ticket,
};
use hamlet_engine_core::LogLine;
use hamlet_engine_grants::{
    Decided, FirstUse, Grants, Verdict, Waiting,
};
use hamlet_engine_plugins::{Asked, Deliver, PluginInfo};
use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use reqwest::{Client, Url};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use crate::address::{NetPluginInfoExt, reach};

pub(crate) fn plugin(app: &mut App) {
    if !app.world().contains_resource::<Offline>() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("hamlet net")
            .enable_all()
            .build();
        match runtime {
            Ok(runtime) => {
                app.insert_resource(Network(runtime));
            }
            Err(why) => error!("net: no fetch can be made: {why}"),
        }
    }
    app.add_systems(PostUpdate, (start, finish).chain());
}

/// Put in before the plugin, fetches are not sent: each waits for a
/// reply given with [`NetWorldExt::answer_fetch`], as a test gives
/// one.
#[derive(Resource, Default, Debug)]
pub struct Offline;

/// The runtime fetches are made on.
#[derive(Resource)]
struct Network(Runtime);

/// On a child of a plugin: a fetch it asked for, under way.
#[derive(Component, Clone, Debug)]
pub struct Fetch {
    pub request: FetchRequest,
    ticket: Ticket,
    depth: u32,
}

/// On a fetch: it being made off the frame, stopped when the fetch
/// goes with its plugin.
#[derive(Component)]
struct Sent(JoinHandle<Result<FetchReply, Failure>>);

impl Drop for Sent {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Starts the fetches that may be made, and answers the others with
/// why not.
fn start(
    mut asked: MessageReader<Asked>,
    mut decided: MessageReader<Decided>,
    plugins: Query<&PluginInfo>,
    grants: Res<Grants>,
    network: Option<Res<Network>>,
    offline: Option<Res<Offline>>,
    mut parked: Local<Waiting<Asked>>,
    mut deliver: MessageWriter<Deliver>,
    mut first_use: MessageWriter<FirstUse>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    parked.keep(|plugin| plugins.contains(plugin));
    let mut again = Vec::new();
    for (waiting, allowed) in parked.settle(decided.read()) {
        if allowed {
            again.push((waiting, true));
        } else {
            deliver.write(waiting.reply(Err(Failure::Refused(
                format!(
                    "{} was not allowed",
                    Capability::NetFetch.name()
                ),
            ))));
        }
    }
    let fresh = asked.read().map(|asked| (asked.clone(), false));
    for (request, allowed) in again.into_iter().chain(fresh) {
        let Request::Fetch(fetch) = &request.request else {
            continue;
        };
        let Ok(plugin) = plugins.get(request.plugin) else {
            continue;
        };
        let (url, target) = match plugin.listed(&fetch.url) {
            Ok(listed) => listed,
            Err(why) => {
                deliver.write(request.reply(Err(why)));
                continue;
            }
        };
        if !allowed {
            match grants.verdict(
                plugin,
                Capability::NetFetch,
                &target,
            ) {
                Verdict::Allowed => {}
                Verdict::Asking => {
                    parked.park(
                        request.plugin,
                        Capability::NetFetch,
                        target,
                        request.clone(),
                        &mut first_use,
                    );
                    continue;
                }
                Verdict::Refused => {
                    deliver.write(request.reply(Err(
                        Failure::Refused(format!(
                            "{} {target} is not allowed",
                            Capability::NetFetch.name()
                        )),
                    )));
                    continue;
                }
            }
        }
        if network.is_none() && offline.is_none() {
            deliver.write(request.reply(Err(Failure::Failed(
                "the network could not be started".into(),
            ))));
            continue;
        }
        log.write(LogLine::info(
            plugin.id.as_str(),
            format!(
                "fetches from {}",
                url.host_str().unwrap_or_default()
            ),
        ));
        let mut entity = commands.spawn((
            Fetch {
                request: fetch.clone(),
                ticket: request.ticket,
                depth: request.depth,
            },
            ChildOf(request.plugin),
        ));
        if let Some(network) = &network {
            let fetch = fetch.clone();
            entity.insert(Sent(network.0.spawn(send(url, fetch))));
        }
    }
}

/// Replies to each fetch that is done.
fn finish(
    mut fetches: Query<(Entity, &Fetch, &ChildOf, &mut Sent)>,
    mut deliver: MessageWriter<Deliver>,
    mut commands: Commands,
) {
    for (entity, fetch, parent, mut sent) in &mut fetches {
        let Some(done) = check_ready(&mut sent.0) else {
            continue;
        };
        let done = done.unwrap_or_else(|why| {
            Err(Failure::Failed(format!("the fetch stopped: {why}")))
        });
        deliver.write(fetch.reply(parent.parent(), done));
        commands.entity(entity).despawn();
    }
}

/// The network's methods on [`World`].
pub trait NetWorldExt {
    /// Replies to the fetch `fetch` with `done`, as the network would
    /// have, and lets it go.
    fn answer_fetch(
        &mut self,
        fetch: Entity,
        done: Result<FetchReply, Failure>,
    );
}

impl NetWorldExt for World {
    fn answer_fetch(
        &mut self,
        fetch: Entity,
        done: Result<FetchReply, Failure>,
    ) {
        let Ok(entity) = self.get_entity(fetch) else {
            return;
        };
        let (Some(made), Some(parent)) =
            (entity.get::<Fetch>().cloned(), entity.get::<ChildOf>())
        else {
            return;
        };
        let message = made.reply(parent.parent(), done);
        self.write_message(message);
        self.despawn(fetch);
    }
}

impl Fetch {
    /// The reply `done` to this fetch, for `plugin`.
    fn reply(
        &self,
        plugin: Entity,
        done: Result<FetchReply, Failure>,
    ) -> Deliver {
        Deliver {
            plugin,
            message: hamlet_engine_api::Message::Reply {
                ticket: self.ticket,
                reply: done.map(Reply::Fetched),
            },
            depth: self.depth,
        }
    }
}

/// Sends `fetch` to `url` at the public addresses its host is at, and
/// reads the answer, whatever its status. A redirect is not followed,
/// and no proxy is used.
async fn send(
    url: Url,
    fetch: FetchRequest,
) -> Result<FetchReply, Failure> {
    let failed = |why: String| Failure::Failed(why);
    let host = url.host_str().unwrap_or_default().to_owned();
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = reach(&host, port).await?;
    let client = Client::builder()
        .redirect(Policy::none())
        .referer(false)
        .no_proxy()
        .resolve_to_addrs(&host, &addresses)
        .build()
        .map_err(|why| failed(format!("no client: {why}")))?;
    let request = match fetch.method {
        Method::Get => client.get(url),
        Method::Post => {
            let post = client.post(url);
            match &fetch.content_type {
                Some(kind) => post.header(CONTENT_TYPE, kind),
                None => post,
            }
            .body(fetch.body)
        }
    };
    let unsent = |why: reqwest::Error| {
        failed(format!("the fetch failed: {}", why.without_url()))
    };
    let mut answer = request.send().await.map_err(unsent)?;
    let status = answer.status().as_u16();
    let content_type = answer
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let mut body = Vec::new();
    while let Some(chunk) = answer.chunk().await.map_err(unsent)? {
        body.extend_from_slice(&chunk);
    }
    Ok(FetchReply {
        status,
        content_type,
        body,
    })
}
