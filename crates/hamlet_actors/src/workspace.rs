//! The `workspace` actor, one per machine: it runs the agents'
//! commands and file changes under the sandbox, within each agent's
//! reach.

pub mod reach;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory, BindParam};
use hamlet_host::{Caller, HostRef};
use hamlet_sandbox::Sandbox;
use serde::{Deserialize, Serialize};

use self::reach::{Op, Reach};

pub const KIND: &str = "workspace";

/// The workspace's key.
pub const MACHINE: &str = "local";

/// The kind of the realm, the only caller that sets a reach.
pub const REALM: &str = "realm";

/// The kind of an agent, keyed by its [`AgentNumber`].
pub const AGENT: &str = "agent";

/// Callers the workspace refused, so that one can be found later.
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS refusals (
	id INTEGER PRIMARY KEY,
	caller TEXT NOT NULL,
	action TEXT NOT NULL,
	why TEXT NOT NULL,
	at INTEGER NOT NULL) STRICT";

/// The workspace's kind. Its host gives it the [`Sandbox`] its
/// commands run in, as a value of [`hamlet_host::HostOptions`].
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Workspace>(
        KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(KIND)
        .expect("the registry holds the kind it registered")
}

struct Workspace;

#[async_trait::async_trait]
impl Actor for Workspace {
    type Input = ();
    type Actions = (SetReach,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        HostRef::of(ctx.inner())?
            .value::<Sandbox>()
            .context("the host gave the workspace no sandbox")?;
        ctx.sql()
            .exec(format!("{SCHEMA};{}", reach::SCHEMA))
            .await?;
        Ok(Self)
    }
}

impl Handler<SetReach> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        set: SetReach,
    ) -> Result<ReachAnswer> {
        let caller = caller(&ctx)?;
        if !is_the_realm(caller.as_ref()) {
            let refusal = ReachRefusal::NotTheRealm;
            refused(&ctx, caller.as_ref(), SetReach::NAME, &refusal)
                .await?;
            return Ok(ReachAnswer::Refused(refusal));
        }
        let tx = ctx.sql().begin_transaction(None).await?;
        let in_force = reach::apply(&tx, &set).await?;
        tx.commit().await?;
        Ok(ReachAnswer::InForce(in_force))
    }
}

/// Who made the call the handler runs for, as the host stamped it.
fn caller(ctx: &Ctx<Workspace>) -> Result<Option<Caller>> {
    ctx.conn().context("the call has no connection")?.params()
}

fn is_the_realm(caller: Option<&Caller>) -> bool {
    matches!(caller, Some(Caller::Actor(from)) if from.kind == REALM)
}

/// The agent a call of an agent's tool is for: the stamped caller,
/// which must be an agent and the one the request names (W2).
pub fn agent_of(
    caller: Option<&Caller>,
    named: AgentNumber,
) -> Result<AgentNumber, CallerRefusal> {
    let Some(Caller::Actor(from)) = caller else {
        return Err(CallerRefusal::NotAnAgent);
    };
    if from.kind != AGENT {
        return Err(CallerRefusal::NotAnAgent);
    }
    let caller = from
        .key
        .parse()
        .map(AgentNumber)
        .map_err(|_| CallerRefusal::NotAnAgent)?;
    if caller == named {
        Ok(caller)
    } else {
        Err(CallerRefusal::NotTheCaller { named, caller })
    }
}

/// Records that `caller` was refused `action`, and why.
async fn refused(
    ctx: &Ctx<Workspace>,
    caller: Option<&Caller>,
    action: &str,
    why: &impl std::fmt::Display,
) -> Result<()> {
    let caller = match caller {
        Some(Caller::Actor(from)) => {
            format!("{}/{}", from.kind, from.key)
        }
        Some(Caller::Client) => "client".to_owned(),
        None => "none".to_owned(),
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    ctx.sql()
        .execute(
            "INSERT INTO refusals (caller, action, why, at) VALUES \
             (?1, ?2, ?3, ?4)",
            Some(vec![
                BindParam::Text(caller),
                BindParam::Text(action.to_owned()),
                BindParam::Text(why.to_string()),
                BindParam::Integer(i64::try_from(now.as_millis())?),
            ]),
        )
        .await?;
    Ok(())
}

/// The realm's version of one agent's reach, from 1.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub struct ReachVersion(pub u64);

/// Sets an agent's reach when `version` is newer than the one in
/// force. Called by the realm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetReach {
    pub agent: AgentNumber,
    pub version: ReachVersion,
    pub reach: Reach,
}

impl Action for SetReach {
    type Output = ReachAnswer;
    const NAME: &'static str = "set_reach";
}

/// The workspace's answer to a [`SetReach`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachAnswer {
    /// The agent's version in force after the call.
    InForce(ReachVersion),
    Refused(ReachRefusal),
}

/// A [`SetReach`] the workspace does not take.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
pub enum ReachRefusal {
    #[error("only the realm sets a reach")]
    NotTheRealm,
}

/// A call of an agent's tool from a caller it is not for (W2).
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
pub enum CallerRefusal {
    #[error("only an agent calls for its tools")]
    NotAnAgent,
    #[error("agent {} called for agent {}", caller.0, named.0)]
    NotTheCaller {
        named: AgentNumber,
        caller: AgentNumber,
    },
}

/// The number in an agent actor's file name,
/// `actors/agent/<n>.sqlite`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct AgentNumber(pub u64);

/// The id an agent gives a tool call. A repeat of the call carries
/// the same id.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct CallId(pub String);

/// A conversation of an agent, which trust is given for (H11).
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct ConversationId(pub String);

/// The realm's task a call is made for, which counts a chief's
/// refusals. Not the id of the call's own durable run.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct RealmTask(pub String);

/// The shell a command's script is written for.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Shell {
    Bash,
}

/// Runs a script for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// `None` when no chief's instruction started the work.
    pub realm_task: Option<RealmTask>,
    pub shell: Shell,
    pub script: String,
    /// The model said the command needs the network (C19).
    pub network: bool,
    /// Shortens the 120 s limit (C16).
    pub timeout: Option<Duration>,
}

/// Creates or replaces a whole file for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteFile {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// `None` when no chief's instruction started the work.
    pub realm_task: Option<RealmTask>,
    pub path: PathBuf,
    pub content: String,
}

/// Makes exact text replacements in one file for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditFile {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// `None` when no chief's instruction started the work.
    pub realm_task: Option<RealmTask>,
    pub path: PathBuf,
    pub edits: Vec<Replace>,
}

/// One replacement of an [`EditFile`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replace {
    pub old: String,
    pub new: String,
}

impl Run {
    pub fn op(&self) -> Op<'_> {
        Op::Command {
            conversation: &self.conversation,
            script: &self.script,
            network: self.network,
        }
    }
}

impl WriteFile {
    pub fn op(&self) -> Op<'_> {
        Op::Change {
            conversation: &self.conversation,
            path: &self.path,
        }
    }
}

impl EditFile {
    pub fn op(&self) -> Op<'_> {
        Op::Change {
            conversation: &self.conversation,
            path: &self.path,
        }
    }
}

#[cfg(test)]
mod tests {
    use hamlet_host::Address;

    use super::*;

    fn actor(kind: &str, key: &str) -> Option<Caller> {
        Some(Caller::Actor(Address::new(kind, key)))
    }

    #[test]
    fn a_call_is_for_the_agent_that_made_it() {
        assert_eq!(
            agent_of(actor(AGENT, "7").as_ref(), AgentNumber(7)),
            Ok(AgentNumber(7))
        );
    }

    #[test]
    fn an_agent_that_names_another_is_refused() {
        assert_eq!(
            agent_of(actor(AGENT, "8").as_ref(), AgentNumber(7)),
            Err(CallerRefusal::NotTheCaller {
                named: AgentNumber(7),
                caller: AgentNumber(8),
            })
        );
    }

    #[test]
    fn only_an_agent_calls_for_its_tools() {
        for caller in [
            actor(REALM, "7"),
            actor(AGENT, "hand"),
            Some(Caller::Client),
            None,
        ] {
            assert_eq!(
                agent_of(caller.as_ref(), AgentNumber(7)),
                Err(CallerRefusal::NotAnAgent),
                "{caller:?}"
            );
        }
    }
}
