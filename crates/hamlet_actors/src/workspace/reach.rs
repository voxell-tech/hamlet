//! Each agent's reach on this machine (C18 to C21, H11): what its
//! tool calls may touch and when they wait for a chief. The realm is
//! the reach's only author. The workspace applies it, passes each
//! call through [`gate`], and runs it under the policy of
//! [`placement`].

use std::io;
use std::path::{Path, PathBuf};

use hamlet_actor_core::{BindParam, ColumnValue, SqliteTransaction};
use hamlet_messages::address::LOCAL;
use hamlet_messages::workspace::reach::{
    Footing, Mode, Need, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::refusal::{
    PathRefusal, ReachRefusal, ReadRefusal, ToChange, ToRead,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, ReachVersion, SetReach,
};
use hamlet_os::fs::path::{normalize, text};
use hamlet_sandbox::{
    Policy, base_env, host_env, is_within, real_path,
};

// Copied from the server's `oversight/gate.rs` and `tools/shell.rs`.
/// Commands that reach the network, by the words they name, in lower
/// case.
const NETWORK_WORDS: &[&str] = &[
    "curl",
    "wget",
    "http://",
    "https://",
    "ssh",
    "scp",
    "ftp",
    "ping",
    "git clone",
    "git fetch",
    "git pull",
    "git push",
    "pip install",
    "pip3 install",
    "npm install",
    "cargo install",
    "rsync",
    "nc",
    "brew install",
];

/// Each agent's reach in force: one row per agent, never deleted, so
/// an older version that arrives late is ignored.
pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS reach (
	agent INTEGER PRIMARY KEY,
	version INTEGER NOT NULL,
	body BLOB NOT NULL,
	applied_at INTEGER NOT NULL) STRICT";

/// Keeps `set` when it is newer than the agent's reach in force, in
/// `tx`. Returns the version in force after it.
pub(super) async fn apply(
    tx: &SqliteTransaction,
    set: &SetReach,
) -> anyhow::Result<ReachVersion> {
    let agent = BindParam::Integer(i64::from(set.agent.0));
    let body = crate::cbor(&set.reach)?;
    tx.execute(
        "INSERT INTO reach (agent, version, body, applied_at) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT (agent) DO UPDATE SET \
         version = excluded.version, body = excluded.body, \
         applied_at = excluded.applied_at WHERE excluded.version > \
         reach.version",
        Some(vec![
            agent.clone(),
            BindParam::Integer(i64::try_from(set.version.0)?),
            BindParam::Blob(body),
            BindParam::Integer(super::calls::now_ms()?),
        ]),
    )
    .await?;
    let read = tx
        .execute(
            "SELECT version FROM reach WHERE agent = ?1",
            Some(vec![agent]),
        )
        .await?;
    match read.rows.first().and_then(|row| row.first()) {
        Some(ColumnValue::Integer(version)) => {
            Ok(ReachVersion(u64::try_from(*version)?))
        }
        _ => anyhow::bail!("agent {} has no reach row", set.agent.0),
    }
}

/// `agent`'s reach in force, in `tx`.
pub(super) async fn read(
    tx: &SqliteTransaction,
    agent: AgentNumber,
) -> anyhow::Result<Option<Reach>> {
    Ok(in_force(tx, agent).await?.map(|(_, reach)| reach))
}

/// `agent`'s reach in force and its version, in `tx`.
pub(super) async fn in_force(
    tx: &SqliteTransaction,
    agent: AgentNumber,
) -> anyhow::Result<Option<(ReachVersion, Reach)>> {
    let read = tx
        .execute(
            "SELECT version, body FROM reach WHERE agent = ?1",
            Some(vec![BindParam::Integer(i64::from(agent.0))]),
        )
        .await?;
    match read.rows.first().map(Vec::as_slice) {
        Some(
            [ColumnValue::Integer(version), ColumnValue::Blob(body)],
        ) => Ok(Some((
            ReachVersion(u64::try_from(*version)?),
            ciborium::from_reader(body.as_slice())?,
        ))),
        None => Ok(None),
        Some(other) => {
            anyhow::bail!("agent {}'s reach is {other:?}", agent.0)
        }
    }
}

/// The realm folder, `HAMLET_HOME`, as its real path, links followed,
/// and as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RealmFolder {
    real: PathBuf,
    written: PathBuf,
}

impl RealmFolder {
    pub fn new(path: &Path) -> io::Result<Self> {
        let written = std::path::absolute(path)?;
        Ok(Self {
            real: real_path(&written),
            written,
        })
    }

    pub fn root(&self) -> &Path {
        &self.real
    }

    /// `actors/agent/<n>.fs/`, the agent's filesystem. The only way
    /// the workspace names one.
    pub fn filesystem(&self, agent: AgentNumber) -> PathBuf {
        self.real
            .join("actors/agent")
            .join(format!("{}.fs", agent.0))
    }

    /// `workers/`, which holds the workers' scratch folders.
    pub fn workers(&self) -> PathBuf {
        self.real.join("workers")
    }

    /// `workers/<n>/`, a worker's scratch folder.
    pub fn scratch(&self, worker: AgentNumber) -> PathBuf {
        self.workers().join(worker.0.to_string())
    }

    /// The folder of `agent`'s spilled command output.
    pub fn spill(&self, agent: AgentNumber) -> PathBuf {
        self.real
            .join("actors/workspace")
            .join(format!("{LOCAL}.output"))
            .join(agent.0.to_string())
    }

    fn library(&self) -> PathBuf {
        self.real.join("library")
    }

    /// Tools a chief adds by hand (C17).
    pub fn tools(&self) -> PathBuf {
        self.real.join("tools")
    }

    fn has_inside(&self, path: &Path) -> bool {
        is_within(&real_path(path), &self.real)
    }

    /// Checks whether `path`, as written, is in an agent's filesystem
    /// under either spelling of the realm folder.
    fn is_in_a_filesystem(&self, path: &Path) -> bool {
        [&self.real, &self.written].into_iter().any(|root| {
            path.strip_prefix(root.join("actors/agent"))
                .ok()
                .and_then(|rest| rest.components().next())
                .is_some_and(|first| {
                    first
                        .as_os_str()
                        .to_string_lossy()
                        .ends_with(".fs")
                })
        })
    }
}

/// A call as the gate weighs it. Paths arrive absolute.
#[derive(Clone, Copy, Debug)]
pub enum Op<'a> {
    Command {
        conversation: &'a ConversationId,
        script: &'a str,
        network: bool,
    },
    /// A tool added by hand, whose manifest says whether it reaches
    /// the network.
    Tool {
        conversation: &'a ConversationId,
        network: bool,
    },
    Change {
        conversation: &'a ConversationId,
        path: &'a Path,
    },
    Read {
        path: &'a Path,
    },
    /// A read of a file a letter of the agent's sends.
    Parcel {
        path: &'a Path,
    },
}

impl Op<'_> {
    /// The purpose a file tool's `op` is placed under.
    pub fn purpose(&self) -> Purpose {
        match self {
            Self::Parcel { .. } => Purpose::Parcel,
            _ => Purpose::FileTool,
        }
    }
}

/// The gate's verdict on a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    Run {
        network: Network,
    },
    NeedsChief(Vec<Need>),
    Refuse(Refused),
    /// The workspace's list of secrets is older than this version:
    /// fetch it from the realm, then pass the gate again.
    NeedsSecrets(SecretsVersion),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    Reach(ReachRefusal),
    Read(PathRefusal<ToRead>),
    Change(PathRefusal<ToChange>),
}

impl Refused {
    pub fn of_program(self) -> anyhow::Result<ReachRefusal> {
        match self {
            Self::Reach(why) => Ok(why),
            Self::Read(_) | Self::Change(_) => {
                anyhow::bail!("the gate refused a program by a path")
            }
        }
    }

    pub fn of_read(self) -> anyhow::Result<ReadRefusal> {
        match self {
            Self::Reach(why) => Ok(why.into()),
            Self::Read(why) => Ok(why.into()),
            Self::Change(_) => {
                anyhow::bail!("the gate refused a read as a change")
            }
        }
    }

    pub fn of_change<R>(self) -> anyhow::Result<R>
    where
        R: From<ReachRefusal> + From<PathRefusal<ToChange>>,
    {
        match self {
            Self::Reach(why) => Ok(why.into()),
            Self::Change(why) => Ok(why.into()),
            Self::Read(_) => {
                anyhow::bail!("the gate refused a change as a read")
            }
        }
    }
}

/// A program's access to the network, as the gate lets it run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    On,
    Off,
}

/// A policy's purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Command { network: Network },
    FileTool,
    /// The read of the files an agent's letter sends, which takes in
    /// the agent's own filesystem as today's letters do
    /// (`crates/hamlet_server/src/work/tools.rs` `parcel`).
    Parcel,
}

/// The sandbox policy of a call, and the folder it works in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub policy: Policy,
    pub cwd: PathBuf,
}

/// The verdict on `op` of `agent` under `reach`, while the workspace
/// holds the secrets of version `held`.
pub fn gate(
    home: &RealmFolder,
    agent: AgentNumber,
    reach: Option<&Reach>,
    held: Option<SecretsVersion>,
    op: &Op<'_>,
) -> Gate {
    let Some((reach, secrets)) =
        reach.and_then(|reach| Some((reach, reach.secrets()?)))
    else {
        return Gate::Refuse(Refused::Reach(ReachRefusal::NoReach));
    };
    let needs = match needs(home, agent, reach, op) {
        Ok(needs) => needs,
        Err(refusal) => return Gate::Refuse(refusal),
    };
    if held < Some(secrets) {
        Gate::NeedsSecrets(secrets)
    } else if needs.is_empty() {
        Gate::Run {
            network: if matches!(
                op,
                Op::Command { .. } | Op::Tool { .. }
            ) && reach.network()
            {
                Network::On
            } else {
                Network::Off
            },
        }
    } else {
        Gate::NeedsChief(needs)
    }
}

/// The needs `op` waits for a chief to allow, in H11's order. `reach`
/// is live. A dream's errand waits for none.
fn needs(
    home: &RealmFolder,
    agent: AgentNumber,
    reach: &Reach,
    op: &Op<'_>,
) -> Result<Vec<Need>, Refused> {
    let footing = reach.footing();
    let project =
        footing.and_then(|footing| footing.project.as_ref());
    if let Some(project) = project
        && home.has_inside(&project.folder)
    {
        return Err(Refused::Reach(ReachRefusal::ProjectInRealm {
            folder: project.folder.clone(),
        }));
    }
    let file_tool = || {
        placement(home, agent, reach, op.purpose())
            .ok_or(Refused::Reach(ReachRefusal::NoReach))
    };
    match *op {
        Op::Command {
            conversation,
            script,
            network,
        } => Ok(footing.map_or_else(Vec::new, |footing| {
            program_needs(
                footing,
                conversation,
                network || is_networked(script),
            )
        })),
        Op::Tool {
            conversation,
            network,
        } => Ok(footing.map_or_else(Vec::new, |footing| {
            program_needs(footing, conversation, network)
        })),
        Op::Change { conversation, path } => {
            let path = normalized(path);
            if home.is_in_a_filesystem(&path) {
                return Err(Refused::Change(
                    PathRefusal::AgentFilesystem { path },
                ));
            }
            let policy = file_tool()?.policy;
            if !policy.can_write(&path) {
                return Err(Refused::Change(
                    PathRefusal::OutsideReach {
                        path,
                        access: ToChange::Change,
                    },
                ));
            }
            let in_manual_project = project.is_some_and(|project| {
                project.mode == Mode::Manual
                    && is_within(
                        &real_path(&path),
                        &real_path(&project.folder),
                    )
            });
            Ok(
                if in_manual_project
                    && !footing.is_some_and(|footing| {
                        trusts(footing, conversation, |trust| {
                            trust.edits
                        })
                    })
                {
                    vec![Need::Edit]
                } else {
                    Vec::new()
                },
            )
        }
        Op::Read { path } | Op::Parcel { path } => {
            let path = normalized(path);
            if file_tool()?.policy.can_read(&path) {
                Ok(Vec::new())
            } else if home.is_in_a_filesystem(&path) {
                Err(Refused::Read(PathRefusal::AgentFilesystem {
                    path,
                }))
            } else {
                Err(Refused::Read(PathRefusal::OutsideReach {
                    path,
                    access: ToRead::Read,
                }))
            }
        }
    }
}

/// The policy `agent`'s calls of `purpose` run under, or `None` once
/// it ended.
pub fn placement(
    home: &RealmFolder,
    agent: AgentNumber,
    reach: &Reach,
    purpose: Purpose,
) -> Option<Placement> {
    if let Reach::Dream { main, hire, .. } = reach {
        return Some(dream_placement(
            home,
            agent,
            main,
            hire.as_deref(),
            purpose,
        ));
    }
    let footing = reach.footing()?;
    let mut ro =
        vec![home.library(), home.tools(), home.spill(agent)];
    let own = match reach {
        Reach::Worker { starter, .. } => {
            ro.extend(
                starter.map(|starter| home.filesystem(starter)),
            );
            home.scratch(agent)
        }
        _ => {
            let own = home.filesystem(agent);
            if purpose == Purpose::Parcel {
                ro.push(own.clone());
            }
            own
        }
    };
    let mut rw = Vec::new();
    rw.extend(footing.project.as_ref().map(|p| p.folder.clone()));
    if matches!(reach, Reach::Worker { .. })
        || matches!(purpose, Purpose::Command { .. })
    {
        rw.push(own.clone());
    }
    let mut policy = match &footing.project {
        // The machine to read but the realm, and the chief's own
        // environment, so the project's tools are found.
        Some(_) => Policy {
            ro,
            rw,
            open: true,
            hidden: vec![home.root().to_owned()],
            env: host_env(),
            ..Policy::default()
        },
        None => {
            let mut env = base_env();
            env.insert("HOME".into(), text(&own));
            Policy {
                ro,
                rw,
                env,
                ..Policy::default()
            }
        }
    };
    policy.net = purpose
        == Purpose::Command {
            network: Network::On,
        };
    // Never written by an agent, whatever root lies in them (C18).
    policy.locked = vec![home.library(), home.tools()];
    let cwd = footing
        .project
        .as_ref()
        .map_or(own, |project| project.folder.clone());
    Some(Placement { policy, cwd })
}

/// A dream's errand's policy: its `main` to work in and its `hire`
/// side to write, and the machine to read but the rest of the realm.
fn dream_placement(
    home: &RealmFolder,
    agent: AgentNumber,
    main: &Path,
    hire: Option<&Path>,
    purpose: Purpose,
) -> Placement {
    let mut rw = vec![main.to_owned()];
    rw.extend(hire.map(Path::to_owned));
    Placement {
        policy: Policy {
            ro: vec![home.spill(agent)],
            rw,
            open: true,
            hidden: vec![home.root().to_owned()],
            env: host_env(),
            net: purpose
                == Purpose::Command {
                    network: Network::On,
                },
            ..Policy::default()
        },
        cwd: main.to_owned(),
    }
}

fn program_needs(
    footing: &Footing,
    conversation: &ConversationId,
    network: bool,
) -> Vec<Need> {
    let mut needs = Vec::new();
    if network && !footing.network {
        needs.push(Need::Network);
    }
    if is_manual(footing)
        && !trusts(footing, conversation, |trust| trust.commands)
    {
        needs.push(Need::Command);
    }
    needs
}

/// Whether the footing's project is in manual mode.
fn is_manual(footing: &Footing) -> bool {
    footing
        .project
        .as_ref()
        .is_some_and(|project| project.mode == Mode::Manual)
}

/// Whether the footing trusts `conversation` for what `allows`
/// picks.
fn trusts(
    footing: &Footing,
    conversation: &ConversationId,
    allows: impl Fn(&Trust) -> bool,
) -> bool {
    footing.trust.iter().any(|trust| {
        trust.conversation == *conversation && allows(trust)
    })
}

/// Whether `script` names a word that reaches the network: a word as
/// a whole word, other text anywhere.
fn is_networked(script: &str) -> bool {
    let lower = script.to_lowercase();
    let is_word =
        |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    NETWORK_WORDS.iter().any(|needle| {
        if needle.chars().all(is_word) {
            lower.split(|c| !is_word(c)).any(|part| part == *needle)
        } else {
            lower.contains(needle)
        }
    })
}

/// `path` as written, without `.` and `..` parts. The work checks the
/// real path again before it touches the file.
fn normalized(path: &Path) -> PathBuf {
    PathBuf::from(normalize(&text(path)))
}

#[cfg(test)]
mod tests;
