// This file was changed for Hamlet. See NOTICE in this crate.
//! Individual actor forks: the lineage a forked actor carries in
//! `_actor_meta`, and the one run of the fork hook at the first start
//! of each fork.
//!
//! The source makes the copy with [`ActorContext::copy_for_fork`],
//! which writes into it the fork state the copy takes, under
//! `fork_start` in `_actor_meta`. A start that finds that row removes
//! the timers and the run wake copied from the source, runs the
//! runtime's fork hook, and records the fork state and removes the
//! row in one commit. A crash before that commit leaves the row, and
//! the next start does it all again.
//!
//! The hook's state edits and the applied fork are one commit on
//! every SQLite backend, as long as the hook leaves them to the
//! startup save. On native SQLite its SQL writes, and any save
//! it starts itself, are held in that commit too; a save still in
//! flight at the startup save is waited for, so the commit carries
//! the hook's final state. On remote SQLite (wasm runtimes)
//! SQL writes and state transactions the hook commits itself land at
//! once, without the fork record, so a crash or a failed start before
//! the fork is recorded runs the hook again on top of them, and the
//! hook must be idempotent there. A save the hook only requests is
//! folded into the startup save. On native SQLite the hook's writes
//! must fit one commit (`commit_page_cap`): a transaction that would
//! take them past it fails as too large instead of being committed
//! early by the VFS, so nothing the hook wrote lands without the fork
//! record. A new actor's held startup writes differ: the VFS commits
//! those held so far early instead.

use anyhow::{Context, Result};
use hamlet_actor_wire::persist::generated::fork_v1;
use hamlet_actor_wire::persist::versioned as persist_versioned;

use crate::ActorContext;
use crate::actor::persist::{
    decode_latest_with_embedded_version,
    encode_latest_with_embedded_version,
};
use crate::sqlite::BindParam;
use crate::types::{ActorKey, format_actor_key};

/// The `_actor_meta` key of the actor's fork state.
pub(crate) const FORK_STATE_META_KEY: &str = "fork_state";

/// The `_actor_meta` key of the fork a copy applies at its next
/// start: the fork state the copy takes once it is applied.
pub(crate) const FORK_START_META_KEY: &str = "fork_start";

const FORK_STATE_VERSION: u16 = 1;

const COPY_SQL: &str = "VACUUM INTO ?";
const ATTACH_COPY_SQL: &str = "ATTACH ? AS fork";
const WRITE_COPY_FORK_START_SQL: &str =
    "INSERT INTO fork._actor_meta (key, value) VALUES (?, ?) ON \
     CONFLICT(key) DO UPDATE SET value = excluded.value";
const DETACH_COPY_SQL: &str = "DETACH fork";

impl ActorContext {
    /// Copies this actor's database to `path`, a file that is absent
    /// or empty, as the individual actor fork `fork_id` of this
    /// actor, which the copy's next start applies. The caller syncs
    /// the copy and puts it in place.
    pub async fn copy_for_fork(
        &self,
        path: &str,
        fork_id: Vec<u8>,
    ) -> Result<()> {
        let mut chain = self
            .fork_chain()
            .iter()
            .map(stored_link)
            .collect::<Vec<_>>();
        chain.push(fork_v1::ForkLink {
            actor_id: self.actor_id().to_owned().into(),
            key: stored_key(self.key()),
            forked_at: self.schedule_now_timestamp_ms(),
        });
        let start = encode_state(&fork_v1::ForkState {
            applied_fork_id: Some(fork_id.into()),
            chain,
        })?;
        let db = self.sql();
        let path = || Some(vec![BindParam::Text(path.to_owned())]);
        db.execute(COPY_SQL, path())
            .await
            .context("copy the actor's database")?;
        db.execute(ATTACH_COPY_SQL, path())
            .await
            .context("open the copy")?;
        let written = db
            .execute(
                WRITE_COPY_FORK_START_SQL,
                Some(vec![
                    BindParam::Text(FORK_START_META_KEY.to_owned()),
                    BindParam::Blob(start),
                ]),
            )
            .await
            .context("write the fork into the copy");
        // Detached whatever the write did, so the copy is not held
        // open and the next copy can attach under the same name.
        db.execute(DETACH_COPY_SQL, None)
            .await
            .context("close the copy")?;
        written?;
        Ok(())
    }
}

/// One individual actor fork behind an actor: the source and when the
/// fork was made.
#[derive(Clone, Debug, PartialEq)]
pub struct ForkLink {
    pub actor_id: String,
    pub key: ActorKey,
    /// Milliseconds since the epoch.
    pub forked_at: i64,
}

/// A fork whose hook runs in this start: the link it adds to the
/// chain and the fork input.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorForkStart {
    pub link: ForkLink,
    pub input: Option<Vec<u8>>,
}

/// The actor's fork state as stored, empty for an actor that was
/// never forked.
pub(crate) fn empty_state() -> fork_v1::ForkState {
    fork_v1::ForkState {
        applied_fork_id: None,
        chain: Vec::new(),
    }
}

pub(crate) fn decode_state(
    bytes: &[u8],
) -> Result<fork_v1::ForkState> {
    decode_latest_with_embedded_version::<persist_versioned::ForkState>(
        bytes,
        "fork state",
    )
}

pub(crate) fn encode_state(
    state: &fork_v1::ForkState,
) -> Result<Vec<u8>> {
    encode_latest_with_embedded_version::<persist_versioned::ForkState>(
        state.clone(),
        FORK_STATE_VERSION,
        "fork state",
    )
}

/// The fork a start applies, from the `fork_start` row it read, as
/// the fork state to record once its hook ran and the start the
/// runtime receives. `None` when the database holds no fork to apply.
pub(crate) fn pending(
    fork_start: Option<Vec<u8>>,
    input: Option<Vec<u8>>,
) -> Result<Option<(fork_v1::ForkState, ActorForkStart)>> {
    let Some(bytes) = fork_start else {
        return Ok(None);
    };
    let next =
        decode_state(&bytes).context("decode the fork to apply")?;
    let link = next
        .chain
        .last()
        .map(link_of)
        .context("the fork to apply has no link")?;
    Ok(Some((next, ActorForkStart { link, input })))
}

/// The chain the runtime reads, oldest first.
pub(crate) fn chain_of(state: &fork_v1::ForkState) -> Vec<ForkLink> {
    state.chain.iter().map(link_of).collect()
}

fn stored_link(link: &ForkLink) -> fork_v1::ForkLink {
    fork_v1::ForkLink {
        actor_id: link.actor_id.clone().into(),
        key: stored_key(&link.key),
        forked_at: link.forked_at,
    }
}

/// A key in the slash form a fork link stores, none for an empty key.
fn stored_key(key: &ActorKey) -> Option<vbare::Str> {
    (!key.is_empty()).then(|| format_actor_key(key).into())
}

fn link_of(stored: &fork_v1::ForkLink) -> ForkLink {
    ForkLink {
        actor_id: stored.actor_id.to_string(),
        key: crate::host::actor_key_from_protocol(
            stored.key.as_ref().map(ToString::to_string),
        ),
        forked_at: stored.forked_at,
    }
}

/// Reads the fork state a startup row carries, empty when the actor
/// was never forked.
pub(crate) fn state_from_row(
    bytes: Option<Vec<u8>>,
) -> Result<fork_v1::ForkState> {
    match bytes {
        Some(bytes) => {
            decode_state(&bytes).context("decode actor fork state")
        }
        None => Ok(empty_state()),
    }
}
