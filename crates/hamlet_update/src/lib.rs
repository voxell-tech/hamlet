//! Hamlet updating itself from its releases
//! (docs/releases.md#updates): how this copy was installed, where
//! updates are kept, and putting one in place.

use std::fs;
use std::path::{Path, PathBuf};

use hamlet_protocol::Version;

mod apply;
#[cfg(feature = "check")]
mod check;

pub use apply::{Applied, apply};
#[cfg(feature = "check")]
pub use check::{Asset, FEED, Feed, Found, client, download};

/// The public key, in base64, that a release's manifest is signed
/// with. Only a release build is given it; a copy without it never
/// looks for an update.
pub const KEY: Option<&str> = match option_env!("HAMLET_UPDATE_KEY") {
    Some(key) if !key.is_empty() => Some(key),
    _ => None,
};

/// This copy's version.
pub fn current() -> Version {
    env!("CARGO_PKG_VERSION")
        .parse()
        .expect("the workspace's version is semver")
}

/// Whether this copy looks for updates: a release, unless
/// `HAMLET_NO_UPDATE` is set.
pub fn is_enabled() -> bool {
    KEY.is_some()
        && std::env::var_os("HAMLET_NO_UPDATE")
            .is_none_or(|value| value.is_empty())
}

/// How this copy of Hamlet was installed, which says how an update is
/// put in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// The macOS app at this path.
    App(PathBuf),
    /// Windows' installer, for the account running it.
    Setup,
    /// A Debian package.
    Deb,
    /// An RPM package.
    Rpm,
    /// An archive or a build from source, which a chief replaces by
    /// hand.
    Manual,
}

impl Install {
    /// How the programs beside this one were installed.
    pub fn here() -> Self {
        std::env::current_exe()
            .map_or(Self::Manual, |exe| Self::of(&exe))
    }

    /// How the programs beside `exe` were installed.
    pub fn of(exe: &Path) -> Self {
        let Some(folder) = exe.parent() else {
            return Self::Manual;
        };
        if cfg!(target_os = "macos")
            && let Some(bundle) = folder
                .parent()
                .and_then(Path::parent)
                .filter(|bundle| {
                    bundle.extension().is_some_and(|end| end == "app")
                })
        {
            return Self::App(bundle.to_path_buf());
        }
        if cfg!(windows) && folder.join("unins000.exe").is_file() {
            return Self::Setup;
        }
        if cfg!(target_os = "linux")
            && folder == Path::new("/opt/hamlet")
        {
            if Path::new("/var/lib/dpkg/info/hamlet.list").is_file() {
                return Self::Deb;
            }
            let rpm = std::process::Command::new("rpm")
                .args(["--quiet", "-qf"])
                .arg(exe)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if rpm.is_ok_and(|status| status.success()) {
                return Self::Rpm;
            }
        }
        Self::Manual
    }

    /// The name of this kind's file in a release's manifest.
    pub fn file(&self) -> Option<&'static str> {
        match self {
            Self::App(_) => Some("macos"),
            Self::Setup => Some("windows"),
            Self::Deb => Some("deb"),
            Self::Rpm => Some("rpm"),
            Self::Manual => None,
        }
    }
}

/// The folder downloaded updates are kept in.
pub fn folder() -> Option<PathBuf> {
    let var = |name| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let cache = if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Caches/Hamlet")
    } else if cfg!(windows) {
        var("LOCALAPPDATA")?.join("Hamlet")
    } else {
        var("XDG_CACHE_HOME")
            .map(|cache| cache.join("hamlet"))
            .or_else(|| {
                var("HOME").map(|home| home.join(".cache/hamlet"))
            })?
    };
    Some(cache.join("updates"))
}

/// The note of the version an update was last put in place to.
fn note() -> Option<PathBuf> {
    Some(folder()?.join("attempt"))
}

/// Notes that an update to `version` is being put in place, so the
/// next start can tell whether it took.
pub fn attempting(version: Version) -> std::io::Result<()> {
    let Some(note) = note() else {
        return Ok(());
    };
    if let Some(folder) = note.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(note, version.to_string())
}

/// The version the last update was to, when this copy is still older:
/// that update did not take. Once one has, its download is removed.
pub fn missed() -> Option<Version> {
    let note = note()?;
    let noted = fs::read_to_string(&note).ok()?;
    let _ = fs::remove_file(&note);
    let version = noted.trim().parse::<Version>().ok()?;
    if version > current() {
        return Some(version);
    }
    if let Some(folder) = note.parent() {
        let _ = fs::remove_dir_all(folder);
    }
    None
}
