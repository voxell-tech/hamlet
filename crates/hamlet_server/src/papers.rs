//! Notes confined to houses, and the chief's prompt material.

pub(crate) mod copy;
pub(crate) mod files;
pub(crate) mod moving;
pub(crate) mod tree;

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Result, ensure};

pub(crate) const CHARTER: &str = include_str!("../assets/charter.md");

/// An agent's notes, at the top of its house.
pub(crate) const NOTES: &str = "NOTES.md";

/// The opened home directory anchors every notes read and write.
pub(crate) struct Houses {
    root: File,
    pub workspace: Option<crate::credentials::Workspace>,
    skills: std::collections::BTreeMap<
        (String, u64),
        (std::time::Instant, Vec<String>),
    >,
    reading: std::collections::BTreeSet<(String, u64)>,
    readers: tokio::task::JoinSet<(String, u64, Vec<String>)>,
}

impl Houses {
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            root: File::open(root)?,
            workspace: None,
            skills: Default::default(),
            reading: Default::default(),
            readers: tokio::task::JoinSet::new(),
        })
    }

    pub(crate) fn root(&self) -> &File {
        &self.root
    }

    #[cfg(unix)]
    pub(crate) fn house(
        &self,
        hamlet: &str,
        number: u64,
        create: bool,
    ) -> Result<File> {
        use rustix::fs::{Mode, OFlags, mkdirat, openat};
        ensure!(
            self.workspace.is_none(),
            "{}",
            crate::workspace::unavailable("open a local house")
        );
        ensure!(
            !hamlet.is_empty()
                && hamlet.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'-'),
            "invalid house address"
        );
        let mut dir = self.root.try_clone()?;
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

    pub fn build(&self, hamlet: &str, number: u64) -> Result<()> {
        #[cfg(unix)]
        {
            use rustix::fs::{Mode, OFlags, mkdirat, openat};
            let dir = self.house(hamlet, number, true)?;
            for part in ["skills", "inbox"] {
                if let Err(error) =
                    mkdirat(&dir, part, Mode::from_raw_mode(0o700))
                    && error != rustix::io::Errno::EXIST
                {
                    return Err(error.into());
                }
                openat(
                    &dir,
                    part,
                    OFlags::RDONLY
                        | OFlags::DIRECTORY
                        | OFlags::NOFOLLOW,
                    Mode::empty(),
                )?;
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
                Err(error) if error == rustix::io::Errno::EXIST => {}
                Err(error) => return Err(error.into()),
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = (hamlet, number);
            anyhow::bail!(
                "House files require a supported sandbox platform."
            )
        }
    }

    fn notes(
        &self,
        hamlet: &str,
        number: u64,
        write: bool,
    ) -> Result<File> {
        #[cfg(unix)]
        {
            use rustix::fs::{Mode, OFlags, openat};
            let dir = self.house(hamlet, number, false)?;
            let flags = if write {
                OFlags::WRONLY | OFlags::CREATE
            } else {
                OFlags::RDONLY
            };
            let file = File::from(openat(
                &dir,
                NOTES,
                flags
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            ensure!(
                file.metadata()?.is_file(),
                "The agent's NOTES.md could not be read safely. \
                 Inspect its house before trying again."
            );
            Ok(file)
        }
        #[cfg(not(unix))]
        {
            let _ = (hamlet, number, write);
            anyhow::bail!(
                "House files require a supported sandbox platform."
            )
        }
    }

    pub fn read(&self, hamlet: &str, number: u64) -> Result<String> {
        let mut text = String::new();
        self.notes(hamlet, number, false)?
            .read_to_string(&mut text)?;
        Ok(text)
    }

    pub fn remote(
        &self,
        hamlet: &str,
        number: u64,
        read_only: bool,
    ) -> Result<Option<crate::workspace::Remote>> {
        self.workspace
            .as_ref()
            .map(|connection| {
                crate::workspace::Remote::new(
                    connection.clone(),
                    crate::record::AgentRef {
                        hamlet: hamlet.into(),
                        number,
                    },
                    false,
                    false,
                    read_only,
                )
            })
            .transpose()
    }

    pub fn files(
        &self,
        hamlet: &str,
        number: u64,
    ) -> Result<files::Files> {
        match self.remote(hamlet, number, false)? {
            Some(env) => Ok(files::Files::Remote {
                env,
                path: "/house".into(),
            }),
            None => self
                .house(hamlet, number, false)
                .map(files::Files::Local),
        }
    }

    pub fn skill_names(
        &mut self,
        hamlet: &str,
        number: u64,
        local: &Path,
    ) -> Result<Vec<String>> {
        let Some(remote) = self.remote(hamlet, number, true)? else {
            return Ok(crate::prompt::skill_names(local));
        };
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
            self.readers.spawn(async move {
                let names =
                    crate::prompt::shelf::remote_skills(&remote)
                        .await
                        .into_iter()
                        .map(|skill| skill.folder)
                        .collect();
                (key.0, number, names)
            });
        }
        Ok(cached.map(|(_, names)| names.clone()).unwrap_or_default())
    }

    pub async fn initialize(
        &self,
        hamlet: &str,
        number: u64,
    ) -> Result<()> {
        match self.remote(hamlet, number, false)? {
            Some(remote) => {
                remote.initialize().await.map_err(Into::into)
            }
            None => self.build(hamlet, number),
        }
    }

    pub async fn read_notes(
        &self,
        hamlet: &str,
        number: u64,
    ) -> Result<String> {
        match self.remote(hamlet, number, true)? {
            Some(remote) => {
                let bytes = remote
                    .read("/house/NOTES.md", true)
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!("{}", error.message)
                    })?;
                String::from_utf8(bytes).map_err(Into::into)
            }
            None => self.read(hamlet, number),
        }
    }

    pub async fn write_notes(
        &self,
        hamlet: &str,
        number: u64,
        text: &str,
    ) -> Result<()> {
        match self.remote(hamlet, number, false)? {
            Some(remote) => {
                let files = files::Files::Remote {
                    env: remote,
                    path: "/house".into(),
                };
                files.replace(NOTES, text.as_bytes()).await
            }
            None => self.write(hamlet, number, text),
        }
    }

    pub fn move_house(
        &self,
        from: &crate::record::AgentRef,
        to: &crate::record::AgentRef,
    ) -> Result<()> {
        #[cfg(unix)]
        {
            use rustix::fs::{
                Mode, OFlags, RenameFlags, openat, renameat_with,
            };
            let source =
                match self.house(&from.hamlet, from.number, false) {
                    Ok(source) => source,
                    Err(error)
                        if error
                            .downcast_ref::<rustix::io::Errno>()
                            == Some(&rustix::io::Errno::NOENT) =>
                    {
                        self.house(&to.hamlet, to.number, false)?;
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
            let flags =
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
            let parent = openat(&source, "..", flags, Mode::empty())?;
            let mayor = self.house(&to.hamlet, 0, false)?;
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
        #[cfg(not(unix))]
        {
            let _ = (from, to);
            anyhow::bail!(
                "House files require a supported sandbox platform."
            )
        }
    }

    pub fn erase_hamlet(&self, hamlet: &str) -> Result<()> {
        #[cfg(unix)]
        {
            use rustix::fs::{
                AtFlags, Mode, OFlags, openat, unlinkat,
            };
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
            let dir =
                match openat(&parent, hamlet, flags, Mode::empty()) {
                    Ok(dir) => File::from(dir),
                    Err(error)
                        if error == rustix::io::Errno::NOENT =>
                    {
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                };
            clear(&dir)?;
            unlinkat(&parent, hamlet, AtFlags::REMOVEDIR)?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = hamlet;
            anyhow::bail!(
                "House files require a supported sandbox platform."
            )
        }
    }

    pub fn erase(&self, hamlet: &str, number: u64) -> Result<()> {
        #[cfg(unix)]
        {
            use rustix::fs::{
                AtFlags, Mode, OFlags, openat, unlinkat,
            };
            let house = match self.house(hamlet, number, false) {
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
        #[cfg(not(unix))]
        {
            let _ = (hamlet, number);
            anyhow::bail!(
                "House files require a supported sandbox platform."
            )
        }
    }

    pub fn write(
        &self,
        hamlet: &str,
        number: u64,
        text: &str,
    ) -> Result<()> {
        let mut file = self.notes(hamlet, number, true)?;
        file.set_len(0)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }
}

#[cfg(unix)]
fn clear(dir: &File) -> Result<()> {
    use rustix::fs::{
        AtFlags, Dir, FileType, Mode, OFlags, openat, statat,
        unlinkat,
    };
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
