//! The `workspace` actor, one per machine: it runs the agents'
//! commands and file changes under the sandbox, within each agent's
//! reach.

pub mod reach;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use hamlet_actor::{Actor, Ctx, Registry};
use hamlet_actor_core::{
    ActorConfig, ActorFactory, SqliteTransaction,
};
use hamlet_host::send::{self, Message};
use serde::{Deserialize, Serialize};

use self::reach::{Op, Reach};

pub const KIND: &str = "workspace";

/// The workspace's key.
pub const MACHINE: &str = "local";

/// The only kind whose sends set a reach (W2).
const REALM: &str = "realm";

/// The workspace's kind, receiving sends.
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Workspace>(
        KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    let layer = registry
        .into_factories()
        .remove(KIND)
        .expect("the registry holds the kind it registered");
    send::receiving(layer, receive)
}

struct Workspace;

#[async_trait::async_trait]
impl Actor for Workspace {
    type Input = ();
    type Actions = ();
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        ctx.sql().exec(reach::SCHEMA.to_owned()).await?;
        Ok(Self)
    }
}

/// Applies a send in its receive transaction. A send the workspace
/// does not take is answered with a [`SendRefusal`] and changes
/// nothing, so it is not tried again.
async fn receive(
    tx: SqliteTransaction,
    message: Message,
) -> Result<Vec<u8>> {
    let refusal = match message.name.as_str() {
        SetReach::NAME if message.from.kind == REALM => {
            let set: SetReach =
                ciborium::from_reader(message.payload.as_slice())?;
            let in_force = reach::apply(&tx, &set).await?;
            let applied = ReachApplied {
                agent: set.agent,
                sent: set.version,
                in_force,
            };
            send::send(
                &tx,
                &message.from,
                ReachApplied::NAME,
                &encode(&applied)?,
                &message.follow(),
            )
            .await?;
            return Ok(Vec::new());
        }
        SetReach::NAME => SendRefusal::WrongSender {
            kind: message.from.kind.clone(),
        },
        name => SendRefusal::Unknown {
            name: name.to_owned(),
        },
    };
    tracing::warn!(send = message.id(), %refusal, "workspace refused a send");
    encode(&refusal)
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes)?;
    Ok(bytes)
}

/// A send the workspace takes no part of.
#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
pub enum SendRefusal {
    #[error("a send of kind `{kind}` may not set a reach")]
    WrongSender { kind: String },
    #[error("the workspace takes no send `{name}`")]
    Unknown { name: String },
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

/// Sets an agent's reach, when `version` is newer than the one in
/// force. Sent by the realm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetReach {
    pub agent: AgentNumber,
    pub version: ReachVersion,
    pub reach: Reach,
}

impl SetReach {
    pub const NAME: &str = "set_reach";
}

/// The workspace's answer to a [`SetReach`]: the version it was sent
/// and the version in force after it. Sent to the realm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachApplied {
    pub agent: AgentNumber,
    pub sent: ReachVersion,
    pub in_force: ReachVersion,
}

impl ReachApplied {
    pub const NAME: &str = "reach_applied";
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
