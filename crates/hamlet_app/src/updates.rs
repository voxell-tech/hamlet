//! Updates looked for while the window is open: shortly after it
//! opens and every hour, downloaded, and offered in a toast that
//! stays until the chief restarts or closes it
//! (docs/releases.md#updates).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::app::{App, PreUpdate, Startup};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
use hamlet_client::Tokio;
use hamlet_protocol::Version;
use hamlet_ui::theme::StatusKind;
use hamlet_update::{Feed, Install};
use reqwest::Client;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::PanelSystems;
use crate::toasts::Toasts;

/// The wait after the window opens before the first look, which
/// leaves the start to itself.
const FIRST: Duration = Duration::from_secs(15);

/// The wait between looks.
const EVERY: Duration = Duration::from_secs(60 * 60);

/// A newer Hamlet.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Offer {
    pub version: Version,
    pub ready: Ready,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Ready {
    /// Downloaded and checked, at this path.
    File(PathBuf),
    /// The release's page: this copy cannot update itself.
    Page(String),
}

/// The offers the looks send.
#[derive(Resource)]
pub(crate) struct Updates {
    rx: UnboundedReceiver<Offer>,
    /// The version whose offer the chief closed, not offered again
    /// while the window is open.
    pub declined: Option<Version>,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, begin)
        .add_systems(PreUpdate, offer.in_set(PanelSystems::Bind));
}

/// Says when the last update did not take, and starts looking.
fn begin(
    mut commands: Commands,
    tokio: Option<Res<Tokio>>,
    time: Res<Time>,
    toasts: Option<ResMut<Toasts>>,
) {
    if !hamlet_update::is_enabled() {
        return;
    }
    if let (Some(version), Some(mut toasts)) =
        (hamlet_update::missed(), toasts)
    {
        toasts.note(
            StatusKind::Danger,
            "Hamlet did not update".into(),
            format!(
                "The update to {version} did not install. Hamlet \
                 offers it again."
            ),
            time.elapsed_secs(),
        );
    }
    let (Some(tokio), Some(feed)) = (tokio, Feed::here()) else {
        return;
    };
    let (tx, rx) = unbounded_channel();
    tokio.0.spawn(watch(feed, Install::here(), tx));
    commands.insert_resource(Updates { rx, declined: None });
}

/// Looks for an update after [`FIRST`], then every [`EVERY`], and
/// sends each one found.
async fn watch(
    feed: Feed,
    install: Install,
    tx: UnboundedSender<Offer>,
) {
    tokio::time::sleep(FIRST).await;
    let client = match hamlet_update::client() {
        Ok(client) => client,
        Err(error) => {
            bevy::log::warn!("Cannot look for updates: {error}");
            return;
        }
    };
    loop {
        match look(&feed, &client, &install).await {
            Ok(Some(offer)) => {
                if tx.send(offer).is_err() {
                    return;
                }
            }
            Ok(None) => {}
            Err(error) => {
                bevy::log::warn!(
                    "Cannot look for updates: {error:#}"
                );
            }
        }
        tokio::time::sleep(EVERY).await;
    }
}

/// A newer release, downloaded when this copy can put it in place.
async fn look(
    feed: &Feed,
    client: &Client,
    install: &Install,
) -> Result<Option<Offer>> {
    let Some(found) = feed
        .check(client, install, hamlet_update::current())
        .await?
    else {
        return Ok(None);
    };
    let ready = match (found.asset, hamlet_update::folder()) {
        (Some(asset), Some(folder)) => Ready::File(
            hamlet_update::download(client, &asset, &folder).await?,
        ),
        _ => Ready::Page(found.page),
    };
    Ok(Some(Offer {
        version: found.version,
        ready,
    }))
}

/// Toasts the offers the looks sent, except the one declined.
fn offer(
    time: Res<Time>,
    updates: Option<ResMut<Updates>>,
    mut toasts: ResMut<Toasts>,
) {
    let Some(mut updates) = updates else {
        return;
    };
    while let Ok(offer) = updates.rx.try_recv() {
        if updates.declined.as_ref() != Some(&offer.version) {
            toasts.offer(offer, time.elapsed_secs());
        }
    }
}

/// Closes the offer of `version` for as long as the window is open.
pub(crate) fn decline(world: &mut World, version: Version) {
    if let Some(mut updates) = world.get_resource_mut::<Updates>() {
        updates.declined = Some(version);
    }
}

/// Hands the update in `file` to the launcher beside the window,
/// which stops the server, puts it in place and opens Hamlet again,
/// and closes the window.
pub(crate) fn restart(
    world: &mut World,
    version: Version,
    file: &Path,
) {
    match hand_over(version, file) {
        Ok(()) => crate::saving::close(world),
        Err(error) => {
            let now = world.resource::<Time>().elapsed_secs();
            world.resource_mut::<Toasts>().note(
                StatusKind::Danger,
                "Hamlet could not restart".into(),
                format!("{error:#}"),
                now,
            );
        }
    }
}

/// Notes the attempt and starts `hamlet_launch --update file`.
fn hand_over(version: Version, file: &Path) -> Result<()> {
    let launcher = std::env::current_exe()?.with_file_name(format!(
        "hamlet_launch{}",
        std::env::consts::EXE_SUFFIX
    ));
    let mut command = Command::new(&launcher);
    command
        .arg("--update")
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Its own group, so it outlives a terminal the window was opened
    // from.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(
        &mut command,
        0,
    );
    hamlet_update::attempting(version)?;
    command.spawn().with_context(|| {
        format!("Cannot run {}", launcher.display())
    })?;
    Ok(())
}
