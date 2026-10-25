//! The bazaar's fetches from the network, on the window's Tokio, each
//! remembered by its errand until its answer comes and is handed to
//! its stall.

use std::time::Duration;

use bevy::app::{App, PreUpdate};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use bevy::ecs::world::World;
use hamlet_client::Tokio;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::PanelSystems;
use crate::bazaar::market::client::Request;
use crate::bazaar::{covers, market, models, skills};

/// The longest a fetch is waited for.
const PATIENCE: Duration = Duration::from_secs(30);

/// The longest a package of the bazaar's server is waited for, up or
/// down.
const LONG: Duration = Duration::from_secs(300);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Errands>().add_systems(
        PreUpdate,
        answered.run_if(waiting).in_set(PanelSystems::Follow),
    );
}

/// The purpose of a fetch.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Errand {
    /// The page of OpenRouter's models from this place on.
    Models(usize),
    /// Who serves the model of this id through OpenRouter.
    Servings(String),
    /// This page of skills.sh's leaderboard.
    Board(u32),
    /// What skills.sh finds for these words.
    Search(String),
    /// The files of this skill.
    Files(skills::SkillId),
    /// A call of the bazaar's server.
    Market(market::Call),
    /// The picture at this address, chosen as a cover at the desk.
    Picture(String),
}

/// What a fetch brought: the status and the body, or why there was
/// none.
pub(crate) type Fetched = Result<(u16, Vec<u8>), String>;

/// The fetches made and not yet answered, and the way their answers
/// come back to the frame.
#[derive(Resource)]
pub(crate) struct Errands {
    sent: Vec<Errand>,
    tx: UnboundedSender<(Errand, Fetched)>,
    rx: UnboundedReceiver<(Errand, Fetched)>,
    client: Option<reqwest::Client>,
}

impl Default for Errands {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        let client = reqwest::Client::builder()
            .timeout(PATIENCE)
            .user_agent(concat!("Hamlet/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok();
        Errands {
            sent: Vec::new(),
            tx,
            rx,
            client,
        }
    }
}

impl Errands {
    /// Whether `errand` was sent and waits for its answer.
    pub fn waiting(&self, errand: &Errand) -> bool {
        self.sent.contains(errand)
    }
}

/// Fetches `url` for `errand`; false where the window has no runtime
/// to fetch it on.
pub(crate) fn send(
    world: &mut World,
    url: String,
    errand: Errand,
) -> bool {
    let request = Request {
        method: reqwest::Method::GET,
        url,
        token: None,
        body: None,
        long: false,
    };
    send_request(world, request, errand)
}

/// Sends `request` for `errand`; false where the window has no
/// runtime to send it on.
pub(crate) fn send_request(
    world: &mut World,
    request: Request,
    errand: Errand,
) -> bool {
    let Some(tokio) = world.get_resource::<Tokio>().cloned() else {
        return false;
    };
    let mut errands = world.resource_mut::<Errands>();
    let Some(client) = errands.client.clone() else {
        return false;
    };
    let tx = errands.tx.clone();
    errands.sent.push(errand.clone());
    tokio.0.spawn(async move {
        let fetched = async {
            let mut asked =
                client.request(request.method, request.url);
            if request.long {
                asked = asked.timeout(LONG);
            }
            if let Some(token) = request.token {
                asked = asked.bearer_auth(token);
            }
            if let Some((kind, body)) = request.body {
                asked = asked
                    .header(reqwest::header::CONTENT_TYPE, kind)
                    .body(body);
            }
            let answer = asked.send().await?;
            let status = answer.status().as_u16();
            Ok::<_, reqwest::Error>((
                status,
                answer.bytes().await?.to_vec(),
            ))
        }
        .await
        .map_err(|why| why.without_url().to_string());
        let _ = tx.send((errand, fetched));
    });
    true
}

/// The body of an answer read as JSON, or why it cannot be, in words
/// that name `from`.
pub(crate) fn json(
    answer: Fetched,
    from: &str,
) -> Result<serde_json::Value, String> {
    let (status, body) = answer
        .map_err(|why| format!("{from} could not be asked: {why}"))?;
    let value = serde_json::from_slice::<serde_json::Value>(&body)
        .unwrap_or_default();
    match status {
        200..=299 => Ok(value),
        404 => Err(format!("{from} does not have it")),
        _ => Err(format!("{from} answered {status}")),
    }
}

fn waiting(errands: Res<Errands>) -> bool {
    !errands.rx.is_empty()
}

/// Hands each answer of a fetch to the stall that asked.
fn answered(world: &mut World) {
    while let Ok((errand, answer)) =
        world.resource_mut::<Errands>().rx.try_recv()
    {
        let mut errands = world.resource_mut::<Errands>();
        if let Some(at) =
            errands.sent.iter().position(|e| *e == errand)
        {
            errands.sent.remove(at);
        }
        match errand {
            Errand::Models(from) => models::came(world, from, answer),
            Errand::Servings(id) => models::served(world, id, answer),
            Errand::Board(page) => skills::board(world, page, answer),
            Errand::Search(words) => {
                skills::found(world, words, answer);
            }
            Errand::Files(id) => skills::unrolled(world, id, answer),
            Errand::Market(call) => {
                market::answered(world, call, answer);
            }
            Errand::Picture(url) => {
                covers::picked(world, &url, answer);
            }
        }
    }
}
