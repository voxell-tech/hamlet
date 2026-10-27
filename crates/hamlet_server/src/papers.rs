//! Houses on the workspace chosen at startup, notes confined to them,
//! and the chief's prompt material.

pub(crate) mod copy;
pub(crate) mod files;
pub(crate) mod moving;
pub(crate) mod remote;
pub(crate) mod tree;

use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use hamlet_os::fs::{Directory, LastLink, Missing, Mode};
use hamlet_protocol::client::Confinement;
use hamlet_workspace_wire::MAX_FILE;

use self::files::Files;
use crate::protocol::{Cause, Refusable, Refusal};
use crate::record::AgentRef;
use crate::sandbox::{AllLeftovers, Isolation, Sandbox};
use crate::workspace::{Policy, Transport, WorkspaceSession};

pub(crate) const CHARTER: &str = include_str!("../assets/charter.md");

/// An agent's notes, at the top of its house.
pub(crate) const NOTES: &str = "NOTES.md";

/// A deed a workspace may refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capability {
    EraseHouse,
    FoundHamlet,
    MoveHamlet,
    RemoveHamlet,
    RestoreHamlet,
    EraseHamlet,
    /// Moving a house from one hamlet to another.
    MoveHouse,
}

impl Capability {
    fn operation(self) -> &'static str {
        match self {
            Self::EraseHouse => "erase an archived agent's house",
            Self::FoundHamlet => "found a hamlet",
            Self::MoveHamlet => "move a hamlet between workspaces",
            Self::RemoveHamlet => "remove a hamlet",
            Self::RestoreHamlet => "restore a hamlet",
            Self::EraseHamlet => "erase a hamlet",
            Self::MoveHouse => "move a house between hamlets",
        }
    }
}

/// Where houses live and tools run. The sandbox and the shells
/// belong to this machine, so a remote workspace holds neither.
enum Workspace {
    Local {
        sandbox: Sandbox,
        shells: crate::tools::Shells,
    },
    Remote(Transport),
}

/// The houses of the home's agents on the workspace chosen at
/// startup. The opened home directory anchors every local house.
pub(crate) struct Houses {
    root: Directory,
    workspace: Workspace,
    leftovers: AllLeftovers,
    skills: std::collections::BTreeMap<
        (String, u64),
        (std::time::Instant, Vec<String>),
    >,
    reading: std::collections::BTreeSet<(String, u64)>,
    readers: tokio::task::JoinSet<(String, u64, Vec<String>)>,
}

impl Houses {
    /// The houses under `root`, with tools run as `isolation` asks,
    /// or in the remote `workspace`.
    pub fn open(
        root: &Path,
        workspace: Option<crate::credentials::WorkspaceAddress>,
        isolation: Isolation,
    ) -> Result<Self> {
        Ok(Self {
            root: Directory::open(root, LastLink::Follow)?,
            workspace: match workspace {
                Some(connection) => {
                    Workspace::Remote(Transport::new(connection)?)
                }
                None => Workspace::Local {
                    sandbox: Sandbox::open(isolation)?,
                    shells: crate::tools::Shells::local(),
                },
            },
            leftovers: AllLeftovers::default(),
            skills: Default::default(),
            reading: Default::default(),
            readers: tokio::task::JoinSet::new(),
        })
    }

    pub(crate) fn root(&self) -> &Directory {
        &self.root
    }

    /// The remote workspace the houses are in.
    pub fn workspace(
        &self,
    ) -> Option<&crate::credentials::WorkspaceAddress> {
        match &self.workspace {
            Workspace::Local { .. } => None,
            Workspace::Remote(transport) => {
                Some(transport.connection())
            }
        }
    }

    /// Refuses `capability` when the workspace cannot do it.
    pub fn allows(&self, capability: Capability) -> Refusable<()> {
        match &self.workspace {
            Workspace::Local { .. } => Ok(()),
            Workspace::Remote(_) => Err(Refusal::Cannot {
                operation: capability.operation().into(),
                cause: Cause::Remote,
            }),
        }
    }

    /// Refuses a file of `size` bytes when the workspace cannot take
    /// it in one transfer.
    pub fn admits(&self, size: u64) -> Result<(), remote::TooLarge> {
        match self.workspace {
            Workspace::Remote(_) if size > MAX_FILE as u64 => {
                Err(remote::TooLarge)
            }
            _ => Ok(()),
        }
    }

    /// Whether every house change has happened or not once it
    /// returns, so work a crash cut short may be done again.
    pub fn can_confirm(&self) -> bool {
        matches!(self.workspace, Workspace::Local { .. })
    }

    /// Checks before serving that tools can run.
    pub fn probe(&self) -> Result<()> {
        match &self.workspace {
            Workspace::Local { sandbox, .. } => sandbox.probe(),
            Workspace::Remote(_) => Ok(()),
        }
    }

    /// How the workspace's tools are confined.
    pub fn confinement(&self) -> Confinement {
        match &self.workspace {
            Workspace::Local { sandbox, .. } => sandbox.confinement(),
            Workspace::Remote(_) => Confinement::Remote,
        }
    }

    /// The machine tools run on, where it is not this one.
    pub fn machine(&self) -> Option<&'static str> {
        match self.workspace {
            Workspace::Local { .. } => None,
            Workspace::Remote(_) => Some(remote::MACHINE),
        }
    }

    /// The only folder a hamlet may work on, where the workspace
    /// fixes it.
    pub fn fixed_folder(&self) -> Option<&'static str> {
        match self.workspace {
            Workspace::Local { .. } => None,
            Workspace::Remote(_) => Some(remote::PROJECT),
        }
    }

    fn session(
        transport: &Transport,
        hamlet: &str,
        number: u64,
        policy: Policy,
    ) -> Result<WorkspaceSession> {
        transport.session(
            AgentRef {
                hamlet: hamlet.into(),
                number,
            },
            policy,
        )
    }

    /// The house of agent `number` of `hamlet`.
    pub fn house(&self, hamlet: &str, number: u64) -> Result<Files> {
        Ok(match &self.workspace {
            Workspace::Local { .. } => Files::Local(tree::house(
                &self.root,
                hamlet,
                number,
                Missing::Fail,
            )?),
            Workspace::Remote(transport) => {
                Files::Remote(remote::Folder::house(Self::session(
                    transport,
                    hamlet,
                    number,
                    Policy::default(),
                )?))
            }
        })
    }

    /// A read-only view of agent `number`'s house and, with
    /// `project`, of its hamlet's folder.
    pub fn reader(
        &self,
        hamlet: &str,
        number: u64,
        project: bool,
    ) -> Result<Reader> {
        Ok(match &self.workspace {
            Workspace::Local { .. } => Reader::Local {
                root: self.root.try_clone()?,
                hamlet: hamlet.into(),
                number,
            },
            Workspace::Remote(transport) => {
                Reader::Remote(Self::session(
                    transport,
                    hamlet,
                    number,
                    Policy {
                        project,
                        network: false,
                        read_only: true,
                    },
                )?)
            }
        })
    }

    /// The shells the workspace offers.
    pub fn shells(&self) -> crate::tools::Shells {
        match &self.workspace {
            Workspace::Local { shells, .. } => shells.clone(),
            Workspace::Remote(_) => crate::tools::Shells::remote(),
        }
    }

    /// The tools of `agent` working at `place`, reaching the network
    /// only with `net`.
    pub fn tools(
        &self,
        agent: &AgentRef,
        place: crate::tools::Place,
        net: bool,
    ) -> Result<crate::tools::ToolSession> {
        Ok(match &self.workspace {
            Workspace::Local { sandbox, shells } => {
                crate::tools::ToolSession::local(
                    place,
                    net,
                    *sandbox,
                    shells.clone(),
                    self.leftovers.of(agent),
                )
            }
            Workspace::Remote(transport) => {
                let policy = Policy {
                    project: place.folder.is_some(),
                    network: net,
                    read_only: false,
                };
                let session =
                    transport.session(agent.clone(), policy)?;
                crate::tools::ToolSession::remote(
                    place,
                    session,
                    crate::tools::Shells::remote(),
                )
            }
        })
    }

    /// Makes the house of agent `number` of `hamlet` with its folders
    /// and notes, where they are missing.
    pub fn initialize(
        &self,
        hamlet: &str,
        number: u64,
    ) -> impl Future<Output = Result<()>> + Send + 'static {
        enum Making {
            Local(Directory, String),
            Remote(remote::Folder),
        }
        let making = match &self.workspace {
            Workspace::Local { .. } => self
                .root
                .try_clone()
                .map(|root| Making::Local(root, hamlet.into()))
                .map_err(Into::into),
            Workspace::Remote(transport) => Self::session(
                transport,
                hamlet,
                number,
                Policy::default(),
            )
            .map(|session| {
                Making::Remote(remote::Folder::house(session))
            }),
        };
        async move {
            match making? {
                Making::Local(root, hamlet) => {
                    tokio::task::spawn_blocking(move || {
                        tree::build(&root, &hamlet, number)
                    })
                    .await?
                }
                Making::Remote(house) => house.initialize().await,
            }
        }
    }

    /// Makes a local house now. A remote house is made by
    /// [`Houses::initialize`], which waits for the workspace.
    pub fn build(&self, hamlet: &str, number: u64) -> Result<()> {
        match self.workspace {
            Workspace::Local { .. } => {
                tree::build(&self.root, hamlet, number)
            }
            Workspace::Remote(_) => Ok(()),
        }
    }

    pub async fn read_notes(
        &self,
        hamlet: &str,
        number: u64,
    ) -> Result<String> {
        self.reader(hamlet, number, false)?.notes().await
    }

    pub async fn write_notes(
        &self,
        hamlet: &str,
        number: u64,
        text: &str,
    ) -> Result<()> {
        self.house(hamlet, number)?
            .replace(NOTES, text.as_bytes())
            .await
    }

    /// The folders of the skills in agent `number`'s house, as a
    /// client lists them. A remote house's are read apart and shown
    /// once read.
    pub fn skill_names(
        &mut self,
        hamlet: &str,
        number: u64,
    ) -> Result<Vec<String>> {
        if let Workspace::Local { .. } = self.workspace {
            return Ok(tree::house(
                &self.root,
                hamlet,
                number,
                Missing::Fail,
            )
            .map(|house| crate::prompt::skill_names(&house))
            .unwrap_or_default());
        }
        while let Some(Ok((hamlet, number, names))) =
            self.readers.try_join_next()
        {
            self.reading.remove(&(hamlet.clone(), number));
            self.skills.insert(
                (hamlet, number),
                (std::time::Instant::now(), names),
            );
        }
        let key = (hamlet.to_string(), number);
        let cached = self.skills.get(&key);
        if cached.is_none_or(|(at, _)| at.elapsed().as_secs() >= 1)
            && self.reading.insert(key.clone())
        {
            let reader = self.reader(hamlet, number, false)?;
            self.readers.spawn(async move {
                let names = reader
                    .skills()
                    .await
                    .into_iter()
                    .map(|skill| skill.folder)
                    .collect();
                (key.0, number, names)
            });
        }
        Ok(cached.map(|(_, names)| names.clone()).unwrap_or_default())
    }

    /// Moves the house of `from` to `to`, which must be free.
    pub fn move_house(
        &self,
        from: &AgentRef,
        to: &AgentRef,
    ) -> Result<()> {
        self.allows(Capability::MoveHouse)?;
        self.leftovers.end(|agent| agent == from);
        tree::move_house(&self.root, from, to)
    }

    pub fn erase_hamlet(&self, hamlet: &str) -> Result<()> {
        self.allows(Capability::EraseHamlet)?;
        ensure!(
            crate::record::check_name(hamlet).is_none(),
            "invalid hamlet name"
        );
        self.leftovers.end(|agent| agent.hamlet == hamlet);
        tree::erase_hamlet(&self.root, hamlet)
    }

    pub fn erase(&self, hamlet: &str, number: u64) -> Result<()> {
        self.allows(Capability::EraseHouse)?;
        self.leftovers.end(|agent| {
            agent.hamlet == hamlet && agent.number == number
        });
        tree::erase_house(&self.root, hamlet, number)
    }

    /// Ends what every agent's commands left running.
    pub fn end_leftovers(&self) {
        self.leftovers.end(|_| true);
    }

    /// Makes agent `number`'s house hold `snapshot`. A local house is
    /// staged beside the houses and swapped in whole.
    pub async fn replace(
        &self,
        hamlet: &str,
        number: u64,
        snapshot: &copy::Snapshot,
    ) -> Result<()> {
        let Workspace::Local { .. } = self.workspace else {
            self.initialize(hamlet, number).await?;
            return snapshot
                .replace(&self.house(hamlet, number)?)
                .await;
        };
        let stages = tree::directory(
            &self.root,
            ".gifts",
            Missing::Make(Mode::FOLDER),
        )?;
        let name = format!("move-{}", uuid::Uuid::new_v4());
        let stage = tree::directory(
            &stages,
            &name,
            Missing::Make(Mode::FOLDER),
        )?;
        let mapped = snapshot
            .entries
            .iter()
            .filter(|entry| entry.path != ".")
            .map(|entry| (entry, entry.path.clone()))
            .collect::<Vec<_>>();
        let mut result = snapshot
            .write(&Files::Local(stage.try_clone()?), &mapped)
            .await;
        if result.is_ok() {
            let root = self.root.try_clone()?;
            let (hamlet, name) = (hamlet.to_owned(), name.clone());
            let stages = stages.try_clone()?;
            let mode = snapshot.entries[0].mode;
            result = tokio::task::spawn_blocking(move || {
                stage.set_mode(Mode::new(mode))?;
                tree::swap(&root, &stages, &name, &hamlet, number)
            })
            .await?;
        }
        if result.is_err() {
            let _ = tree::erase(&stages, &name);
        }
        result
    }
}

/// A read-only view of one agent's house and its hamlet's folder.
pub(crate) enum Reader {
    Local {
        root: Directory,
        hamlet: String,
        number: u64,
    },
    Remote(WorkspaceSession),
}

impl Reader {
    /// Where tools see the house at `local`.
    pub fn house(&self, local: &Path) -> PathBuf {
        match self {
            Self::Local { .. } => local.to_owned(),
            Self::Remote(_) => PathBuf::from(remote::HOUSE),
        }
    }

    /// Where tools see the library at `local`.
    pub fn library(&self, local: &Path) -> PathBuf {
        match self {
            Self::Local { .. } => local.to_owned(),
            Self::Remote(_) => PathBuf::from(remote::LIBRARY),
        }
    }

    /// Where tools see the hamlet's `folder`.
    pub fn folder<'a>(&self, folder: &'a str) -> &'a str {
        match self {
            Self::Local { .. } => folder,
            Self::Remote(_) => remote::PROJECT,
        }
    }

    pub async fn notes(&self) -> Result<String> {
        match self {
            Self::Local {
                root,
                hamlet,
                number,
            } => {
                let (root, hamlet, number) =
                    (root.try_clone()?, hamlet.clone(), *number);
                tokio::task::spawn_blocking(move || {
                    let mut text = String::new();
                    tree::notes(&root, &hamlet, number)?
                        .read_to_string(&mut text)?;
                    Ok(text)
                })
                .await?
            }
            Self::Remote(session) => remote::notes(session).await,
        }
    }

    pub async fn skills(&self) -> Vec<crate::prompt::shelf::Skill> {
        match self {
            Self::Local {
                root,
                hamlet,
                number,
            } => {
                let Ok(root) = root.try_clone() else {
                    return Vec::new();
                };
                let (hamlet, number) = (hamlet.clone(), *number);
                tokio::task::spawn_blocking(move || {
                    tree::house(&root, &hamlet, number, Missing::Fail)
                        .map(|house| {
                            crate::prompt::shelf::house_skills(&house)
                        })
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default()
            }
            Self::Remote(session) => remote::skills(session).await,
        }
    }

    /// The file `name` of the hamlet's `folder`, when it can be read.
    pub async fn project(
        &self,
        folder: &str,
        name: &str,
    ) -> Option<Vec<u8>> {
        match self {
            Self::Local { .. } => {
                let path = Path::new(folder).join(name);
                tokio::task::spawn_blocking(move || {
                    std::fs::read(path).ok()
                })
                .await
                .ok()
                .flatten()
            }
            Self::Remote(session) => {
                remote::project(session, name).await
            }
        }
    }

    /// Whether the hamlet's `folder` has `name`.
    pub async fn has_project_file(
        &self,
        folder: &str,
        name: &str,
    ) -> bool {
        match self {
            Self::Local { .. } => {
                let path = Path::new(folder).join(name);
                tokio::task::spawn_blocking(move || path.exists())
                    .await
                    .unwrap_or(false)
            }
            Self::Remote(session) => {
                remote::has_project_file(session, name).await
            }
        }
    }
}

pub(crate) fn cut(text: &str, most: usize, note: &str) -> String {
    let mut chars = text.chars();
    let mut result = chars.by_ref().take(most).collect::<String>();
    if chars.next().is_some() {
        result.push_str(note);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use hamlet_os::process::{Command, Limits, Tree};
    use hamlet_testing::Scratch;
    use hamlet_testing::signals::is_alive;
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, BufReader};

    use super::*;
    use crate::tools::Place;

    fn agent(number: u64) -> AgentRef {
        AgentRef {
            hamlet: "home".into(),
            number,
        }
    }

    /// The pid of a `sleep` that `agent`'s command left in `cwd`.
    #[cfg(unix)]
    async fn leftover(
        houses: &Houses,
        agent: &AgentRef,
        cwd: &Path,
    ) -> u32 {
        let argv = [
            "bash".to_owned(),
            "-c".into(),
            "sleep 300 & echo $!".into(),
        ];
        let env = BTreeMap::from([(
            "PATH".to_owned(),
            std::env::var("PATH").unwrap_or_default(),
        )]);
        kept(houses, agent, cwd, &argv, &env).await
    }

    /// The pid of a `ping` that waits as `sleep` does, which
    /// `agent`'s command left in `cwd`. Git Bash's `$!` is not a
    /// Windows pid.
    #[cfg(windows)]
    async fn leftover(
        houses: &Houses,
        agent: &AgentRef,
        cwd: &Path,
    ) -> u32 {
        let argv = [
            format!(
                r"{}\System32\WindowsPowerShell\v1.0\powershell.exe",
                std::env::var("SystemRoot").unwrap()
            ),
            "-NoProfile".into(),
            "-Command".into(),
            "(Start-Process ping -ArgumentList \
             '-n','300','127.0.0.1' -NoNewWindow -PassThru).Id"
                .into(),
        ];
        let env = std::env::vars().collect();
        kept(houses, agent, cwd, &argv, &env).await
    }

    /// The pid `argv` prints of what it leaves running in `cwd`, kept
    /// as `agent`'s leftovers.
    async fn kept(
        houses: &Houses,
        agent: &AgentRef,
        cwd: &Path,
        argv: &[String],
        env: &BTreeMap<String, String>,
    ) -> u32 {
        let (mut tree, stdout, _) = Tree::spawn(
            Command {
                argv,
                env,
                cwd: &cwd.to_string_lossy(),
                stdin: std::process::Stdio::null(),
            },
            &Limits::default(),
        )
        .unwrap();
        let mut lines = BufReader::new(stdout).lines();
        // The first number, past what `ping` prints.
        let pid = loop {
            let line = lines.next_line().await.unwrap().unwrap();
            if let Ok(pid) = line.trim().parse() {
                break pid;
            }
        };
        tree.ended().await.unwrap();
        houses
            .leftovers
            .of(agent)
            .keep(tree.release().expect("the sleep runs on"));
        pid
    }

    async fn is_gone(pid: u32) -> bool {
        for _ in 0..500 {
            if !is_alive(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[tokio::test]
    async fn erase_ends_the_agent_s_leftovers_first() {
        let home = Scratch::new("erase-leftovers");
        let houses =
            Houses::open(&home, None, Isolation::TESTING).unwrap();
        houses.initialize("home", 1).await.unwrap();
        let house = home.join("houses/home/1");
        let erased = leftover(&houses, &agent(1), &house).await;
        let kept = leftover(&houses, &agent(2), &home).await;
        houses.erase("home", 1).unwrap();
        assert!(!house.exists());
        assert!(is_gone(erased).await);
        assert!(is_alive(kept));
        houses.end_leftovers();
    }

    #[tokio::test]
    async fn move_house_ends_the_agent_s_leftovers_first() {
        let home = Scratch::new("move-leftovers");
        let houses =
            Houses::open(&home, None, Isolation::TESTING).unwrap();
        houses.initialize("home", 1).await.unwrap();
        let house = home.join("houses/home/1");
        houses.initialize("away", 0).await.unwrap();
        let moved = leftover(&houses, &agent(1), &house).await;
        let to = AgentRef {
            hamlet: "away".into(),
            number: 1,
        };
        houses.move_house(&agent(1), &to).unwrap();
        assert!(home.join("houses/away/1").exists());
        assert!(is_gone(moved).await);
    }

    #[tokio::test]
    async fn stopping_the_server_ends_every_agent_s_leftovers() {
        let home = Scratch::new("stop-leftovers");
        let houses =
            Houses::open(&home, None, Isolation::TESTING).unwrap();
        let first = leftover(&houses, &agent(1), &home).await;
        let house = home.join("houses/home/2");
        std::fs::create_dir_all(&house).unwrap();
        let place = Place {
            house: house.clone(),
            folder: None,
            home: home.to_path_buf(),
            shared: Vec::new(),
        };
        let tools = houses.tools(&agent(2), place, false).unwrap();
        let command = "(while :; do echo . >> beat; sleep 0.05; \
                       done) > /dev/null 2>&1 &";
        let arguments = json!({"command": command});
        let arguments = arguments.as_object().cloned().unwrap();
        let ran = tools.run(&[], "bash", &arguments).await;
        assert!(!ran.error, "{}", ran.text);
        let beats = || {
            std::fs::metadata(house.join("beat"))
                .map_or(0, |m| m.len())
        };
        let pause = || tokio::time::sleep(Duration::from_millis(300));
        // Under Seatbelt and in insecure mode the loop outlives its
        // call. Under bubblewrap it ends with it.
        #[cfg(not(target_os = "linux"))]
        {
            let before = beats();
            pause().await;
            assert!(beats() > before);
        }
        houses.end_leftovers();
        assert!(is_gone(first).await);
        pause().await;
        let after = beats();
        pause().await;
        assert_eq!(beats(), after);
    }
}
