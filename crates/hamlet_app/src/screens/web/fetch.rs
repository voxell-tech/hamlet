//! Fetching the web engine, CEF, onto a machine that does not have
//! it, from CEF's own builds into the user's caches. The window
//! starts the engine when it opens, so it is used from the next
//! launch.

use std::path::Path;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Local, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::{Real, Time};
use hamlet_client::Tokio;
use tokio::sync::oneshot;

use super::engine;

/// How often the bytes fetched so far are read.
const COUNT_EVERY: Duration = Duration::from_millis(500);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Fetch>().add_systems(Update, follow);
}

/// Where fetching the engine has got to.
#[derive(Resource, Default)]
pub(super) enum Fetch {
    #[default]
    Idle,
    Fetching {
        done: oneshot::Receiver<Result<(), String>>,
        /// The bytes of the archive fetched so far.
        bytes: u64,
    },
    Fetched,
    /// It failed, for the reason given.
    Failed(String),
}

impl Fetch {
    /// What the panel shows of it.
    pub(super) fn state(&self) -> State {
        match self {
            Fetch::Idle => State::Idle,
            Fetch::Fetching { bytes, .. } => State::Fetching(*bytes),
            Fetch::Fetched => State::Fetched,
            Fetch::Failed(why) => State::Failed(why.clone()),
        }
    }
}

/// [`Fetch`] as the panel shows it.
#[derive(Clone, PartialEq)]
pub(super) enum State {
    Idle,
    /// The bytes fetched so far.
    Fetching(u64),
    Fetched,
    Failed(String),
}

/// Starts fetching the engine, unless it is already being fetched.
pub(super) fn start(world: &mut World) {
    if matches!(world.resource::<Fetch>(), Fetch::Fetching { .. }) {
        return;
    }
    let Some(folder) = engine::fetched() else {
        *world.resource_mut::<Fetch>() = Fetch::Failed(
            "there is no home folder to keep it in".into(),
        );
        return;
    };
    let (tx, done) = oneshot::channel();
    let tokio = world.resource::<Tokio>().0.clone();
    tokio.spawn_blocking(move || {
        let _ = tx.send(fetch(&folder).map_err(|e| e.to_string()));
    });
    *world.resource_mut::<Fetch>() =
        Fetch::Fetching { done, bytes: 0 };
}

/// Downloads this version's archive into `folder` and unpacks it
/// there, then removes the archive.
fn fetch(folder: &Path) -> Result<(), download_cef::Error> {
    let version = engine::version();
    let archive = download_cef::download_target_archive(
        download_cef::DEFAULT_TARGET,
        &version,
        folder,
        false,
    )?;
    download_cef::extract_target_archive(
        download_cef::DEFAULT_TARGET,
        &archive,
        folder,
        false,
    )?;
    let _ = std::fs::remove_file(archive);
    Ok(())
}

/// The bytes of the archive being downloaded into `folder`.
fn fetched_bytes(folder: &Path) -> u64 {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry.file_name().to_string_lossy().ends_with(".tar.bz2")
        })
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum()
}

/// Counts the bytes fetched, and notes when it ends.
fn follow(
    time: Res<Time<Real>>,
    mut fetch: ResMut<Fetch>,
    mut counted: Local<Duration>,
) {
    let Fetch::Fetching { done, bytes } = &mut *fetch else {
        return;
    };
    match done.try_recv() {
        Ok(Ok(())) => *fetch = Fetch::Fetched,
        Ok(Err(why)) => *fetch = Fetch::Failed(why),
        Err(oneshot::error::TryRecvError::Closed) => {
            *fetch =
                Fetch::Failed("it stopped before it ended".into());
        }
        Err(oneshot::error::TryRecvError::Empty) => {
            if time.elapsed() >= *counted + COUNT_EVERY {
                *counted = time.elapsed();
                let now = engine::fetched()
                    .map_or(0, |folder| fetched_bytes(&folder));
                if *bytes != now {
                    *bytes = now;
                }
            }
        }
    }
}
