//! The bazaar's fetches from the network, each remembered by its
//! ticket until its answer comes and is handed to its stall.

use std::collections::BTreeMap;

use hamlet_engine_bevy::api::Method;
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{
    Failure, FetchReply, FetchRequest, Reply, Ticket,
};

use crate::market::{self, Call};
use crate::models;
use crate::skills::{self, SkillId};

/// The most fetches remembered at once. An answer the inbox left out
/// never comes, so the oldest are forgotten past it.
const REMEMBERED: usize = 64;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Errands>()
        .add_systems(PreUpdate, answered);
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
    Files(SkillId),
    /// A call of the bazaar's server, with the fetch that made it, to
    /// be sent again when the engine held it back.
    Market(Box<(Call, FetchRequest)>),
}

/// The fetches made and not yet answered, by their tickets.
#[derive(Resource, Default)]
pub(crate) struct Errands(BTreeMap<Ticket, Errand>);

impl Errands {
    /// Whether `errand` was sent and waits for its answer.
    pub fn waiting(&self, errand: &Errand) -> bool {
        self.0.values().any(|sent| sent == errand)
    }
}

/// The bazaar's fetches on [`World`].
pub(crate) trait ErrandWorldExt {
    /// Fetches `url` for `errand`; false when the fetch was not
    /// taken.
    fn send(&mut self, url: String, errand: Errand) -> bool;

    /// Fetches `request` for `errand`; false when the fetch was not
    /// taken.
    fn send_request(
        &mut self,
        request: &FetchRequest,
        errand: Errand,
    ) -> bool;
}

impl ErrandWorldExt for World {
    fn send(&mut self, url: String, errand: Errand) -> bool {
        let request = FetchRequest {
            method: Method::Get,
            url,
            content_type: None,
            body: Vec::new(),
        };
        self.send_request(&request, errand)
    }

    fn send_request(
        &mut self,
        request: &FetchRequest,
        errand: Errand,
    ) -> bool {
        let ticket = self.fetch(request);
        if ticket == 0 {
            return false;
        }
        let mut errands = self.resource_mut::<Errands>();
        errands.0.insert(ticket, errand);
        if errands.0.len() > REMEMBERED {
            errands.0.pop_first();
        }
        true
    }
}

/// The body of an answer read as JSON, or why it cannot be.
pub(crate) fn json(
    answer: Result<Reply, Failure>,
    from: &str,
) -> Result<serde_json::Value, String> {
    let fetched = match answer {
        Ok(Reply::Fetched(fetched)) => fetched,
        Ok(_) => return Err(format!("{from} answered oddly")),
        Err(why) => {
            return Err(format!("{from} could not be asked: {why}"));
        }
    };
    let FetchReply { status, body, .. } = fetched;
    let value = serde_json::from_slice::<serde_json::Value>(&body)
        .unwrap_or_default();
    match status {
        200..=299 => Ok(value),
        404 => Err(format!("{from} does not have it")),
        _ => Err(format!("{from} answered {status}")),
    }
}

/// Hands each answer of a fetch to the stall that asked.
fn answered(
    world: &mut World,
    mut replies: Local<MessageCursor<Replied>>,
) {
    let replied = replies
        .read(world.resource::<Messages<Replied>>())
        .cloned()
        .collect::<Vec<_>>();
    for Replied { ticket, reply } in replied {
        let Some(errand) =
            world.resource_mut::<Errands>().0.remove(&ticket)
        else {
            continue;
        };
        match errand {
            Errand::Models(from) => models::came(world, from, reply),
            Errand::Servings(id) => models::served(world, id, reply),
            Errand::Board(page) => skills::board(world, page, reply),
            Errand::Search(words) => {
                skills::found(world, words, reply)
            }
            Errand::Files(id) => skills::unrolled(world, id, reply),
            Errand::Market(sent) => {
                let (call, request) = *sent;
                market::answered(world, call, request, reply)
            }
        }
    }
}
