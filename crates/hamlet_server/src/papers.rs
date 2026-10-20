//! Houses on the backend chosen at startup, notes confined to them,
//! and the chief's prompt material.

pub(crate) mod copy;
pub(crate) mod files;
pub(crate) mod moving;
pub(crate) mod remote;
pub(crate) mod tree;

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use hamlet_workspace_wire::MAX_FILE;
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, RenameFlags, mkdirat,
    openat, renameat_with, statat, unlinkat,
};

use self::files::Files;
use crate::record::AgentRef;
use crate::workspace::{Policy, Session, Transport};

pub(crate) const CHARTER: &str = include_str!("../assets/charter.md");

/// An agent's notes, at the top of its house.
pub(crate) const NOTES: &str = "NOTES.md";

/// What a backend may refuse to do.
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

/// Where houses live and tools run.
enum Backend {
    Local,
    Remote(Transport),
}

/// The houses of the home's agents on the backend chosen at startup.
/// The opened home directory anchors every local house.
pub(crate) struct Houses {
    root: File,
    backend: Backend,
    skills: std::collections::BTreeMap<
        (String, u64),
        (std::time::Instant, Vec<String>),
    >,
    reading: std::collections::BTreeSet<(String, u64)>,
    readers: tokio::task::JoinSet<(String, u64, Vec<String>)>,
}

impl Houses {
    /// The houses under `root`, or in the remote `workspace`.
    pub fn open(
        root: &Path,
        workspace: Option<crate::credentials::Workspace>,
    ) -> Result<Self> {
        Ok(Self {
            root: File::open(root)?,
            backend: match workspace {
                Some(connection) => {
                    Backend::Remote(Transport::new(connection)?)
                }
                None => Backend::Local,
            },
            skills: Default::default(),
            reading: Default::default(),
            readers: tokio::task::JoinSet::new(),
        })
    }

    pub(crate) fn root(&self) -> &File {
        &self.root
    }

    /// The remote workspace the houses are in.
    pub fn workspace(
        &self,
    ) -> Option<&crate::credentials::Workspace> {
        match &self.backend {
            Backend::Local => None,
            Backend::Remote(transport) => {
                Some(transport.connection())
            }
        }
    }

    /// Words for the chief when the backend cannot do `capability`.
    pub fn allows(
        &self,
        capability: Capability,
    ) -> Result<(), String> {
        match &self.backend {
            Backend::Local => Ok(()),
            Backend::Remote(_) => Err(crate::workspace::unavailable(
                capability.operation(),
            )),
        }
    }

    /// Words for the chief when the backend cannot take a file of
    /// `size` bytes in one transfer.
    pub fn admits(&self, size: u64) -> Result<(), &'static str> {
        match self.backend {
            Backend::Remote(_) if size > MAX_FILE as u64 => {
                Err(remote::TOO_LARGE)
            }
            _ => Ok(()),
        }
    }

    /// Whether every house change has happened or not once it
    /// returns, so work a crash cut short may be done again.
    pub fn confirms(&self) -> bool {
        matches!(self.backend, Backend::Local)
    }

    /// Checks before serving that tools can run.
    pub fn probe(
        &self,
        sandbox: crate::sandbox::Sandbox,
    ) -> Result<()> {
        match self.backend {
            Backend::Local => sandbox.probe(),
            Backend::Remote(_) => Ok(()),
        }
    }

    /// The machine tools run on, where it is not this one.
    pub fn machine(&self) -> Option<&'static str> {
        match self.backend {
            Backend::Local => None,
            Backend::Remote(_) => Some(remote::MACHINE),
        }
    }

    /// The only folder a hamlet may work on, where the backend fixes
    /// it.
    pub fn fixed_folder(&self) -> Option<&'static str> {
        match self.backend {
            Backend::Local => None,
            Backend::Remote(_) => Some(remote::PROJECT),
        }
    }

    fn session(
        transport: &Transport,
        hamlet: &str,
        number: u64,
        policy: Policy,
    ) -> Result<Session> {
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
        Ok(match &self.backend {
            Backend::Local => Files::Local(house(
                &self.root, hamlet, number, false,
            )?),
            Backend::Remote(transport) => {
                Files::Remote(remote::Dir::house(Self::session(
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
        Ok(match &self.backend {
            Backend::Local => Reader::Local {
                root: self.root.try_clone()?,
                hamlet: hamlet.into(),
                number,
            },
            Backend::Remote(transport) => {
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

    /// The tools of `agent` working at `place`, reaching the network
    /// only with `net`.
    pub fn tools(
        &self,
        agent: &AgentRef,
        place: crate::tools::Place,
        net: bool,
        sandbox: crate::sandbox::Sandbox,
    ) -> Result<crate::tools::Session> {
        Ok(match &self.backend {
            Backend::Local => {
                crate::tools::Session::local(place, net, sandbox)
            }
            Backend::Remote(transport) => {
                let policy = Policy {
                    project: place.folder.is_some(),
                    network: net,
                    read_only: false,
                };
                let session =
                    transport.session(agent.clone(), policy)?;
                crate::tools::Session::remote(place, session)
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
            Local(File, String),
            Remote(remote::Dir),
        }
        let making = match &self.backend {
            Backend::Local => self
                .root
                .try_clone()
                .map(|root| Making::Local(root, hamlet.into()))
                .map_err(Into::into),
            Backend::Remote(transport) => Self::session(
                transport,
                hamlet,
                number,
                Policy::default(),
            )
            .map(|session| {
                Making::Remote(remote::Dir::house(session))
            }),
        };
        async move {
            match making? {
                Making::Local(root, hamlet) => {
                    tokio::task::spawn_blocking(move || {
                        build(&root, &hamlet, number)
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
        match self.backend {
            Backend::Local => build(&self.root, hamlet, number),
            Backend::Remote(_) => Ok(()),
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

    /// The folders of the skills in the house at `local`, as a client
    /// lists them. A remote house's are read apart and shown once
    /// read.
    pub fn skill_names(
        &mut self,
        hamlet: &str,
        number: u64,
        local: &Path,
    ) -> Result<Vec<String>> {
        if let Backend::Local = self.backend {
            return Ok(crate::prompt::skill_names(local));
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
            let local = local.to_owned();
            self.readers.spawn(async move {
                let names = reader
                    .skills(&local)
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
        self.allows(Capability::MoveHouse)
            .map_err(anyhow::Error::msg)?;
        let source =
            match house(&self.root, &from.hamlet, from.number, false)
            {
                Ok(source) => source,
                Err(error)
                    if error.downcast_ref::<rustix::io::Errno>()
                        == Some(&rustix::io::Errno::NOENT) =>
                {
                    house(&self.root, &to.hamlet, to.number, false)?;
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
        let flags =
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
        let parent = openat(&source, "..", flags, Mode::empty())?;
        let mayor = house(&self.root, &to.hamlet, 0, false)?;
        let target = openat(&mayor, "..", flags, Mode::empty())?;
        renameat_with(
            parent,
            from.number.to_string(),
            target,
            to.number.to_string(),
            RenameFlags::NOREPLACE,
        )?;
        Ok(())
    }

    pub fn erase_hamlet(&self, hamlet: &str) -> Result<()> {
        self.allows(Capability::EraseHamlet)
            .map_err(anyhow::Error::msg)?;
        ensure!(
            crate::record::check_name(hamlet).is_none(),
            "invalid hamlet name"
        );
        let flags = OFlags::RDONLY
            | OFlags::DIRECTORY
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC;
        let parent = File::from(openat(
            &self.root,
            "houses",
            flags,
            Mode::empty(),
        )?);
        let dir = match openat(&parent, hamlet, flags, Mode::empty())
        {
            Ok(dir) => File::from(dir),
            Err(rustix::io::Errno::NOENT) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        clear(&dir)?;
        unlinkat(&parent, hamlet, AtFlags::REMOVEDIR)?;
        Ok(())
    }

    pub fn erase(&self, hamlet: &str, number: u64) -> Result<()> {
        self.allows(Capability::EraseHouse)
            .map_err(anyhow::Error::msg)?;
        let house = match house(&self.root, hamlet, number, false) {
            Ok(house) => house,
            Err(error)
                if error.downcast_ref::<rustix::io::Errno>()
                    == Some(&rustix::io::Errno::NOENT) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        clear(&house)?;
        let parent = openat(
            &house,
            "..",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        unlinkat(parent, number.to_string(), AtFlags::REMOVEDIR)?;
        Ok(())
    }

    /// Makes agent `number`'s house hold `snapshot`. A local house is
    /// staged beside the houses and swapped in whole.
    pub async fn replace(
        &self,
        hamlet: &str,
        number: u64,
        snapshot: &copy::Snapshot,
    ) -> Result<()> {
        let Backend::Local = self.backend else {
            self.initialize(hamlet, number).await?;
            return snapshot
                .replace(&self.house(hamlet, number)?)
                .await;
        };
        let stages = tree::directory(&self.root, ".gifts", true)?;
        let name = format!("move-{}", uuid::Uuid::new_v4());
        let stage = tree::directory(&stages, &name, true)?;
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
                stage.set_permissions(
                    std::os::unix::fs::PermissionsExt::from_mode(
                        mode & 0o777,
                    ),
                )?;
                swap(&root, &stages, &name, &hamlet, number)
            })
            .await?;
        }
        if result.is_err() {
            let _ = tree::erase(&stages, &name);
        }
        result
    }
}

/// Puts the stage `name` in place of agent `number`'s house in
/// `hamlet`, erasing the house it replaces.
fn swap(
    root: &File,
    stages: &File,
    name: &str,
    hamlet: &str,
    number: u64,
) -> Result<()> {
    use rustix::fs::renameat;
    let houses = tree::directory(root, "houses", true)?;
    let parent = tree::directory(&houses, hamlet, true)?;
    let number = number.to_string();
    let backup = format!("old-{}", uuid::Uuid::new_v4());
    let detached = match renameat(
        &parent,
        number.as_str(),
        stages,
        backup.as_str(),
    ) {
        Ok(()) => true,
        Err(rustix::io::Errno::NOENT) => false,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) =
        renameat(stages, name, &parent, number.as_str())
    {
        if detached {
            renameat(
                stages,
                backup.as_str(),
                &parent,
                number.as_str(),
            )?;
        }
        return Err(error.into());
    }
    if detached {
        match tree::directory(stages, &backup, false) {
            Ok(_) => tree::erase(stages, &backup)?,
            Err(_) => {
                unlinkat(stages, backup.as_str(), AtFlags::empty())?
            }
        }
    }
    Ok(())
}

/// A read-only view of one agent's house and its hamlet's folder.
pub(crate) enum Reader {
    Local {
        root: File,
        hamlet: String,
        number: u64,
    },
    Remote(Session),
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
                    notes(&root, &hamlet, number)?
                        .read_to_string(&mut text)?;
                    Ok(text)
                })
                .await?
            }
            Self::Remote(session) => remote::notes(session).await,
        }
    }

    /// The house's skills; a local house's are those at `local`.
    pub async fn skills(
        &self,
        local: &Path,
    ) -> Vec<crate::prompt::shelf::Skill> {
        match self {
            Self::Local { .. } => {
                let shelf = local.join("skills");
                tokio::task::spawn_blocking(move || {
                    crate::prompt::shelf::shelf(&shelf)
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
    pub async fn project_has(
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
                remote::project_has(session, name).await
            }
        }
    }
}

/// The local house of agent `number` of `hamlet`, made with its
/// parents when `create` is set.
fn house(
    root: &File,
    hamlet: &str,
    number: u64,
    create: bool,
) -> Result<File> {
    ensure!(
        !hamlet.is_empty()
            && hamlet.bytes().all(|b| b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'-'),
        "invalid house address"
    );
    let mut dir = root.try_clone()?;
    for part in ["houses", hamlet, &number.to_string()] {
        if create
            && let Err(error) =
                mkdirat(&dir, part, Mode::from_raw_mode(0o700))
            && error != rustix::io::Errno::EXIST
        {
            return Err(error.into());
        }
        dir = openat(
            &dir,
            part,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::CLOEXEC,
            Mode::empty(),
        )?
        .into();
    }
    Ok(dir)
}

fn build(root: &File, hamlet: &str, number: u64) -> Result<()> {
    let dir = house(root, hamlet, number, true)?;
    for part in ["skills", "inbox"] {
        tree::directory(&dir, part, true)?;
    }
    match openat(
        &dir,
        NOTES,
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    ) {
        Ok(fd) => File::from(fd).write_all(b"# Notes\n")?,
        Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn notes(root: &File, hamlet: &str, number: u64) -> Result<File> {
    let dir = house(root, hamlet, number, false)?;
    let file = File::from(openat(
        &dir,
        NOTES,
        OFlags::RDONLY
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    ensure!(
        file.metadata()?.is_file(),
        "The agent's NOTES.md could not be read safely. Inspect its \
         house before trying again."
    );
    Ok(file)
}

pub(crate) fn clear(dir: &File) -> Result<()> {
    for entry in Dir::read_from(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let info = statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(info.st_mode)
            == FileType::Directory
        {
            let child = File::from(openat(
                dir,
                name,
                OFlags::RDONLY
                    | OFlags::DIRECTORY
                    | OFlags::NOFOLLOW
                    | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            clear(&child)?;
            unlinkat(dir, name, AtFlags::REMOVEDIR)?;
        } else {
            unlinkat(dir, name, AtFlags::empty())?;
        }
    }
    Ok(())
}

pub(crate) fn cut(text: &str, most: usize, note: &str) -> String {
    let mut chars = text.chars();
    let mut result = chars.by_ref().take(most).collect::<String>();
    if chars.next().is_some() {
        result.push_str(note);
    }
    result
}
