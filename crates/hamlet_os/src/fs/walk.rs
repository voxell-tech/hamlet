use std::collections::VecDeque;
use std::io;
use std::path::Path;

use super::{
    Directory, Exists, Kind, LastLink, Mode, Name, Open, Regular,
    Stat, path,
};

/// The most symbolic links one path may pass, as Linux allows, more
/// than macOS does.
pub const MAX_LINKS: usize = 40;

/// The mode a tool's new folder is made with, as the umask allows.
const NEW_FOLDER: Mode = Mode::new(0o777);

/// Whether [`reach`] follows a link at the last name.
#[derive(Clone, Copy, Debug)]
pub enum Follow {
    /// As `stat` and `open` do.
    Last,
    /// As `lstat` does: the link itself.
    NotLast,
}

/// Where a path leads: the deepest folder on the way that exists,
/// held open, and the names from there. An operation goes on from the
/// folder with no link followed, so a link swapped in after the check
/// leads nowhere.
#[derive(Debug)]
pub struct Reached {
    dir: Directory,
    missing: Option<Vec<Name>>,
    last: Option<Name>,
    /// The real path, the one a policy checks.
    pub real: String,
    /// The path as the agent gave it, made absolute, for messages.
    pub resolved: String,
}

/// A path that cannot be reached.
#[derive(Debug, thiserror::Error)]
pub enum Unreached {
    #[error(transparent)]
    System(#[from] io::Error),
    /// A folder on the way is no longer where its path says, as when
    /// a link was swapped in meanwhile.
    #[error("the path changed while it was being reached")]
    Moved,
    /// The path passes more than [`MAX_LINKS`] links.
    #[error("too many links")]
    Loop,
}

/// `resolved`, an absolute path, reached from the root. Past the
/// deepest folder that exists, `.` and `..` lead where they would
/// once the missing folders are made.
pub fn reach(
    resolved: &str,
    follow: Follow,
) -> Result<Reached, Unreached> {
    let mut rest = resolved
        .split('/')
        .map(str::to_owned)
        .collect::<VecDeque<_>>();
    let mut base = "/".to_owned();
    let mut dir = open_dir(&base)?;
    let mut links = 0;
    while let Some(part) = rest.pop_front() {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            dir = parent(&mut base)?;
            continue;
        }
        let name = Name::new(&part).map_err(io::Error::from)?;
        let kind =
            dir.stat(&name).ok().flatten().map(|stat| stat.kind);
        let last = rest.is_empty();
        let link = match kind {
            Some(Kind::Link)
                if matches!(follow, Follow::Last) || !last =>
            {
                dir.read_link(&name).ok()
            }
            _ => None,
        };
        if let Some(link) = link {
            // The system refuses a path through more links, so it
            // leads nowhere.
            links += 1;
            if links > MAX_LINKS {
                return Err(Unreached::Loop);
            }
            if path::is_absolute(&link) {
                base = "/".into();
                dir = open_dir(&base)?;
            }
            for piece in link.split('/').rev() {
                rest.push_front(piece.to_owned());
            }
            continue;
        }
        let below = match kind {
            Some(Kind::Directory) if !last => dir.child(&name).ok(),
            _ => None,
        };
        match below {
            Some(below) => {
                dir = below;
                base = path::join(&[base.as_str(), &part]);
            }
            None if last => {
                return reached(
                    dir,
                    &base,
                    None,
                    Some(name),
                    resolved,
                );
            }
            None if kind.is_some() => {
                return reached(
                    dir,
                    &base,
                    Some(vec![name]),
                    None,
                    resolved,
                );
            }
            None => return beyond(dir, base, name, rest, resolved),
        }
    }
    reached(dir, &base, None, None, resolved)
}

fn beyond(
    mut dir: Directory,
    mut base: String,
    name: Name,
    rest: VecDeque<String>,
    resolved: &str,
) -> Result<Reached, Unreached> {
    let at_folder = matches!(
        rest.back().map(String::as_str),
        Some("" | "." | "..")
    );
    let mut names = vec![name];
    for part in rest {
        match part.as_str() {
            "" | "." => {}
            ".." => {
                if names.pop().is_none() {
                    dir = parent(&mut base)?;
                }
            }
            _ => {
                names
                    .push(Name::new(&part).map_err(io::Error::from)?);
            }
        }
    }
    let last = if at_folder { None } else { names.pop() };
    reached(dir, &base, Some(names), last, resolved)
}

/// Moves `base` to its parent and opens it: the folder `..` leads to,
/// since `base` holds no link.
fn parent(base: &mut String) -> io::Result<Directory> {
    *base = path::dirname(base);
    open_dir(base)
}

/// The walk's end at `dir`, the folder `base`. Only the case of the
/// names may differ from the folder held: the system following a link
/// swapped in meanwhile would differ more.
fn reached(
    dir: Directory,
    base: &str,
    missing: Option<Vec<Name>>,
    last: Option<Name>,
    resolved: &str,
) -> Result<Reached, Unreached> {
    let real = cased(base);
    if real.to_lowercase() != base.to_lowercase() {
        return Err(Unreached::Moved);
    }
    let mut parts = vec![real];
    parts.extend(
        missing
            .iter()
            .flatten()
            .chain(&last)
            .map(|name| name.to_string_lossy().into_owned()),
    );
    Ok(Reached {
        dir,
        missing,
        last,
        real: path::join(&parts),
        resolved: resolved.to_owned(),
    })
}

/// `real`, a path without symbolic links, with each name in the case
/// its volume keeps.
pub fn cased(real: &str) -> String {
    std::fs::canonicalize(real)
        .map(|path| path::text(&path))
        .unwrap_or_else(|_| real.to_owned())
}

/// `real`, a path without links, opened one name at a time, following
/// none.
fn open_dir(real: &str) -> io::Result<Directory> {
    let mut dir = Directory::open(Path::new("/"), LastLink::Refuse)?;
    for name in real.split('/').filter(|name| !name.is_empty()) {
        dir = dir.child(&Name::new(name)?)?;
    }
    Ok(dir)
}

impl Reached {
    fn last(&self) -> io::Result<Option<&Name>> {
        match self.missing {
            Some(_) => Err(io::ErrorKind::NotFound.into()),
            None => Ok(self.last.as_ref()),
        }
    }

    /// The last name's own status.
    pub fn stat(&self) -> io::Result<Stat> {
        match self.last()? {
            Some(name) => self
                .dir
                .stat(name)?
                .ok_or_else(|| io::ErrorKind::NotFound.into()),
            None => self.dir.status(),
        }
    }

    /// The last name as a directory, following no link.
    pub fn directory(&self) -> io::Result<Directory> {
        match self.last()? {
            Some(name) => self.dir.child(name),
            None => self.dir.try_clone(),
        }
    }

    /// The last name as a regular file, following no link.
    pub fn file(&self, how: Open) -> io::Result<Regular> {
        match self.last()? {
            Some(name) => self.dir.file(name, how),
            None => Err(io::ErrorKind::IsADirectory.into()),
        }
    }

    /// Makes the missing folders, so that only the last name is left.
    pub fn make_folders(&mut self) -> io::Result<()> {
        if let Some(folders) = &self.missing {
            for name in folders {
                self.dir =
                    self.dir.make(name, NEW_FOLDER, Exists::Open)?;
            }
            self.missing = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read as _, Write as _};

    use hamlet_testing::Scratch;
    use hamlet_testing::links::symlink;

    use super::*;

    fn at(path: &Path, follow: Follow) -> Reached {
        reach(&path::text(path), follow).unwrap()
    }

    fn read(at: &Reached) -> io::Result<String> {
        let mut text = String::new();
        at.file(Open::Read)?.into_file().read_to_string(&mut text)?;
        Ok(text)
    }

    fn write(mut at: Reached, text: &str) -> io::Result<()> {
        at.make_folders()?;
        at.file(Open::Truncate(Mode::new(0o644)))?
            .file()
            .write_all(text.as_bytes())
    }

    #[test]
    fn a_dot_in_a_link_past_a_missing_folder_is_skipped() {
        let scratch = Scratch::new("walk-dot");
        symlink("missing/./x", scratch.join("l")).unwrap();
        let at = at(&scratch.join("l"), Follow::Last);
        assert_eq!(at.real, path::text(&scratch.join("missing/x")));
        assert_eq!(
            at.stat().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        write(at, "made").unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join("missing/x")).unwrap(),
            "made"
        );
    }

    #[test]
    fn a_dot_dot_in_a_link_backs_out_of_a_missing_folder() {
        let scratch = Scratch::new("walk-back");
        fs::write(scratch.join("f"), "old").unwrap();
        symlink("new/../f", scratch.join("l")).unwrap();
        let at = at(&scratch.join("l"), Follow::Last);
        assert_eq!(at.real, path::text(&scratch.join("f")));
        assert_eq!(
            read(&at).unwrap_err().kind(),
            io::ErrorKind::NotFound,
            "a read finds nothing past a missing folder"
        );
        write(at, "new").unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join("f")).unwrap(),
            "new"
        );
        assert!(
            !scratch.join("new").exists(),
            "the folder the link backs out of is not made"
        );
    }

    #[test]
    fn a_dot_dot_in_a_link_above_the_deepest_folder_reaches_its_parent()
     {
        let scratch = Scratch::new("walk-climb");
        fs::create_dir(scratch.join("held")).unwrap();
        symlink("new/../../f", scratch.join("held/l")).unwrap();
        let at = at(&scratch.join("held/l"), Follow::Last);
        assert_eq!(at.real, path::text(&scratch.join("f")));
        assert_eq!(
            at.stat().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        write(at, "above").unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join("f")).unwrap(),
            "above"
        );
        assert!(!scratch.join("held/new").exists());
    }

    #[test]
    fn nothing_is_reached_past_a_name_that_is_not_a_folder() {
        let scratch = Scratch::new("walk-file");
        fs::write(scratch.join("file"), "kept").unwrap();
        symlink("file/../x", scratch.join("back")).unwrap();
        symlink("file/", scratch.join("slash")).unwrap();
        for link in ["back", "slash"] {
            let at = at(&scratch.join(link), Follow::Last);
            assert_eq!(
                at.stat().unwrap_err().kind(),
                io::ErrorKind::NotFound
            );
            assert_eq!(
                write(at, "lost").unwrap_err().kind(),
                io::ErrorKind::NotADirectory
            );
        }
        assert!(!scratch.join("x").exists());
        assert_eq!(
            fs::read_to_string(scratch.join("file")).unwrap(),
            "kept"
        );
    }

    #[test]
    fn a_link_ending_at_a_missing_folder_is_never_written_as_a_file()
    {
        let scratch = Scratch::new("walk-ends");
        for (link, target) in
            [("slash", "a/"), ("dot", "b/."), ("back", "c/d/..")]
        {
            symlink(target, scratch.join(link)).unwrap();
            let at = at(&scratch.join(link), Follow::Last);
            let made = at.real.clone();
            assert!(write(at, "lost").is_err(), "{link}");
            assert!(Path::new(&made).is_dir(), "{link}");
        }
    }

    #[test]
    fn a_link_swapped_after_the_walk_changes_nothing_reached() {
        let scratch = Scratch::new("walk-swap");
        let inside = scratch.join("inside");
        let outside = scratch.join("outside");
        for (folder, text) in
            [(&inside, "shown"), (&outside, "hidden")]
        {
            fs::create_dir(folder).unwrap();
            fs::write(folder.join("secret.txt"), text).unwrap();
        }
        let link = scratch.join("link");
        symlink(&inside, &link).unwrap();
        let read_at = at(&link.join("secret.txt"), Follow::Last);
        let folder_at = at(&link, Follow::Last);
        let write_at = at(&link.join("new.txt"), Follow::Last);
        fs::remove_file(&link).unwrap();
        symlink(&outside, &link).unwrap();

        assert_eq!(
            read_at.real,
            path::text(&inside.join("secret.txt"))
        );
        assert_eq!(read(&read_at).unwrap(), "shown");
        assert_eq!(folder_at.real, path::text(&inside));
        assert_eq!(
            folder_at.directory().unwrap().status().unwrap().id,
            Directory::open(&inside, LastLink::Refuse)
                .unwrap()
                .status()
                .unwrap()
                .id
        );
        assert_eq!(
            write_at.real,
            path::text(&inside.join("new.txt"))
        );
        write(write_at, "kept").unwrap();
        assert!(inside.join("new.txt").exists());
        assert!(!outside.join("new.txt").exists());
    }

    #[test]
    fn a_path_through_more_than_forty_links_leads_nowhere() {
        let scratch = Scratch::new("walk-loop");
        fs::write(scratch.join("file"), "end").unwrap();
        let mut target = "file".to_owned();
        for each in 0..=MAX_LINKS {
            let link = format!("l{each}");
            symlink(&target, scratch.join(&link)).unwrap();
            target = link;
        }
        let through = |links: usize| {
            reach(
                &path::text(&scratch.join(format!("l{}", links - 1))),
                Follow::Last,
            )
        };
        assert_eq!(
            through(MAX_LINKS).unwrap().real,
            path::text(&scratch.join("file"))
        );
        assert!(matches!(
            through(MAX_LINKS + 1),
            Err(Unreached::Loop)
        ));
    }

    #[test]
    fn a_dot_dot_after_a_link_leaves_the_folder_it_leads_to() {
        let scratch = Scratch::new("walk-link-back");
        fs::create_dir_all(scratch.join("a/b")).unwrap();
        symlink("a/b", scratch.join("l")).unwrap();
        assert_eq!(
            at(&scratch.join("l/../c"), Follow::Last).real,
            path::text(&scratch.join("a/c"))
        );
    }

    #[test]
    fn an_absolute_link_target_is_reached_from_the_root() {
        let scratch = Scratch::new("walk-absolute");
        fs::create_dir_all(scratch.join("deep")).unwrap();
        fs::create_dir(scratch.join("target")).unwrap();
        symlink(scratch.join("target"), scratch.join("deep/l"))
            .unwrap();
        assert_eq!(
            at(&scratch.join("deep/l/file"), Follow::Last).real,
            path::text(&scratch.join("target/file"))
        );
    }

    #[test]
    fn a_write_through_a_dangling_link_makes_its_target() {
        let scratch = Scratch::new("walk-dangling");
        symlink("gone", scratch.join("l")).unwrap();
        let link = at(&scratch.join("l"), Follow::NotLast);
        assert_eq!(link.stat().unwrap().kind, Kind::Link);
        let target = at(&scratch.join("l"), Follow::Last);
        assert_eq!(target.real, path::text(&scratch.join("gone")));
        assert_eq!(
            target.stat().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        write(target, "made").unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join("gone")).unwrap(),
            "made"
        );
    }

    #[test]
    fn a_link_at_the_last_name_is_followed_only_with_follow_last() {
        let scratch = Scratch::new("walk-follow");
        fs::write(scratch.join("file"), "text").unwrap();
        symlink("file", scratch.join("l")).unwrap();
        let followed = at(&scratch.join("l"), Follow::Last);
        assert_eq!(followed.real, path::text(&scratch.join("file")));
        assert_eq!(followed.stat().unwrap().kind, Kind::File);
        let own = at(&scratch.join("l"), Follow::NotLast);
        assert_eq!(own.real, path::text(&scratch.join("l")));
        assert_eq!(own.stat().unwrap().kind, Kind::Link);
    }
}
