use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Write};

use anyhow::{Context, Result, bail, ensure};
use hamlet_os::fs::{
    Directory, Exists, Kind, Missing, Mode, Name, NotRegular, Open,
    Regular, Removing, Replace, Stat, TreePath,
};
use hamlet_workspace_wire::{Code, TreeEntry, TreeKind};
use sha2::{Digest, Sha256};

use super::NOTES;
use crate::diagnostic::{Secrets, tell};
use crate::record::AgentRef;
use crate::workspace::Failure;

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct Entry {
    pub bytes: Vec<u8>,
    pub mode: u32,
}

pub(crate) fn house(
    root: &Directory,
    hamlet: &str,
    number: u64,
    missing: Missing,
) -> io::Result<Directory> {
    directory(
        &hamlet_folder(root, hamlet, missing)?,
        &number.to_string(),
        missing,
    )
}

fn hamlet_folder(
    root: &Directory,
    hamlet: &str,
    missing: Missing,
) -> io::Result<Directory> {
    if hamlet.is_empty()
        || !hamlet.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid house address",
        ));
    }
    directory(&directory(root, "houses", missing)?, hamlet, missing)
}

pub(crate) fn build(
    root: &Directory,
    hamlet: &str,
    number: u64,
) -> Result<()> {
    let dir =
        house(root, hamlet, number, Missing::Make(Mode::FOLDER))?;
    for part in ["skills", "inbox"] {
        directory(&dir, part, Missing::Make(Mode::FOLDER))?;
    }
    match dir.file(&Name::new(NOTES)?, Open::New(Mode::PRIVATE)) {
        Ok(notes) => notes.file().write_all(b"# Notes\n")?,
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(crate) fn notes(
    root: &Directory,
    hamlet: &str,
    number: u64,
) -> Result<File> {
    let dir = house(root, hamlet, number, Missing::Fail)?;
    match dir.file(&Name::new(NOTES)?, Open::Read) {
        Ok(notes) => Ok(notes.into_file()),
        Err(error) if NotRegular::of(&error).is_some() => bail!(
            "The agent's NOTES.md could not be read safely. Inspect \
             its house before trying again."
        ),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn move_house(
    root: &Directory,
    from: &AgentRef,
    to: &AgentRef,
) -> Result<()> {
    match house(root, &from.hamlet, from.number, Missing::Fail) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            house(root, &to.hamlet, to.number, Missing::Fail)?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }
    house(root, &to.hamlet, 0, Missing::Fail)?;
    hamlet_folder(root, &from.hamlet, Missing::Fail)?.rename(
        &Name::new(&from.number.to_string())?,
        &hamlet_folder(root, &to.hamlet, Missing::Fail)?,
        &Name::new(&to.number.to_string())?,
        Replace::Never,
    )?;
    Ok(())
}

pub(crate) fn erase_house(
    root: &Directory,
    hamlet: &str,
    number: u64,
) -> Result<()> {
    match hamlet_folder(root, hamlet, Missing::Fail) {
        Ok(houses) => {
            remove_tree(&houses, &Name::new(&number.to_string())?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn erase_hamlet(
    root: &Directory,
    hamlet: &str,
) -> Result<()> {
    erase(&directory(root, "houses", Missing::Fail)?, hamlet)
}

/// Puts the stage `name` in place of agent `number`'s house in
/// `hamlet`, erasing the house it replaces.
pub(crate) fn swap(
    root: &Directory,
    stages: &Directory,
    name: &str,
    hamlet: &str,
    number: u64,
) -> Result<()> {
    let parent =
        hamlet_folder(root, hamlet, Missing::Make(Mode::FOLDER))?;
    let name = Name::in_tree(name)?;
    let number = Name::new(&number.to_string())?;
    let old = format!("old-{}", uuid::Uuid::new_v4());
    let backup = Name::new(&old)?;
    let detached = match parent.rename(
        &number,
        stages,
        &backup,
        Replace::Always,
    ) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            false
        }
        Err(error) => return Err(error.into()),
    };
    if let Err(error) =
        stages.rename(&name, &parent, &number, Replace::Always)
    {
        let error = anyhow::Error::from(error);
        if detached
            && let Err(rollback) = stages.rename(
                &backup,
                &parent,
                &number,
                Replace::Always,
            )
        {
            return Err(error.context(format!(
                "The replaced house could not be put back, and is \
                 kept beside the stage as {old}: {rollback}"
            )));
        }
        return Err(error);
    }
    if detached {
        remove_any(stages, &backup)?;
    }
    Ok(())
}

pub(crate) fn directory(
    parent: &Directory,
    name: &str,
    missing: Missing,
) -> io::Result<Directory> {
    let name = Name::in_tree(name)?;
    match missing {
        Missing::Fail => parent.child(&name),
        Missing::Make(mode) => parent.make(&name, mode, Exists::Open),
    }
}

pub(crate) fn read(
    root: &Directory,
    cap: u64,
    entries: usize,
) -> Result<BTreeMap<String, Entry>> {
    let mut files = BTreeMap::new();
    let mut left = cap;
    let mut remaining = entries;
    scan(root, "", &mut files, &mut left, &mut remaining, false)?;
    Ok(files)
}

/// The library's files, without its hidden ones, within `cap` bytes
/// and `count` entries.
pub(crate) fn library(
    root: &Directory,
    cap: u64,
    count: usize,
) -> Result<BTreeMap<String, Entry>> {
    let mut files = BTreeMap::new();
    scan(root, "", &mut files, &mut { cap }, &mut { count }, true)?;
    Ok(files)
}

/// What `path` names, a link itself rather than what it names, or
/// `None` where nothing is.
pub(crate) fn kind(
    root: &Directory,
    path: &TreePath,
) -> Result<Option<TreeKind>> {
    let (dir, name) = root.parent(path, Missing::Fail)?;
    Ok(dir.stat(name)?.map(|stat| match stat.kind {
        Kind::Directory => TreeKind::Directory,
        Kind::File => TreeKind::File,
        Kind::Link => TreeKind::Link,
        Kind::Special => TreeKind::Special,
    }))
}

/// Makes the folder `path`, which must be new.
pub(crate) fn mkdir(root: &Directory, path: &TreePath) -> Result<()> {
    let (dir, name) = root.parent(path, Missing::Fail)?;
    dir.make(name, Mode::FOLDER, Exists::Fail)?;
    Ok(())
}

pub(crate) fn set_mode(
    root: &Directory,
    path: &TreePath,
    mode: u32,
) -> Result<()> {
    let mode = Mode::new(mode);
    let (dir, name) = root.parent(path, Missing::Fail)?;
    match present(&dir, name)?.kind {
        Kind::Directory => dir.child(name)?.set_mode(mode)?,
        Kind::File => dir.file(name, Open::Read)?.set_mode(mode)?,
        Kind::Link | Kind::Special => {
            anyhow::bail!("Links and special files cannot be copied.")
        }
    }
    Ok(())
}

/// Every entry of the tree, `.` first, with each regular file's
/// SHA-256, within `cap` bytes and `count` entries. A file with
/// other names is special.
pub(crate) fn entries(
    root: &Directory,
    cap: u64,
    count: usize,
) -> Result<Vec<TreeEntry>> {
    let mut entries = vec![TreeEntry {
        path: ".".into(),
        kind: TreeKind::Directory,
        size: 0,
        mode: root.status()?.mode.bits(),
        sha256: None,
    }];
    listed(root, "", &mut entries, &mut { cap }, count)?;
    Ok(entries)
}

/// The entries under `dir`, each folder's listed while it alone is
/// open, so a wide tree holds few descriptors.
fn listed(
    dir: &Directory,
    prefix: &str,
    entries: &mut Vec<TreeEntry>,
    left: &mut u64,
    count: usize,
) -> Result<()> {
    for name in dir.listing()? {
        let name = name?;
        if entries.len() >= count {
            return Err(Failure::refused(
                Code::TooMany,
                "The tree has too many entries.",
            )
            .into());
        }
        let path = format!("{prefix}{}", text(&name)?);
        let stat = present(dir, &name)?;
        let kind = match stat.kind {
            Kind::Directory => TreeKind::Directory,
            Kind::File if stat.links == 1 => TreeKind::File,
            Kind::Link => TreeKind::Link,
            Kind::File | Kind::Special => TreeKind::Special,
        };
        let (size, sha256) = if kind == TreeKind::File {
            let bytes = read_lone(dir, &name, *left)?;
            *left -= bytes.len() as u64;
            (
                bytes.len() as u64,
                Some(format!("{:x}", Sha256::digest(&bytes))),
            )
        } else {
            (0, None)
        };
        entries.push(TreeEntry {
            path: path.clone(),
            kind,
            size,
            mode: stat.mode.bits(),
            sha256,
        });
        if kind == TreeKind::Directory {
            listed(
                &dir.child(&name)?,
                &format!("{path}/"),
                entries,
                left,
                count,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn read_file(
    root: &Directory,
    path: &TreePath,
    cap: u64,
) -> Result<Vec<u8>> {
    let (dir, name) = root.parent(path, Missing::Fail)?;
    read_lone(&dir, name, cap)
}

fn read_lone(
    dir: &Directory,
    name: &Name,
    cap: u64,
) -> Result<Vec<u8>> {
    let file = lone(dir, name, "Not a regular unlinked file.")?;
    let mut bytes = Vec::new();
    file.into_file()
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(Failure::refused(
            Code::TooLarge,
            "The file is too large.",
        )
        .into());
    }
    Ok(bytes)
}

fn lone(
    dir: &Directory,
    name: &Name,
    refusal: &'static str,
) -> Result<Regular> {
    match dir.file(name, Open::Read) {
        Ok(file) if file.is_lone() => Ok(file),
        Err(error) if NotRegular::of(&error).is_none() => {
            Err(error.into())
        }
        _ => bail!(refusal),
    }
}

/// The folder of society snapshots under the houses' root.
const SNAPSHOTS: &str = ".society-files";

/// The files of the snapshot `name`, read as [`read`] reads a tree.
pub(crate) fn read_snapshot(
    root: &Directory,
    name: &str,
    cap: u64,
    count: usize,
) -> Result<BTreeMap<String, Entry>> {
    read(
        &directory(
            &directory(root, SNAPSHOTS, Missing::Fail)?,
            name,
            Missing::Fail,
        )?,
        cap,
        count,
    )
}

/// Erases the snapshot `name`.
pub(crate) fn erase_snapshot(
    root: &Directory,
    name: &str,
) -> Result<()> {
    erase(&directory(root, SNAPSHOTS, Missing::Fail)?, name)
}

pub(crate) fn sweep_snapshots(
    root: &Directory,
    kept: &[String],
) -> Result<()> {
    let snapshots =
        directory(root, SNAPSHOTS, Missing::Make(Mode::FOLDER))?;
    for name in snapshots.names()? {
        let text = text(&name)?;
        if !kept.iter().any(|kept| kept == text) {
            remove_any(&snapshots, &name)?;
        }
    }
    Ok(())
}

pub(crate) fn snapshot(
    root: &Directory,
    files: &BTreeMap<String, Entry>,
) -> Result<String> {
    let snapshots =
        directory(root, SNAPSHOTS, Missing::Make(Mode::FOLDER))?;
    let name = uuid::Uuid::new_v4().to_string();
    let snapshot =
        directory(&snapshots, &name, Missing::Make(Mode::FOLDER))?;
    for (path, file) in files {
        put(&snapshot, &TreePath::parse(path)?, file)?;
    }
    snapshot.sync()?;
    snapshots.sync()?;
    root.sync()?;
    crate::fault::point("after_society_snapshot");
    Ok(name)
}

fn scan(
    dir: &Directory,
    prefix: &str,
    files: &mut BTreeMap<String, Entry>,
    left: &mut u64,
    remaining: &mut usize,
    hidden: bool,
) -> Result<()> {
    for name in dir.listing()? {
        let name = name?;
        let text = text(&name)?;
        if hidden && text.starts_with('.') {
            continue;
        }
        ensure!(
            *remaining > 0,
            "The house has too many files; remove the agent instead."
        );
        *remaining -= 1;
        let path = format!("{prefix}{text}");
        match present(dir, &name)?.kind {
            Kind::Directory => scan(
                &dir.child(&name)?,
                &format!("{path}/"),
                files,
                left,
                remaining,
                false,
            )?,
            Kind::File => {
                let file = lone(
                    dir,
                    &name,
                    "Links and special files cannot be merged.",
                )?;
                let mode = file.stat().mode.bits();
                let mut bytes = Vec::new();
                file.into_file()
                    .take(left.saturating_add(1))
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() as u64 <= *left,
                    "The house is too large; remove the agent \
                     instead."
                );
                *left -= bytes.len() as u64;
                files.insert(path, Entry { bytes, mode });
            }
            Kind::Link | Kind::Special => anyhow::bail!(
                "Links and special files cannot be merged."
            ),
        }
    }
    Ok(())
}

pub(crate) fn put(
    root: &Directory,
    path: &TreePath,
    entry: &Entry,
) -> Result<()> {
    let (dir, name) =
        root.parent(path, Missing::Make(Mode::FOLDER))?;
    let temporary =
        Name::new(&format!(".incoming-{}", uuid::Uuid::new_v4()))?;
    let file = dir.file(&temporary, Open::New(Mode::PRIVATE))?;
    file.file().write_all(&entry.bytes)?;
    file.set_mode(Mode::new(entry.mode))?;
    file.file().sync_all()?;
    let renamed = dir.rename(&temporary, &dir, name, Replace::Never);
    if renamed.is_err() {
        dir.remove(&temporary, Removing::File)?;
    }
    match renamed {
        Ok(()) => dir.sync()?,
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists =>
        {
            let present = lone(
                &dir,
                name,
                "The merge target is not a regular file.",
            )?;
            let mut bytes = Vec::new();
            present
                .into_file()
                .take(entry.bytes.len() as u64 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes == entry.bytes,
                "The merge target changed; inspect the houses \
                 before retrying."
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(crate) fn remove(
    root: &Directory,
    path: &TreePath,
) -> Result<()> {
    let (dir, name) = root.parent(path, Missing::Fail)?;
    dir.remove(name, Removing::File)?;
    Ok(())
}

pub(crate) fn replace(
    root: &Directory,
    path: &TreePath,
    bytes: &[u8],
) -> Result<()> {
    let (dir, name) = root.parent(path, Missing::Fail)?;
    if let Some(stat) = dir.stat(name)? {
        ensure!(
            stat.kind == Kind::File,
            "The file to replace is not a regular file."
        );
    }
    let temporary =
        Name::new(&format!(".incoming-{}", uuid::Uuid::new_v4()))?;
    let file = dir.file(&temporary, Open::New(Mode::PRIVATE))?;
    file.file().write_all(bytes)?;
    file.file().sync_all()?;
    dir.rename(&temporary, &dir, name, Replace::Always)?;
    dir.sync()?;
    Ok(())
}

pub(crate) fn publish(
    staging: &Directory,
    target: &Directory,
    name: &str,
    files: &BTreeMap<String, Entry>,
) -> Result<()> {
    let stage =
        directory(staging, name, Missing::Make(Mode::FOLDER))?;
    for (path, entry) in files {
        put(&stage, &TreePath::parse(path)?, entry)?;
    }
    stage.sync()?;
    let skill = Name::in_tree(name)?;
    match staging.rename(&skill, target, &skill, Replace::Never) {
        Ok(()) => {
            staging.sync()?;
            target.sync()?;
        }
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists =>
        {
            let cap = files
                .values()
                .map(|entry| entry.bytes.len() as u64)
                .sum();
            ensure!(
                read(
                    &directory(target, name, Missing::Fail)?,
                    cap,
                    4096
                )? == *files,
                "The published skill changed; inspect the library \
                 before retrying."
            );
            remove_tree(staging, &skill)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// Erases the file or folder `path`.
pub(crate) fn erase_path(
    root: &Directory,
    path: &TreePath,
) -> Result<()> {
    let (dir, name) = root.parent(path, Missing::Fail)?;
    remove_any(&dir, name)
}

pub(crate) fn erase(root: &Directory, name: &str) -> Result<()> {
    remove_tree(root, &Name::in_tree(name)?)
}

fn remove_tree(root: &Directory, name: &Name) -> Result<()> {
    let dir = match root.child(name) {
        Ok(dir) => dir,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    clear(&dir)?;
    drop(dir);
    root.remove(name, Removing::Directory)?;
    Ok(())
}

fn clear(dir: &Directory) -> Result<()> {
    // Only the owner can change the mode. Another folder is emptied
    // as it is.
    let _ = dir.set_mode(Mode::FOLDER);
    for name in dir.names()? {
        remove_any(dir, &name)?;
    }
    Ok(())
}

fn remove_any(dir: &Directory, name: &Name) -> Result<()> {
    if present(dir, name)?.kind == Kind::Directory {
        remove_tree(dir, name)
    } else {
        Ok(dir.remove(name, Removing::File)?)
    }
}

fn present(dir: &Directory, name: &Name) -> io::Result<Stat> {
    dir.stat(name)?
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
}

fn text(name: &Name) -> Result<&str> {
    name.to_str().context("A name in the tree is not UTF-8.")
}

pub(crate) fn sweep(root: &Directory) -> Result<()> {
    clear(&directory(root, ".gifts", Missing::Make(Mode::FOLDER))?)
}

pub(crate) struct Stage {
    parent: Directory,
    name: Name,
}

impl Stage {
    pub fn new(root: &Directory) -> Result<Self> {
        let parent =
            directory(root, ".gifts", Missing::Make(Mode::FOLDER))?;
        let name =
            Name::new(&format!("gift-{}", uuid::Uuid::new_v4()))?;
        parent.make(&name, Mode::FOLDER, Exists::Fail)?;
        Ok(Self { parent, name })
    }

    pub fn dir(&self) -> io::Result<Directory> {
        self.parent.child(&self.name)
    }

    pub fn file(&self, path: &str, missing: Missing) -> Result<File> {
        ensure!(
            path.len() < 4096,
            "gift path exceeds the file system limit"
        );
        let path = TreePath::parse(path)?;
        let how = match missing {
            Missing::Make(_) => Open::New(Mode::SHARED),
            Missing::Fail => Open::Append,
        };
        let (dir, name) = self.dir()?.parent(&path, missing)?;
        Ok(dir.file(name, how)?.into_file())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if remove_tree(&self.parent, &self.name).is_err() {
            tell(Secrets::Fixed, "could not discard a gift.");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use hamlet_os::fs::LastLink;
    use hamlet_testing::Scratch;
    use hamlet_testing::links::hard_link;

    use super::*;

    fn path(text: &str) -> TreePath {
        TreePath::parse(text).unwrap()
    }

    #[test]
    fn put_refuses_a_merge_target_that_is_no_regular_file() {
        let home = Scratch::new("tree-merge-target");
        fs::create_dir(home.join("folder")).unwrap();
        let mut targets = vec!["folder"];
        #[cfg(unix)]
        {
            hamlet_testing::fifo(&home.join("pipe"));
            targets.push("pipe");
        }
        let root = Directory::open(&home, LastLink::Follow).unwrap();
        let entry = Entry {
            bytes: b"merged".to_vec(),
            mode: 0o600,
        };
        for target in targets {
            let refused =
                put(&root, &path(target), &entry).unwrap_err();
            assert_eq!(
                refused.to_string(),
                "The merge target is not a regular file.",
                "{target}"
            );
        }
    }

    #[test]
    fn read_file_refuses_what_is_no_lone_regular_file() {
        let home = Scratch::new("tree-read-lone");
        fs::create_dir(home.join("folder")).unwrap();
        fs::write(home.join("file"), "shared").unwrap();
        hard_link(home.join("file"), home.join("second")).unwrap();
        let mut names = vec!["folder", "second"];
        #[cfg(unix)]
        {
            hamlet_testing::fifo(&home.join("pipe"));
            names.push("pipe");
        }
        let root = Directory::open(&home, LastLink::Follow).unwrap();
        for name in names {
            let refused =
                read_file(&root, &path(name), 64).unwrap_err();
            assert_eq!(
                refused.to_string(),
                "Not a regular unlinked file.",
                "{name}"
            );
        }
    }

    #[test]
    fn notes_are_unsafe_only_where_they_are_no_regular_file() {
        let home = Scratch::new("tree-notes");
        let root = Directory::open(&home, LastLink::Follow).unwrap();
        build(&root, "north", 1).unwrap();
        let file = home.join("houses/north/1/NOTES.md");
        fs::remove_file(&file).unwrap();
        let missing = notes(&root, "north", 1).unwrap_err();
        assert_eq!(
            missing.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::NotFound)
        );
        assert!(!missing.to_string().contains("read safely"));
        fs::create_dir(&file).unwrap();
        let folder = notes(&root, "north", 1).unwrap_err();
        assert!(folder.to_string().contains("read safely"));
    }

    #[cfg(unix)]
    #[test]
    fn erase_fails_on_a_folder_the_server_cannot_open() {
        use std::os::unix::fs::PermissionsExt;
        let home = Scratch::new("tree-unopened");
        let locked = home.join("stage/locked");
        fs::create_dir_all(&locked).unwrap();
        fs::set_permissions(
            &locked,
            fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let root = Directory::open(&home, LastLink::Follow).unwrap();
        let refused = erase(&root, "stage").unwrap_err();
        fs::set_permissions(
            &locked,
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert_eq!(
            refused.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::PermissionDenied)
        );
    }

    #[cfg(unix)]
    #[test]
    fn clear_empties_a_read_only_folder() {
        use std::os::unix::fs::PermissionsExt;
        let home = Scratch::new("tree-clear");
        let locked = home.join("stage/locked");
        fs::create_dir_all(&locked).unwrap();
        fs::write(locked.join("kept"), "kept").unwrap();
        fs::set_permissions(
            &locked,
            fs::Permissions::from_mode(0o500),
        )
        .unwrap();
        let root = Directory::open(&home, LastLink::Follow).unwrap();
        erase(&root, "stage").unwrap();
        assert!(!home.join("stage").exists());
    }

    #[test]
    fn move_house_reaches_the_parent_by_name() {
        let home = Scratch::new("tree-move");
        let root = Directory::open(&home, LastLink::Follow).unwrap();
        build(&root, "north", 1).unwrap();
        build(&root, "south", 0).unwrap();
        let from = AgentRef {
            hamlet: "north".into(),
            number: 1,
        };
        let to = AgentRef {
            hamlet: "south".into(),
            number: 2,
        };
        move_house(&root, &from, &to).unwrap();
        assert!(home.join("houses/south/2/NOTES.md").is_file());
        assert!(!home.join("houses/north/1").exists());
        move_house(&root, &from, &to).unwrap();
        build(&root, "north", 1).unwrap();
        assert!(move_house(&root, &from, &to).is_err());
        assert!(home.join("houses/north/1/NOTES.md").is_file());
    }

    #[test]
    fn move_house_needs_house_0_in_the_target_hamlet() {
        let home = Scratch::new("tree-move-mayor");
        let root = Directory::open(&home, LastLink::Follow).unwrap();
        build(&root, "north", 1).unwrap();
        build(&root, "south", 3).unwrap();
        let from = AgentRef {
            hamlet: "north".into(),
            number: 1,
        };
        let to = AgentRef {
            hamlet: "south".into(),
            number: 2,
        };
        let refused = move_house(&root, &from, &to).unwrap_err();
        assert_eq!(
            refused.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::NotFound)
        );
        assert!(home.join("houses/north/1/NOTES.md").is_file());
        assert!(!home.join("houses/south/2").exists());
    }
}
