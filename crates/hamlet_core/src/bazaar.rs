//! The bazaar's wares as a resource: read from their file, and asked
//! of OpenRouter again once they are a day old.

use std::collections::BTreeMap;

use bevy_app::{App, Startup, Update};
use bevy_ecs::prelude::*;
use hamlet_model::bazaar::{self, Stall, Ware, Wares};
use hamlet_model::log;

use crate::core::{
    HomeDir, ListModels, Model, Rounds, Tokio, answered,
};

/// What OpenRouter offers, or the reason it could not be asked.
type Fetched = Result<Vec<Ware>, String>;

/// A ware's id and the stalls that serve it.
type Looked = (String, Result<Vec<Stall>, String>);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Bazaar>()
        .init_resource::<Rounds<Fetched>>()
        .init_resource::<Rounds<Looked>>()
        .add_observer(fetch_wares)
        .add_observer(look_at)
        .add_systems(Startup, open)
        .add_systems(
            Update,
            (
                stock.run_if(answered::<Fetched>),
                seen.run_if(answered::<Looked>),
            ),
        );
}

/// The wares, and what is being asked of OpenRouter.
#[derive(Resource, Default)]
pub struct Bazaar {
    pub wares: Wares,
    /// OpenRouter is being asked for its wares.
    pub fetching: bool,
    /// The reason the wares could not be asked for or kept, last
    /// time.
    pub problem: Option<String>,
    /// The stalls of each ware looked at, by its id. `None` while
    /// they are asked for.
    pub stalls: BTreeMap<String, Option<Result<Vec<Stall>, String>>>,
}

impl Bazaar {
    /// A bazaar that holds `wares`, as if their file had been read.
    pub fn of(wares: Wares) -> Bazaar {
        Bazaar {
            wares,
            ..Bazaar::default()
        }
    }
}

/// Asks OpenRouter for its wares in the background.
#[derive(Event)]
pub struct FetchWares;

/// Asks OpenRouter which providers serve the ware of this id, unless
/// that is known already.
#[derive(Event)]
pub struct LookAt(pub String);

/// Reads the wares, and asks for them again when they are stale.
fn open(world: &mut World) {
    let home = world.resource::<HomeDir>().0.root.clone();
    let wares = Wares::load(&home);
    let stale = wares.stale(log::now_millis());
    world.resource_mut::<Bazaar>().wares = wares;
    // Answers come from a recording, and nothing is asked of a
    // network.
    if stale && !world.resource::<Model>().replay {
        world.trigger(FetchWares);
        world.trigger(ListModels);
    }
}

fn fetch_wares(
    _: On<FetchWares>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Fetched>>,
    mut market: ResMut<Bazaar>,
) {
    market.fetching = true;
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        tell(bazaar::fetch().await.map_err(|e| format!("{e:#}")));
    });
}

/// Keeps what OpenRouter answered.
fn stock(
    home: Res<HomeDir>,
    mut rounds: ResMut<Rounds<Fetched>>,
    mut market: ResMut<Bazaar>,
) {
    while let Some(fetched) = rounds.next() {
        market.fetching = false;
        market.problem = match fetched {
            Ok(wares) => {
                market.wares.openrouter = wares;
                market.wares.fetched = log::now_millis();
                market
                    .wares
                    .save(&home.0.root)
                    .err()
                    .map(|e| format!("{e:#}"))
            }
            Err(e) => Some(e),
        };
    }
}

fn look_at(
    at: On<LookAt>,
    tokio: Res<Tokio>,
    mut rounds: ResMut<Rounds<Looked>>,
    mut market: ResMut<Bazaar>,
) {
    let id = at.event().0.clone();
    if matches!(market.stalls.get(&id), Some(Some(Ok(_)))) {
        return;
    }
    // Only the last look is heard: one still asked would wait
    // forever.
    market.stalls.retain(|_, stalls| stalls.is_some());
    market.stalls.insert(id.clone(), None);
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let stalls = bazaar::stalls(&id).await;
        tell((id, stalls.map_err(|e| format!("{e:#}"))));
    });
}

fn seen(
    mut rounds: ResMut<Rounds<Looked>>,
    mut market: ResMut<Bazaar>,
) {
    while let Some((id, stalls)) = rounds.next() {
        market.stalls.insert(id, Some(stalls));
    }
}
