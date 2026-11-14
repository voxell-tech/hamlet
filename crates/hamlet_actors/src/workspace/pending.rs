//! Calls that wait for a chief (H11, H34). A call the gate says needs
//! one keeps its row in `calls`, `waiting`, and what it runs if a
//! chief allows it in `waits`, both in one transaction. After the
//! commit the realm is told, and it asks a chief. The answer comes
//! back by a call from the realm, which checks the call again against
//! the reach in force before it runs.
//!
//! A wait stays through a restart, and nothing of it ran. Nothing
//! calls again when a call on either way is cut off: the wait stays,
//! and a repeat of the call answers that it waits.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_actor_core::{BindParam, ColumnValue, SqliteTransaction};
use hamlet_host::Address;
use hamlet_messages::agent::Outcome;
use hamlet_messages::realm::NeedsChief;
use hamlet_messages::workspace::files::{
    EditDiff, EditFile, Hashed, WriteFile,
};
use hamlet_messages::workspace::reach::{Need, Reach};
use hamlet_messages::workspace::{
    AgentNumber, Answer, AnswerTaken, CallId, CallRefusal, Changed,
    ChiefAnswer, ReachVersion, Run,
};
use hamlet_sandbox::scrub::scrub_text;
use serde::{Deserialize, Serialize};

use super::reach::{self, Gate, Op};
use super::{
    REALM, REALM_KEY, Workspace, call_actor, calls, read_only,
    realm_calling, tell, transact_if,
};

/// One row per call that waits for a chief, beside its `calls` row.
/// `request` is the call's own request, the only place a call's text
/// is kept; `base_hash` is what the file held, for a change.
pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS waits (
	agent INTEGER NOT NULL,
	call TEXT NOT NULL,
	needs BLOB NOT NULL,
	subject TEXT NOT NULL,
	request BLOB NOT NULL,
	reach_version INTEGER NOT NULL,
	base_hash BLOB,
	opened_at INTEGER NOT NULL,
	PRIMARY KEY (agent, call)) STRICT";

/// What a waiting call runs if a chief allows it: its own request,
/// never what the workspace read for it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) enum Pending {
    Run(Run),
    Write(WriteFile),
    Edit(EditFile),
}

/// A wait as it is stored.
pub(super) struct Wait {
    pub agent: AgentNumber,
    pub call: CallId,
    pub needs: Vec<Need>,
    pub request: Pending,
    pub reach_version: ReachVersion,
    pub base_hash: Option<Hashed>,
}

/// A wait to open, and what the realm is told of it.
pub(super) struct Opening {
    pub agent: AgentNumber,
    pub op: &'static str,
    pub request_hash: Vec<u8>,
    pub needs: Vec<Need>,
    pub request: Pending,
    pub reach_version: ReachVersion,
    pub base_hash: Option<Hashed>,
    /// The call in words, before the secrets are taken out.
    pub words: String,
    /// An edit's change, before the secrets are taken out.
    pub change: Option<EditDiff>,
}

/// Opens the wait of `opening` in `tx`, or refuses it when its
/// request holds one of `secrets`, which is never stored.
/// Returns what the realm is told once `tx` commits.
pub(super) async fn open(
    tx: &SqliteTransaction,
    secrets: &[String],
    opening: Opening,
) -> Result<Result<NeedsChief, CallRefusal>> {
    let (call, conversation, realm_task) = match &opening.request {
        Pending::Run(Run {
            call,
            conversation,
            realm_task,
            ..
        })
        | Pending::Write(WriteFile {
            call,
            conversation,
            realm_task,
            ..
        })
        | Pending::Edit(EditFile {
            call,
            conversation,
            realm_task,
            ..
        }) => {
            (call.clone(), conversation.clone(), realm_task.clone())
        }
    };
    let request = calls::cbor(&opening.request)?;
    if holds_a_secret(secrets, &request) {
        tracing::warn!(
            agent = opening.agent.0,
            call = call.0,
            "refused to keep a call that waits for a chief: it \
             holds one of the realm's secrets"
        );
        return Ok(Err(CallRefusal::HoldsSecret));
    }
    let scrub = |words: &str| scrub_text(secrets, words);
    let subject = scrub(&opening.words);
    let change = opening.change.map(|change| EditDiff {
        diff: scrub(&change.diff),
        patch: scrub(&change.patch),
        ..change
    });
    calls::insert(
        tx,
        opening.agent,
        &call,
        opening.op,
        opening.request_hash,
        "waiting",
        Some(calls::cbor(&opening.needs)?),
        None,
    )
    .await?;
    tx.execute(
        "INSERT INTO waits (agent, call, needs, subject, request, \
         reach_version, base_hash, opened_at) VALUES (?1, ?2, ?3, \
         ?4, ?5, ?6, ?7, ?8)",
        Some(vec![
            BindParam::Integer(i64::try_from(opening.agent.0)?),
            BindParam::Text(call.0.clone()),
            BindParam::Blob(calls::cbor(&opening.needs)?),
            BindParam::Text(subject.clone()),
            BindParam::Blob(request),
            BindParam::Integer(i64::try_from(
                opening.reach_version.0,
            )?),
            match &opening.base_hash {
                Some(hash) => BindParam::Blob(calls::cbor(hash)?),
                None => BindParam::Null,
            },
            BindParam::Integer(calls::now_ms()?),
        ]),
    )
    .await?;
    Ok(Ok(NeedsChief {
        agent: opening.agent,
        call,
        conversation,
        realm_task,
        needs: opening.needs,
        subject,
        change,
        reach_version: opening.reach_version,
    }))
}

/// Whether `request`, as encoded, holds any of `secrets`. A text is
/// kept whole in its encoding, so a secret in any of its fields is
/// found.
fn holds_a_secret(secrets: &[String], request: &[u8]) -> bool {
    secrets.iter().any(|secret| {
        !secret.is_empty()
            && request
                .windows(secret.len())
                .any(|window| window == secret.as_bytes())
    })
}

/// The wait of `agent`'s call `call`, in `tx`.
pub(super) async fn find(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    call: &CallId,
) -> Result<Option<Wait>> {
    let read = tx
        .execute(
            "SELECT needs, request, reach_version, base_hash FROM \
             waits WHERE agent = ?1 AND call = ?2",
            Some(vec![
                BindParam::Integer(i64::try_from(agent.0)?),
                BindParam::Text(call.0.clone()),
            ]),
        )
        .await?;
    let Some(row) = read.rows.first() else {
        return Ok(None);
    };
    let [
        ColumnValue::Blob(needs),
        ColumnValue::Blob(request),
        ColumnValue::Integer(reach_version),
        base_hash,
    ] = row.as_slice()
    else {
        bail!("a wait's row has another shape: {row:?}");
    };
    Ok(Some(Wait {
        agent,
        call: call.clone(),
        needs: ciborium::from_reader(needs.as_slice())?,
        request: ciborium::from_reader(request.as_slice())?,
        reach_version: ReachVersion(u64::try_from(*reach_version)?),
        base_hash: match base_hash {
            ColumnValue::Blob(hash) => {
                Some(ciborium::from_reader(hash.as_slice())?)
            }
            _ => None,
        },
    }))
}

pub(super) async fn settled(
    ctx: &Ctx<Workspace>,
    wait: &Wait,
    state: &str,
    answer: Option<Vec<u8>>,
    temp: Option<&str>,
) -> Result<Option<i64>> {
    let settled = transact_if(ctx, async |tx| {
        Ok(settle(tx, wait, state, answer, temp).await?.ok_or(()))
    })
    .await?;
    Ok(settled.ok())
}

/// Ends `wait` in `tx`: removes it, and sets its call's row to
/// `state` with `answer` and `temp`. Returns the row's id, or `None`
/// when another answer ended the wait first.
pub(super) async fn settle(
    tx: &SqliteTransaction,
    wait: &Wait,
    state: &str,
    answer: Option<Vec<u8>>,
    temp: Option<&str>,
) -> Result<Option<i64>> {
    let agent = BindParam::Integer(i64::try_from(wait.agent.0)?);
    let call = BindParam::Text(wait.call.0.clone());
    let removed = tx
        .execute(
            "DELETE FROM waits WHERE agent = ?1 AND call = ?2",
            Some(vec![agent.clone(), call.clone()]),
        )
        .await?;
    if removed.changes != 1 {
        return Ok(None);
    }
    let ended_at = match state {
        "refused" | "ended" => BindParam::Integer(calls::now_ms()?),
        _ => BindParam::Null,
    };
    let updated = tx
        .execute(
            "UPDATE calls SET state = ?3, answer = ?4, temp = ?5, \
             ended_at = ?6 WHERE agent = ?1 AND call = ?2 AND state \
             = 'waiting' RETURNING id",
            Some(vec![
                agent,
                call,
                BindParam::Text(state.to_owned()),
                answer.map_or(BindParam::Null, BindParam::Blob),
                temp.map_or(BindParam::Null, |temp| {
                    BindParam::Text(temp.to_owned())
                }),
                ended_at,
            ]),
        )
        .await?;
    match updated.rows.first().map(Vec::as_slice) {
        Some([ColumnValue::Integer(id)]) => Ok(Some(*id)),
        _ => bail!(
            "agent {}'s call {} waited with no waiting row",
            wait.agent.0,
            wait.call.0
        ),
    }
}

/// Ends every wait of `agent` with `refusal` in `tx`, as when its
/// reach ended: a chief's answer that comes after finds no wait.
/// Returns the calls whose waits it ended.
pub(super) async fn close(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    refusal: &CallRefusal,
) -> Result<Vec<CallId>> {
    let agent = BindParam::Integer(i64::try_from(agent.0)?);
    tx.execute(
        "DELETE FROM waits WHERE agent = ?1",
        Some(vec![agent.clone()]),
    )
    .await?;
    let closed = tx
        .execute(
            "UPDATE calls SET state = 'refused', answer = ?2, \
             ended_at = ?3 WHERE agent = ?1 AND state = 'waiting' \
             RETURNING call",
            Some(vec![
                agent,
                BindParam::Blob(calls::cbor(refusal)?),
                BindParam::Integer(calls::now_ms()?),
            ]),
        )
        .await?;
    closed
        .rows
        .iter()
        .map(|row| match row.as_slice() {
            [ColumnValue::Text(call)] => Ok(CallId(call.clone())),
            other => bail!("a closed call's row is {other:?}"),
        })
        .collect()
}

/// What the reach in force makes of a call a chief allowed.
pub(super) enum Rechecked {
    Pass {
        reach: Reach,
        /// The command runs with the network: the reach gives it, or
        /// the chief allowed it.
        network: bool,
        secrets: Arc<Vec<String>>,
    },
    Refused(CallRefusal),
}

impl Workspace {
    /// Tells the realm that a call waits for a chief. A failure is
    /// logged and the wait stays.
    pub(super) async fn ask_chief(
        &self,
        ctx: &Ctx<Self>,
        notice: NeedsChief,
    ) {
        let (agent, call) = (notice.agent, notice.call.clone());
        let to = Address::new(REALM, REALM_KEY);
        if let Err(error) = call_actor(ctx, to, &notice).await {
            tracing::warn!(
                ?error,
                agent = agent.0,
                call = call.0,
                "the realm was not told that a call waits for a \
                 chief"
            );
        }
    }

    /// Ends `wait` with `refusal`, and calls its agent with it.
    pub(super) async fn not_run(
        &self,
        ctx: &Ctx<Self>,
        wait: &Wait,
        refusal: CallRefusal,
    ) -> Result<AnswerTaken> {
        let answer = Some(calls::cbor(&refusal)?);
        if settled(ctx, wait, "refused", answer, None)
            .await?
            .is_none()
        {
            return Ok(AnswerTaken::NoWait);
        }
        tell(
            ctx,
            wait.agent,
            wait.call.clone(),
            Outcome::NotRun(refusal.clone()),
        )
        .await;
        Ok(AnswerTaken::NotRun(refusal))
    }

    /// Weighs `wait`, which a chief allowed, again under the reach in
    /// force (S18): it runs only what the chief was asked about.
    pub(super) async fn recheck(
        &self,
        ctx: &Ctx<Self>,
        wait: &Wait,
        op: &Op<'_>,
    ) -> Result<Rechecked> {
        let mut fetched = false;
        loop {
            let in_force = read_only(ctx, async |tx| {
                reach::in_force(tx, wait.agent).await
            })
            .await?;
            let (now, reach) = in_force
                .context("a call waits for an agent with no reach")?;
            let (held, secrets) = self.held();
            let changed = |why| {
                Ok(Rechecked::Refused(CallRefusal::ReachChanged {
                    since: wait.reach_version,
                    now,
                    why,
                }))
            };
            match reach::gate(
                &self.home,
                wait.agent,
                Some(&reach),
                held,
                op,
            ) {
                Gate::NeedsSecrets(version) => {
                    self.fetch_secrets(ctx, version, &mut fetched)
                        .await?;
                }
                Gate::Run { network } => {
                    return Ok(Rechecked::Pass {
                        reach,
                        network,
                        secrets,
                    });
                }
                Gate::NeedsChief(needs)
                    if needs
                        .iter()
                        .all(|need| wait.needs.contains(need)) =>
                {
                    // The chief allowed the network, or the reach
                    // gives it now.
                    let network = needs.contains(&Need::Network)
                        || reach
                            .footing()
                            .is_some_and(|footing| footing.network);
                    return Ok(Rechecked::Pass {
                        reach,
                        network,
                        secrets,
                    });
                }
                Gate::NeedsChief(needs) => {
                    return changed(Changed::Needs(needs));
                }
                Gate::Refuse(refusal) => {
                    return changed(Changed::Refused(refusal));
                }
            }
        }
    }
}

impl Handler<ChiefAnswer> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        answered: ChiefAnswer,
    ) -> Result<AnswerTaken> {
        if let Err(refusal) = realm_calling(&ctx, ChiefAnswer::NAME)?
        {
            return Ok(AnswerTaken::Refused(refusal));
        }
        let wait = read_only(&ctx, async |tx| {
            find(tx, answered.agent, &answered.call).await
        })
        .await?;
        let Some(wait) = wait else {
            return Ok(AnswerTaken::NoWait);
        };
        match answered.answer {
            Answer::Refuse { reason } => {
                self.not_run(
                    &ctx,
                    &wait,
                    CallRefusal::ChiefRefused { reason },
                )
                .await
            }
            Answer::Lapsed(lapse) => {
                self.not_run(&ctx, &wait, CallRefusal::Lapsed(lapse))
                    .await
            }
            Answer::Allow => match &wait.request {
                Pending::Run(run) => {
                    self.clone().grant_run(&ctx, &wait, run).await
                }
                Pending::Write(_) | Pending::Edit(_) => {
                    self.grant_change(&ctx, &wait).await
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_found_in_any_text_of_a_request() {
        let secrets = vec![String::new(), "s3cr3t".to_owned()];
        assert!(holds_a_secret(&secrets, b"echo s3cr3t; ls"));
        assert!(!holds_a_secret(&secrets, b"echo s3cr; ls"));
        assert!(!holds_a_secret(&[String::new()], b"anything"));
    }
}
