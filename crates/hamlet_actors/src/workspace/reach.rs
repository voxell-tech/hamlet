//! Each agent's reach on this machine (C18 to C21, H11): what its
//! tool calls may touch and when they wait for a chief. The realm is
//! the reach's only author. The workspace applies it, passes each
//! call through [`gate`], and runs it under the policy of
//! [`placement`].

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use hamlet_actor_core::{BindParam, ColumnValue, SqliteTransaction};
use hamlet_messages::workspace::reach::{
    Access, Footing, Mode, Need, Reach, Refusal, SecretsVersion,
    Trust,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, ReachVersion, SetReach,
};
use hamlet_os::fs::path::{normalize, text};
use hamlet_sandbox::{Policy, base_env, host_env, is_within};

use super::MACHINE;

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
    let agent = BindParam::Integer(i64::try_from(set.agent.0)?);
    let mut body = Vec::new();
    ciborium::into_writer(&set.reach, &mut body)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
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
            BindParam::Integer(i64::try_from(now.as_millis())?),
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

/// The realm folder, `HAMLET_HOME`, as an absolute normalized path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RealmFolder(PathBuf);

impl RealmFolder {
    pub fn new(path: &Path) -> io::Result<Self> {
        let absolute = std::path::absolute(path)?;
        Ok(Self(PathBuf::from(normalize(&text(&absolute)))))
    }

    pub fn root(&self) -> &Path {
        &self.0
    }

    /// `actors/agent/<n>.fs/`, the agent's filesystem. The only way
    /// the workspace names one.
    pub fn filesystem(&self, agent: AgentNumber) -> PathBuf {
        self.0.join("actors/agent").join(format!("{}.fs", agent.0))
    }

    /// `workers/<n>/`, a worker's scratch folder.
    pub fn scratch(&self, worker: AgentNumber) -> PathBuf {
        self.0.join("workers").join(worker.0.to_string())
    }

    /// The folder of `agent`'s spilled command output.
    pub fn spill(&self, agent: AgentNumber) -> PathBuf {
        self.0
            .join("actors/workspace")
            .join(format!("{MACHINE}.output"))
            .join(agent.0.to_string())
    }

    fn library(&self) -> PathBuf {
        self.0.join("library")
    }

    /// Tools a chief adds by hand (C17).
    fn tools(&self) -> PathBuf {
        self.0.join("tools")
    }

    /// Whether `path`, normalized, is in any agent's filesystem.
    fn is_in_a_filesystem(&self, path: &Path) -> bool {
        path.strip_prefix(self.0.join("actors/agent"))
            .ok()
            .and_then(|rest| rest.components().next())
            .is_some_and(|first| {
                first.as_os_str().to_string_lossy().ends_with(".fs")
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
    Change {
        conversation: &'a ConversationId,
        path: &'a Path,
    },
    Read {
        path: &'a Path,
    },
}

/// The gate's verdict on a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    Run {
        network: bool,
    },
    NeedsChief(Vec<Need>),
    Refuse(Refusal),
    /// The workspace's list of secrets is older than this version:
    /// fetch it from the realm, then pass the gate again.
    NeedsSecrets(SecretsVersion),
}

/// What a policy is for. An agent's commands write its filesystem;
/// its file tools never do, since the agent writes it itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Command { network: bool },
    FileTool,
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
    let Some((reach, footing)) =
        reach.and_then(|reach| Some((reach, reach.footing()?)))
    else {
        return Gate::Refuse(Refusal::NoReach);
    };
    let needs = match needs(home, agent, reach, footing, op) {
        Ok(needs) => needs,
        Err(refusal) => return Gate::Refuse(refusal),
    };
    if held < Some(footing.secrets) {
        Gate::NeedsSecrets(footing.secrets)
    } else if needs.is_empty() {
        Gate::Run {
            network: matches!(op, Op::Command { .. })
                && footing.network,
        }
    } else {
        Gate::NeedsChief(needs)
    }
}

/// What `op` waits for a chief to allow, in H11's order, or why it
/// may not run. `reach` is live, with `footing`.
fn needs(
    home: &RealmFolder,
    agent: AgentNumber,
    reach: &Reach,
    footing: &Footing,
    op: &Op<'_>,
) -> Result<Vec<Need>, Refusal> {
    let file_tool = || {
        placement(home, agent, reach, Purpose::FileTool)
            .ok_or(Refusal::NoReach)
    };
    match *op {
        Op::Command {
            conversation,
            script,
            network,
        } => {
            let mut needs = Vec::new();
            if (network || is_networked(script)) && !footing.network {
                needs.push(Need::Network);
            }
            if is_manual(footing)
                && !trusts(footing, conversation, |trust| {
                    trust.commands
                })
            {
                needs.push(Need::Command);
            }
            Ok(needs)
        }
        Op::Change { conversation, path } => {
            let path = normalized(path);
            if home.is_in_a_filesystem(&path) {
                return Err(Refusal::AgentFilesystem { path });
            }
            if !is_in_any(&path, &file_tool()?.policy.rw) {
                return Err(Refusal::OutsideReach {
                    path,
                    access: Access::Change,
                });
            }
            let in_manual_project =
                footing.project.as_ref().is_some_and(|project| {
                    project.mode == Mode::Manual
                        && is_within(&path, &project.folder)
                });
            Ok(
                if in_manual_project
                    && !trusts(footing, conversation, |trust| {
                        trust.edits
                    })
                {
                    vec![Need::Edit]
                } else {
                    Vec::new()
                },
            )
        }
        Op::Read { path } => {
            let path = normalized(path);
            if is_readable(&path, &file_tool()?.policy) {
                Ok(Vec::new())
            } else if home.is_in_a_filesystem(&path) {
                Err(Refusal::AgentFilesystem { path })
            } else {
                Err(Refusal::OutsideReach {
                    path,
                    access: Access::Read,
                })
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
        _ => home.filesystem(agent),
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
    policy.net = purpose == Purpose::Command { network: true };
    let cwd = footing
        .project
        .as_ref()
        .map_or(own, |project| project.folder.clone());
    Some(Placement { policy, cwd })
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

fn is_in_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| is_within(path, root))
}

fn is_readable(path: &Path, policy: &Policy) -> bool {
    is_in_any(path, &policy.ro)
        || is_in_any(path, &policy.rw)
        || (policy.open && !is_in_any(path, &policy.hidden))
}

#[cfg(test)]
mod tests {
    use hamlet_messages::workspace::reach::Project;

    use super::*;

    const AGENT: AgentNumber = AgentNumber(7);
    const STARTER: AgentNumber = AgentNumber(3);
    const OTHER: AgentNumber = AgentNumber(9);
    const V1: SecretsVersion = SecretsVersion(1);

    fn home() -> RealmFolder {
        RealmFolder::new(Path::new("/realm")).unwrap()
    }

    fn project(mode: Mode) -> Option<Project> {
        Some(Project {
            folder: "/work/project".into(),
            mode,
        })
    }

    fn footing(project: Option<Project>) -> Footing {
        Footing {
            project,
            network: false,
            trust: Vec::new(),
            secrets: V1,
        }
    }

    fn conversation(id: &str) -> ConversationId {
        ConversationId(id.into())
    }

    fn command<'a>(
        conversation: &'a ConversationId,
        script: &'a str,
    ) -> Op<'a> {
        Op::Command {
            conversation,
            script,
            network: false,
        }
    }

    fn change<'a>(
        conversation: &'a ConversationId,
        path: &'a Path,
    ) -> Op<'a> {
        Op::Change { conversation, path }
    }

    fn passed(reach: &Reach, op: &Op<'_>) -> Gate {
        gate(&home(), AGENT, Some(reach), Some(V1), op)
    }

    fn command_policy(reach: &Reach) -> Placement {
        placement(
            &home(),
            AGENT,
            reach,
            Purpose::Command { network: false },
        )
        .unwrap()
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    /// C18, C20, 4.2: an agent with a project folder writes it and
    /// its own filesystem, and reads the machine but the realm
    /// folder, with the chief's environment.
    #[test]
    fn an_agent_with_a_project_works_in_it() {
        let reach = Reach::Agent(footing(project(Mode::Auto)));
        let placed = command_policy(&reach);
        assert_eq!(
            placed.policy.rw,
            paths(&["/work/project", "/realm/actors/agent/7.fs"])
        );
        assert_eq!(
            placed.policy.ro,
            paths(&[
                "/realm/library",
                "/realm/tools",
                "/realm/actors/workspace/local.output/7",
            ])
        );
        assert!(placed.policy.open);
        assert_eq!(placed.policy.hidden, paths(&["/realm"]));
        assert_eq!(placed.policy.env, host_env());
        assert_eq!(placed.cwd, PathBuf::from("/work/project"));
    }

    /// C18, C16, 4.2: the Hand or a hire with no folder works in its
    /// own filesystem, which is its home, and reads nothing else of
    /// the machine.
    #[test]
    fn an_agent_with_no_project_works_in_its_filesystem() {
        let reach = Reach::Agent(footing(None));
        let placed = command_policy(&reach);
        assert_eq!(
            placed.policy.rw,
            paths(&["/realm/actors/agent/7.fs"])
        );
        assert!(!placed.policy.open);
        assert_eq!(
            placed.policy.env.get("HOME").map(String::as_str),
            Some("/realm/actors/agent/7.fs")
        );
        assert_eq!(
            placed.cwd,
            PathBuf::from("/realm/actors/agent/7.fs")
        );
    }

    /// H42, C18, 4.2: a worker writes its scratch folder and the
    /// project, and reads its starter's filesystem, never its own
    /// or another agent's.
    #[test]
    fn a_worker_reads_only_its_starters_filesystem() {
        let reach = Reach::Worker {
            starter: Some(STARTER),
            footing: footing(project(Mode::Auto)),
        };
        let placed = command_policy(&reach);
        assert_eq!(
            placed.policy.rw,
            paths(&["/work/project", "/realm/workers/7"])
        );
        assert!(
            placed
                .policy
                .ro
                .contains(&PathBuf::from("/realm/actors/agent/3.fs"))
        );
        let reads = |path: &str| {
            gate(
                &home(),
                AGENT,
                Some(&reach),
                Some(V1),
                &Op::Read {
                    path: Path::new(path),
                },
            )
        };
        assert_eq!(
            reads("/realm/actors/agent/3.fs/memory/notes.md"),
            Gate::Run { network: false }
        );
        for path in [
            "/realm/actors/agent/9.fs/memory/notes.md",
            "/realm/actors/agent/7.fs/memory/notes.md",
        ] {
            assert_eq!(
                reads(path),
                Gate::Refuse(Refusal::AgentFilesystem {
                    path: path.into()
                })
            );
        }
    }

    /// 4.2: a worker with no project works in its scratch folder,
    /// which is its home.
    #[test]
    fn a_worker_with_no_project_works_in_its_scratch_folder() {
        let reach = Reach::Worker {
            starter: None,
            footing: footing(None),
        };
        let placed = command_policy(&reach);
        assert_eq!(placed.policy.rw, paths(&["/realm/workers/7"]));
        assert!(!placed.policy.open);
        assert_eq!(placed.cwd, PathBuf::from("/realm/workers/7"));
        assert_eq!(
            placed.policy.env.get("HOME").map(String::as_str),
            Some("/realm/workers/7")
        );
    }

    /// A4: a file tool's policy does not write the agent's own
    /// filesystem, so a link into it is refused when the work checks
    /// the real path.
    #[test]
    fn a_file_tool_never_writes_an_agents_filesystem() {
        let reach = Reach::Agent(footing(project(Mode::Auto)));
        let placed =
            placement(&home(), AGENT, &reach, Purpose::FileTool)
                .unwrap();
        assert_eq!(placed.policy.rw, paths(&["/work/project"]));
        let talk = conversation("c");
        for agent in [AGENT, OTHER] {
            let path =
                home().filesystem(agent).join("memory/notes.md");
            assert_eq!(
                passed(&reach, &change(&talk, &path)),
                Gate::Refuse(Refusal::AgentFilesystem { path })
            );
        }
    }

    /// The "You see" box: a change outside the reach is refused with
    /// its path, before any chief is asked, also in manual mode.
    #[test]
    fn a_change_outside_the_reach_names_the_path() {
        let reach = Reach::Agent(footing(project(Mode::Manual)));
        let talk = conversation("c");
        for path in
            ["/etc/passwd", "/work/project/../elsewhere/a.txt"]
        {
            assert_eq!(
                passed(&reach, &change(&talk, Path::new(path))),
                Gate::Refuse(Refusal::OutsideReach {
                    path: normalize(path).into(),
                    access: Access::Change,
                })
            );
        }
    }

    /// H11, C19: a networked command of an agent with no network in
    /// manual mode asks once for both; the network is asked first.
    #[test]
    fn the_network_is_asked_before_manual_mode() {
        let talk = conversation("c");
        let manual = Reach::Agent(footing(project(Mode::Manual)));
        assert_eq!(
            passed(
                &manual,
                &command(&talk, "curl https://example.com")
            ),
            Gate::NeedsChief(vec![Need::Network, Need::Command])
        );
        let auto = Reach::Agent(footing(project(Mode::Auto)));
        assert_eq!(
            passed(
                &auto,
                &Op::Command {
                    conversation: &talk,
                    script: "make",
                    network: true,
                }
            ),
            Gate::NeedsChief(vec![Need::Network])
        );
        assert_eq!(
            passed(&auto, &command(&talk, "echo curly")),
            Gate::Run { network: false }
        );
        let mut networked = footing(project(Mode::Auto));
        networked.network = true;
        assert_eq!(
            passed(
                &Reach::Agent(networked),
                &command(&talk, "git pull")
            ),
            Gate::Run { network: true }
        );
    }

    /// H11, C21: trust for a conversation lets its commands and edits
    /// run in manual mode, and no other conversation's.
    #[test]
    fn trust_holds_for_its_conversation_only() {
        let mut trusted = footing(project(Mode::Manual));
        trusted.trust.push(Trust {
            conversation: conversation("a"),
            edits: true,
            commands: false,
        });
        let reach = Reach::Agent(trusted);
        let file = Path::new("/work/project/src/main.rs");
        let (a, b) = (conversation("a"), conversation("b"));
        assert_eq!(
            passed(&reach, &change(&a, file)),
            Gate::Run { network: false }
        );
        assert_eq!(
            passed(&reach, &change(&b, file)),
            Gate::NeedsChief(vec![Need::Edit])
        );
        assert_eq!(
            passed(&reach, &command(&a, "ls")),
            Gate::NeedsChief(vec![Need::Command])
        );
    }

    /// C21, H42: manual mode asks for edits of the folder only, never
    /// of a worker's scratch folder.
    #[test]
    fn a_workers_scratch_folder_is_never_manual() {
        let reach = Reach::Worker {
            starter: Some(STARTER),
            footing: footing(project(Mode::Manual)),
        };
        let talk = conversation("c");
        assert_eq!(
            passed(
                &reach,
                &change(&talk, Path::new("/realm/workers/7/a"))
            ),
            Gate::Run { network: false }
        );
        assert_eq!(
            passed(
                &reach,
                &change(&talk, Path::new("/work/project/a"))
            ),
            Gate::NeedsChief(vec![Need::Edit])
        );
    }

    #[test]
    fn an_ended_or_missing_reach_refuses_every_call() {
        let talk = conversation("c");
        let op = command(&talk, "ls");
        for reach in [None, Some(&Reach::Ended)] {
            assert_eq!(
                gate(&home(), AGENT, reach, Some(V1), &op),
                Gate::Refuse(Refusal::NoReach)
            );
        }
        assert_eq!(
            placement(
                &home(),
                AGENT,
                &Reach::Ended,
                Purpose::FileTool
            ),
            None
        );
    }

    /// W3: no call passes while the workspace's list of secrets is
    /// older than the reach's, or missing after a restart, even one
    /// that would wait for a chief. A refusal needs no list.
    #[test]
    fn a_call_waits_for_the_secrets_in_force() {
        let mut ahead = footing(project(Mode::Manual));
        ahead.secrets = SecretsVersion(2);
        let reach = Reach::Agent(ahead);
        let talk = conversation("c");
        for held in [None, Some(V1)] {
            assert_eq!(
                gate(
                    &home(),
                    AGENT,
                    Some(&reach),
                    held,
                    &command(&talk, "ls")
                ),
                Gate::NeedsSecrets(SecretsVersion(2))
            );
        }
        let outside = Path::new("/etc/passwd");
        assert!(matches!(
            gate(
                &home(),
                AGENT,
                Some(&reach),
                None,
                &change(&talk, outside)
            ),
            Gate::Refuse(Refusal::OutsideReach { .. })
        ));
        assert_eq!(
            gate(
                &home(),
                AGENT,
                Some(&reach),
                Some(SecretsVersion(2)),
                &command(&talk, "ls")
            ),
            Gate::NeedsChief(vec![Need::Command])
        );
    }
}
