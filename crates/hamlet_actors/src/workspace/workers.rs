//! Workers' scratch folders under `workers/` (H42): made
//! when a worker's reach arrives, and again before its call when one
//! is missing; removed when its reach ends, once its commands have
//! stopped; and swept at the workspace's start when a kill left one
//! behind.

use std::io;

use anyhow::{Context, Result};
use hamlet_actor::Ctx;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Reach;

use super::Workspace;
use super::reach::{self, RealmFolder};

/// Makes `worker`'s scratch folder, which only the realm's owner
/// opens, when it is missing.
pub(super) fn make(
    home: &RealmFolder,
    worker: AgentNumber,
) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(home.scratch(worker))
}

/// Removes `worker`'s scratch folder and all in it. A failure is
/// logged: the next start sweeps the folder.
pub(super) async fn remove(home: &RealmFolder, worker: AgentNumber) {
    let folder = home.scratch(worker);
    let removed = tokio::task::spawn_blocking(move || {
        std::fs::remove_dir_all(&folder)
    })
    .await;
    let error = match removed {
        Ok(Ok(())) => return,
        Ok(Err(error)) if error.kind() == io::ErrorKind::NotFound => {
            return;
        }
        Ok(Err(error)) => error.to_string(),
        Err(error) => error.to_string(),
    };
    tracing::warn!(
        %error,
        worker = worker.0,
        "remove a worker's scratch folder"
    );
}

/// Removes the scratch folder of every worker whose reach ended, as a
/// kill before its removal leaves it. A folder of a worker whose
/// reach lives stays, and so does one of no agent the workspace
/// knows: the workspace makes a folder only after its reach.
pub(super) async fn sweep(
    ctx: &Ctx<Workspace>,
    home: &RealmFolder,
) -> Result<()> {
    let entries = match std::fs::read_dir(home.workers()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => {
            return Err(error).context("read the workers' folder");
        }
    };
    let workers = entries
        .filter_map(|entry| {
            entry.ok()?.file_name().to_str()?.parse().ok()
        })
        .map(AgentNumber)
        .collect::<Vec<AgentNumber>>();
    let ended = super::read_only(ctx, async |tx| {
        let mut ended = Vec::new();
        for worker in workers {
            if reach::read(tx, worker).await? == Some(Reach::Ended) {
                ended.push(worker);
            }
        }
        Ok(ended)
    })
    .await?;
    for worker in ended {
        remove(home, worker).await;
    }
    Ok(())
}
