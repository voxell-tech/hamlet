use std::io::{self, Read as _, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    Directory, Exists, Kind, LastLink, Missing, Mode, Name, Open,
    Removing, Replace, Stat, TreePath, path,
};

/// What a write's new file is named until it is renamed into place.
const INCOMING: &str = ".incoming-";

/// A folder held open, whose files are reached by paths below it, one
/// name at a time from the folder and never through a link. Moving or
/// swapping one of its folders while a call runs changes nothing that
/// call reaches but that folder itself.
#[derive(Debug)]
pub struct Folder {
    root: Directory,
    path: String,
}

/// The refusal of a path that leads out of a [`Folder`].
#[derive(Debug, thiserror::Error)]
#[error("outside the folder")]
pub struct Outside;

impl Outside {
    /// Whether `error` is this refusal.
    pub fn is(error: &io::Error) -> bool {
        matches!(error.get_ref(), Some(inner) if inner.is::<Self>())
    }
}

impl From<Outside> for io::Error {
    fn from(refusal: Outside) -> Self {
        Self::new(io::ErrorKind::PermissionDenied, refusal)
    }
}

/// The refusal to read a file that has another name, which may be
/// outside the folder.
#[derive(Debug, thiserror::Error)]
#[error("the file has more than one name")]
pub struct NotLone;

impl From<NotLone> for io::Error {
    fn from(refusal: NotLone) -> Self {
        Self::new(io::ErrorKind::PermissionDenied, refusal)
    }
}

/// One name in a folder, with its path from the [`Folder`] and its
/// own status.
#[derive(Debug)]
pub struct Listed {
    pub name: String,
    pub path: String,
    pub stat: Stat,
}

/// A regular file below a [`Folder`], by its path from the folder.
#[derive(Debug, PartialEq, Eq)]
pub struct Found {
    pub path: String,
    pub size: u64,
}

/// The files [`Folder::files`] found, and whether it stopped short.
#[derive(Debug, Default)]
pub struct Walked {
    /// The path from the folder the walk started at.
    pub start: String,
    pub files: Vec<Found>,
    pub capped: bool,
}

impl Folder {
    /// The folder at `path`, made with its parents where missing.
    pub fn make(path: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(path)?;
        Self::open(path)
    }

    /// The folder at `path`, which must not be a link itself.
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            root: Directory::open(path, LastLink::Refuse)?,
            path: path::resolve(".", &path::text(path)),
        })
    }

    /// The folder's absolute path, as the paths below it start.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// `given` as a path from this folder, empty for the folder
    /// itself. A relative path starts at the folder. `.` and `..` are
    /// taken as text, so only a path that stays below the folder
    /// passes, and the walk never goes up.
    fn relative(&self, given: &str) -> io::Result<String> {
        if given.contains('\0') {
            return Err(super::BadName.into());
        }
        let resolved = path::resolve(&self.path, given);
        if resolved == self.path {
            return Ok(String::new());
        }
        let rest = if self.path.ends_with('/') {
            resolved.strip_prefix(&self.path)
        } else {
            resolved
                .strip_prefix(&self.path)
                .and_then(|rest| rest.strip_prefix('/'))
        };
        Ok(rest.ok_or(Outside)?.to_owned())
    }

    /// `given` as a path below this folder, or `None` for the folder
    /// itself.
    fn below(&self, given: &str) -> io::Result<Option<TreePath>> {
        let relative = self.relative(given)?;
        if relative.is_empty() {
            return Ok(None);
        }
        Ok(Some(TreePath::parse(&relative)?))
    }

    /// The directory holding the last name of `given`, and that name.
    fn parent(
        &self,
        given: &str,
        missing: Missing,
    ) -> io::Result<(Directory, Name)> {
        let tree = self
            .below(given)?
            .ok_or(io::Error::from(io::ErrorKind::IsADirectory))?;
        let (dir, name) = self.root.parent(&tree, missing)?;
        Ok((dir, name.clone()))
    }

    /// The folder at `given`, following no link.
    fn directory(&self, given: &str) -> io::Result<Directory> {
        if self.below(given)?.is_none() {
            return self.root.try_clone();
        }
        let (dir, name) = self.parent(given, Missing::Fail)?;
        dir.child(&name)
    }

    /// The own status of what is at `given`, a link itself.
    pub fn stat(&self, given: &str) -> io::Result<Stat> {
        if self.below(given)?.is_none() {
            return self.root.status();
        }
        let (dir, name) = self.parent(given, Missing::Fail)?;
        dir.stat(&name)?
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    /// The bytes of the regular file at `given`. A file with another
    /// name is refused with [`NotLone`].
    pub fn read(&self, given: &str) -> io::Result<Vec<u8>> {
        let (dir, name) = self.parent(given, Missing::Fail)?;
        let file = dir.file(&name, Open::Read)?;
        if !file.is_lone() {
            return Err(NotLone.into());
        }
        let mut bytes = Vec::new();
        file.into_file().read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Writes `bytes` whole at `given`, making its folders. The bytes
    /// go to a new file renamed over the name, so a link there, or a
    /// file with another name, is replaced and never written through.
    pub fn write(&self, given: &str, bytes: &[u8]) -> io::Result<()> {
        let (dir, name) =
            self.parent(given, Missing::Make(Mode::FOLDER))?;
        if dir
            .stat(&name)?
            .is_some_and(|stat| stat.kind == Kind::Directory)
        {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        let (incoming, mut file) = incoming(&dir)?;
        let written = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| {
                dir.rename(&incoming, &dir, &name, Replace::Always)
            });
        if written.is_err() {
            let _ = dir.remove(&incoming, Removing::File);
        }
        written
    }

    /// Makes the folder at `given` with the folders on its way, where
    /// missing.
    pub fn make_folder(&self, given: &str) -> io::Result<()> {
        if self.below(given)?.is_none() {
            return Ok(());
        }
        let (dir, name) =
            self.parent(given, Missing::Make(Mode::FOLDER))?;
        dir.make(&name, Mode::FOLDER, Exists::Open).map(drop)
    }

    /// The names in the folder at `given`, sorted, each with its own
    /// status. A link is listed as one and not followed.
    pub fn list(&self, given: &str) -> io::Result<Vec<Listed>> {
        let start = self.relative(given)?;
        let dir = self.directory(given)?;
        let mut listed = Vec::new();
        for name in dir.names()? {
            if is_incoming(&name) {
                continue;
            }
            if let Some(stat) = dir.stat(&name)? {
                let name = name.to_string_lossy().into_owned();
                let path = joined(&start, &name);
                listed.push(Listed { name, path, stat });
            }
        }
        listed.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(listed)
    }

    /// Every regular file with one name at or below `given`, by its
    /// path from this folder, after at most `most` names looked at. A
    /// link is neither followed nor given, nor is a name that is not
    /// UTF-8.
    pub fn files(
        &self,
        given: &str,
        most: usize,
    ) -> io::Result<Walked> {
        let start = self.relative(given)?;
        let mut walked = Walked {
            start: start.clone(),
            ..Walked::default()
        };
        let stat = self.stat(given)?;
        match stat.kind {
            Kind::Directory => {}
            Kind::File if stat.links == 1 => {
                walked.files.push(Found {
                    path: start,
                    size: stat.size,
                });
                return Ok(walked);
            }
            Kind::File => return Ok(walked),
            kind => return Err(super::NotRegular(kind).into()),
        }
        let dir = self.directory(given)?;
        let rest = to_look_at(&dir)?;
        // One open folder for each level of the walk, with the names
        // in it still to look at.
        let mut levels = vec![(dir, start, rest)];
        let mut looked = 0;
        while let Some((dir, prefix, rest)) = levels.last_mut() {
            let Some(name) = rest.pop() else {
                levels.pop();
                continue;
            };
            looked += 1;
            if looked > most {
                walked.capped = true;
                break;
            }
            let Some(text) = name.to_str() else {
                continue;
            };
            let at = joined(prefix, text);
            let Some(stat) = dir.stat(&name)? else {
                continue;
            };
            match stat.kind {
                // A folder swapped for a link meanwhile is passed.
                Kind::Directory => {
                    if let Ok(child) = dir.child(&name) {
                        let inner = to_look_at(&child)?;
                        levels.push((child, at, inner));
                    }
                }
                Kind::File if stat.links == 1 => {
                    walked.files.push(Found {
                        path: at,
                        size: stat.size,
                    });
                }
                Kind::File | Kind::Link | Kind::Special => {}
            }
        }
        Ok(walked)
    }
}

/// `name` in the folder at `prefix`, a path from the [`Folder`].
fn joined(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

fn is_incoming(name: &Name) -> bool {
    name.to_str().is_some_and(|text| text.starts_with(INCOMING))
}

/// The names in `dir` but a write's new files, last first, so that
/// popping them gives them in order.
fn to_look_at(dir: &Directory) -> io::Result<Vec<Name>> {
    let mut names = dir.names()?;
    names.retain(|name| !is_incoming(name));
    names.sort_by(|a, b| b.cmp(a));
    Ok(names)
}

/// A new file in `dir` for a write, under a name no one else uses.
fn incoming(dir: &Directory) -> io::Result<(Name, std::fs::File)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let name = Name::new(&format!(
            "{INCOMING}{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))?;
        match dir.file(&name, Open::New(Mode::PRIVATE)) {
            Ok(file) => return Ok((name, file.into_file())),
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use hamlet_testing::Scratch;
    use hamlet_testing::links::{hard_link, symlink};

    use super::*;

    /// A folder `inside` with `memory/notes.md`, `skills/` and
    /// `inbox/`, beside a folder `outside` that holds `secret`.
    fn planted(tag: &str) -> (Scratch, Folder) {
        let scratch = Scratch::new(tag);
        fs::create_dir(scratch.join("outside")).unwrap();
        fs::write(scratch.join("outside/secret"), "SECRET").unwrap();
        let folder = Folder::make(&scratch.join("inside")).unwrap();
        folder.write("memory/notes.md", b"notes").unwrap();
        folder.make_folder("skills").unwrap();
        folder.make_folder("inbox").unwrap();
        (scratch, folder)
    }

    fn outside_is_untouched(scratch: &Scratch) {
        let names = fs::read_dir(scratch.join("outside"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, ["secret"]);
        assert_eq!(
            fs::read_to_string(scratch.join("outside/secret"))
                .unwrap(),
            "SECRET"
        );
    }

    #[test]
    fn a_file_is_written_and_read_by_a_relative_or_an_absolute_path()
    {
        let (scratch, folder) = planted("folder-read");
        let absolute =
            path::text(&scratch.join("inside/memory/notes.md"));
        for given in [
            "memory/notes.md",
            "skills/../memory/notes.md",
            "./memory//notes.md",
            &absolute,
        ] {
            assert_eq!(
                folder.read(given).unwrap(),
                b"notes",
                "{given}"
            );
        }
        folder.write("skills/new/SKILL.md", b"made").unwrap();
        assert_eq!(
            fs::read(scratch.join("inside/skills/new/SKILL.md"))
                .unwrap(),
            b"made"
        );
        folder.write("memory/notes.md", b"again").unwrap();
        assert_eq!(folder.read("memory/notes.md").unwrap(), b"again");
        assert_eq!(
            folder
                .list("memory")
                .unwrap()
                .iter()
                .map(|listed| listed.name.as_str())
                .collect::<Vec<_>>(),
            ["notes.md"],
            "a write leaves no file of its own behind"
        );
        assert_eq!(
            folder.list("memory").unwrap()[0].path,
            "memory/notes.md"
        );
    }

    #[test]
    fn a_path_that_leaves_the_folder_is_refused() {
        let (scratch, folder) = planted("folder-leave");
        let secret = path::text(&scratch.join("outside/secret"));
        for given in [
            "../outside/secret",
            "memory/../../outside/secret",
            "../../../../../../outside/secret",
            &secret,
        ] {
            let refused = folder.read(given).unwrap_err();
            assert!(Outside::is(&refused), "{given}: {refused}");
            let refused = folder.write(given, b"x").unwrap_err();
            assert!(Outside::is(&refused), "{given}: {refused}");
            assert!(folder.list(given).is_err(), "{given}");
            assert!(folder.files(given, 10).is_err(), "{given}");
        }
        assert!(folder.write("../made", b"x").is_err());
        assert!(folder.read("memory/notes.md\0").is_err());
        assert!(folder.write("memory\\..\\..\\made", b"x").is_err());
        assert!(!scratch.join("made").exists());
        outside_is_untouched(&scratch);
    }

    #[test]
    fn a_link_is_never_followed() {
        let (scratch, folder) = planted("folder-link");
        let inside = scratch.join("inside");
        symlink(
            scratch.join("outside/secret"),
            inside.join("memory/file"),
        )
        .unwrap();
        symlink(scratch.join("outside"), inside.join("skills/out"))
            .unwrap();
        symlink("../memory/notes.md", inside.join("inbox/near"))
            .unwrap();

        for given in
            ["memory/file", "skills/out/secret", "inbox/near"]
        {
            assert!(folder.read(given).is_err(), "{given}");
        }
        assert!(folder.write("skills/out/new", b"x").is_err());
        assert!(folder.list("skills/out").is_err());
        assert!(folder.files("skills/out", 10).is_err());
        assert!(folder.make_folder("skills/out/new").is_err());

        let skills = folder.list("skills").unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].stat.kind, Kind::Link);
        let walked = folder.files("", 100).unwrap();
        assert_eq!(
            walked.files,
            [Found {
                path: "memory/notes.md".into(),
                size: 5
            }]
        );

        // A write at a link's name replaces the link, not its target.
        folder.write("memory/file", b"mine").unwrap();
        assert_eq!(folder.read("memory/file").unwrap(), b"mine");
        outside_is_untouched(&scratch);
    }

    #[test]
    fn a_file_with_another_name_is_neither_read_nor_found() {
        let (scratch, folder) = planted("folder-hard");
        hard_link(
            scratch.join("outside/secret"),
            scratch.join("inside/memory/twin"),
        )
        .unwrap();
        let refused = folder.read("memory/twin").unwrap_err();
        assert!(
            matches!(refused.get_ref(), Some(inner) if inner.is::<NotLone>())
        );
        let walked = folder.files("memory", 100).unwrap();
        assert_eq!(walked.files.len(), 1);
        assert_eq!(walked.files[0].path, "memory/notes.md");
        folder.write("memory/twin", b"mine").unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join("outside/secret"))
                .unwrap(),
            "SECRET"
        );
    }

    #[test]
    fn a_write_over_a_folder_is_refused_and_a_walk_stops_at_its_bound()
     {
        let (_scratch, folder) = planted("folder-bound");
        let refused = folder.write("memory", b"x").unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::IsADirectory);
        for at in ["a", "b", "c", "d"] {
            folder.write(&format!("inbox/{at}"), b"x").unwrap();
        }
        let walked = folder.files("inbox", 3).unwrap();
        assert!(walked.capped);
        assert_eq!(walked.start, "inbox");
        assert_eq!(
            walked
                .files
                .iter()
                .map(|found| found.path.as_str())
                .collect::<Vec<_>>(),
            ["inbox/a", "inbox/b", "inbox/c"]
        );
        let walked = folder.files("memory/notes.md", 3).unwrap();
        assert!(!walked.capped);
        assert_eq!(walked.files.len(), 1);
    }

    /// A thread swaps a folder for a link to outside and back while
    /// calls run, as an agent's command could.
    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_meanwhile_leads_nowhere_outside() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        let (scratch, folder) = planted("folder-race");
        let work = scratch.join("inside/memory/work");
        let parked = scratch.join("inside/memory/parked");
        fs::create_dir(&work).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let swapper = {
            let stop = Arc::clone(&stop);
            let outside = scratch.join("outside");
            std::thread::spawn(move || {
                let mut swaps = 0;
                while !stop.load(Ordering::Relaxed) {
                    if fs::rename(&work, &parked).is_err() {
                        continue;
                    }
                    if symlink(&outside, &work).is_ok() {
                        swaps += 1;
                        fs::remove_file(&work).unwrap();
                    }
                    if fs::rename(&parked, &work).is_err() {
                        let _ = fs::remove_dir_all(&parked);
                    }
                }
                swaps
            })
        };
        let (mut written, mut refused) = (0, 0);
        for round in 0..2000 {
            match folder.write(&format!("memory/work/{round}"), b"x")
            {
                Ok(()) => written += 1,
                Err(_) => refused += 1,
            }
            assert!(folder.read("memory/work/secret").is_err());
            if let Ok(listed) = folder.list("memory/work") {
                assert!(
                    listed
                        .iter()
                        .all(|listed| listed.name != "secret")
                );
            }
            if let Ok(walked) = folder.files("memory", 10_000) {
                assert!(
                    walked
                        .files
                        .iter()
                        .all(|found| !found.path.ends_with("secret"))
                );
            }
        }
        stop.store(true, Ordering::Relaxed);
        let swaps = swapper.join().unwrap();
        assert!(swaps > 100, "{swaps} swaps");
        assert!(
            written > 0 && refused > 0,
            "{written} written, {refused} refused"
        );
        outside_is_untouched(&scratch);
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_is_never_followed() {
        let (scratch, folder) = planted("folder-junction");
        hamlet_testing::links::junction(
            scratch.join("outside"),
            scratch.join("inside/skills/out"),
        )
        .unwrap();
        assert!(folder.read("skills/out/secret").is_err());
        assert!(folder.write("skills/out/new", b"x").is_err());
        assert!(folder.list("skills/out").is_err());
        assert_eq!(
            folder.list("skills").unwrap()[0].stat.kind,
            Kind::Link
        );
        assert_eq!(folder.files("", 100).unwrap().files.len(), 1);
        outside_is_untouched(&scratch);
    }
}
