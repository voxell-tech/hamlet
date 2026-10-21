//! Opening for a plugin: an `https` address in the browser, or a file
//! or folder under a folder it asked for in the program the system
//! keeps for it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use hamlet_engine_api::desktop::WEB;
use hamlet_engine_api::{Ask, Capability, Failure};

/// The endings of the files the system runs when it is asked to open
/// them, in small letters.
const RUNS: &[&str] = &[
    "action", "app", "appimage", "bash", "bat", "cmd", "com",
    "command", "cpl", "csh", "deb", "desktop", "dmg", "exe",
    "fileloc", "fish", "gadget", "hta", "inetloc", "inf", "jar",
    "jnlp", "js", "jse", "kext", "ksh", "lnk", "mpkg", "msc", "msi",
    "msp", "osax", "pif", "pkg", "prefpane", "ps1", "py", "pyw",
    "reg", "rpm", "run", "saver", "scf", "scpt", "scptd", "scr",
    "sh", "snap", "terminal", "tool", "url", "vbe", "vbs", "webloc",
    "workflow", "wsc", "wsf", "wsh", "zsh",
];

/// The characters of an address or a path that a refusal repeats.
const CUT: usize = 60;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Opened>().add_systems(Update, reap);
}

/// What a plugin asked to open.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Opening {
    /// An `https` address.
    Web(String),
    /// A file or folder, with every link on the way to it followed.
    File(PathBuf),
}

/// The programs started to open something, until each is done.
#[derive(Resource, Default)]
pub(crate) struct Opened(pub(crate) Vec<Child>);

/// What `written` opens for a plugin that asks `asks`, and the target
/// of its `desktop.open` ask it falls under: [`WEB`] for an `https`
/// address with a host and no user, or the first folder asked for
/// that holds the file or folder `written` names, whole or from `~/`.
pub fn opening(
    asks: &[Ask],
    written: &str,
) -> Result<(Opening, String), Failure> {
    let shown = written.chars().take(CUT).collect::<String>();
    let refused =
        |why: &str| Failure::Refused(format!("{shown:?} {why}"));
    if written.chars().any(char::is_control) {
        return Err(refused("is not an address or a path"));
    }
    let mut targets = asks
        .iter()
        .filter(|ask| ask.capability == Capability::DesktopOpen)
        .map(|ask| ask.target.as_str());
    if let Some((scheme, rest)) = written.split_once("://") {
        if !scheme.eq_ignore_ascii_case(WEB) {
            return Err(refused("is not https"));
        }
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        if host.is_empty()
            || host.contains('@')
            || written.chars().any(char::is_whitespace)
        {
            return Err(refused("is not an https address of a host"));
        }
        return targets
            .find(|target| *target == WEB)
            .map(|target| {
                (Opening::Web(written.to_owned()), target.to_owned())
            })
            .ok_or_else(|| {
                Failure::Refused(format!(
                    "{} {WEB} is not asked for",
                    Capability::DesktopOpen.name()
                ))
            });
    }
    let path = expand(written)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| refused("is not a whole path"))?
        .canonicalize()
        .map_err(|_| refused("is not there"))?;
    targets
        .filter(|target| *target != WEB)
        .find(|target| {
            expand(target)
                .filter(|folder| folder.is_absolute())
                .and_then(|folder| folder.canonicalize().ok())
                .is_some_and(|folder| path.starts_with(folder))
        })
        .map(|target| {
            (Opening::File(path.clone()), target.to_owned())
        })
        .ok_or_else(|| refused("is not in a folder asked for"))
}

/// `written` with a leading `~` as the person's own folder. None
/// when it starts with `~` and there is no such folder.
fn expand(written: &str) -> Option<PathBuf> {
    match written.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let mine = std::env::home_dir()?;
            Some(mine.join(rest.trim_start_matches('/')))
        }
        _ => Some(PathBuf::from(written)),
    }
}

impl Opening {
    /// Starts the program of the system that opens it, given it as
    /// one argument that no shell reads. A file the system would run
    /// is shown in its folder instead.
    pub(crate) fn start(&self) -> std::io::Result<Child> {
        let mut program = match self {
            Self::Web(address) => browser(address),
            Self::File(path) => shower(path, runs(path)),
        };
        program
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }
}

/// Whether the system would run `path` if it were asked to open it.
fn runs(path: &Path) -> bool {
    let listed = path
        .extension()
        .and_then(|ending| ending.to_str())
        .is_some_and(|ending| {
            RUNS.contains(&ending.to_ascii_lowercase().as_str())
        });
    listed || executable(path)
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|file| {
        file.is_file() && file.permissions().mode() & 0o111 != 0
    })
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    false
}

/// The program that shows `path`, opened or, when `reveal`, in its
/// folder.
fn shower(path: &Path, reveal: bool) -> Command {
    if cfg!(target_os = "macos") {
        let mut open = Command::new("open");
        if reveal {
            open.arg("-R");
        }
        open.arg(path);
        open
    } else if cfg!(target_os = "windows") {
        // The explorer does not read the long form of a path.
        let path = path.to_string_lossy();
        let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
        let mut open = Command::new("explorer");
        if reveal {
            open.arg(format!("/select,{path}"));
        } else {
            open.arg(path);
        }
        open
    } else {
        let mut open = Command::new("xdg-open");
        match (reveal, path.parent()) {
            (true, Some(folder)) => open.arg(folder),
            _ => open.arg(path),
        };
        open
    }
}

/// The program that opens `address` in the browser.
fn browser(address: &str) -> Command {
    let mut open = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(target_os = "windows") {
        let mut open = Command::new("rundll32");
        open.arg("url.dll,FileProtocolHandler");
        open
    } else {
        Command::new("xdg-open")
    };
    open.arg(address);
    open
}

/// Lets go of the programs that are done.
fn reap(mut opened: ResMut<Opened>) {
    if opened.0.is_empty() {
        return;
    }
    opened
        .0
        .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
}
