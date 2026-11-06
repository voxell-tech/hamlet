//! The server a client connects to: the one named by the environment,
//! or the one running for the local realm.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The WebSocket URL and connection token of an agent server.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerAddress {
    pub url: String,
    pub token: String,
}

impl fmt::Debug for ServerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerAddress")
            .field("url", &self.url)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

/// The failure to find a server.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `HAMLET_SERVER` is set without `HAMLET_TOKEN`.
    #[error("HAMLET_SERVER needs HAMLET_TOKEN, its realm's token.")]
    NoToken,
    /// Neither `HAMLET_HOME` nor the chief's home directory is set.
    #[error(
        "No home directory found. Set HAMLET_HOME, or HAMLET_SERVER \
         and HAMLET_TOKEN."
    )]
    NoHomeDirectory,
    /// The realm's path cannot be made absolute.
    #[error("{}: {error}", .home.display())]
    Unresolvable {
        home: PathBuf,
        error: std::io::Error,
    },
    /// No server keeps a record in this realm's folder.
    #[error(
        "No server runs for {}. Start one with hamlet server.",
        .0.display()
    )]
    NotRunning(PathBuf),
    /// This `server.json` cannot be read or names no server.
    #[error(
        "Cannot read {}. Inspect it before starting or stopping its \
         server.",
        .0.display()
    )]
    UnreadableRecord(PathBuf),
    /// This `credentials.json` cannot be read.
    #[error("Cannot read {}.", .0.display())]
    UnreadableCredentials(PathBuf),
    /// This `credentials.json` holds no connection token.
    #[error(
        "Cannot read {}: connection.token must be nonempty text.",
        .0.display()
    )]
    NoConnectionToken(PathBuf),
}

impl ServerAddress {
    /// The server named by the environment, or the one running for
    /// the local realm.
    pub fn find(
        vars: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, Error> {
        match choose(vars)? {
            Chosen::At(address) => Ok(address),
            Chosen::Home(home) => Self::in_home(&home),
        }
    }

    /// The server running for the realm in the folder `home`.
    pub fn in_home(home: &Path) -> Result<Self, Error> {
        Running::read(home)?
            .ok_or_else(|| Error::NotRunning(home.to_owned()))?
            .address(home)
    }
}

/// The server the environment points a client at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// `HAMLET_SERVER` with its `HAMLET_TOKEN`.
    At(ServerAddress),
    /// The local realm's folder, whose server may start later.
    Home(PathBuf),
}

/// The server chosen by `vars`, where an empty variable is unset.
pub fn choose(
    vars: impl Fn(&str) -> Option<String>,
) -> Result<Chosen, Error> {
    let var =
        |name: &str| vars(name).filter(|value| !value.is_empty());
    if let Some(url) = var("HAMLET_SERVER") {
        let token = var("HAMLET_TOKEN").ok_or(Error::NoToken)?;
        return Ok(Chosen::At(ServerAddress { url, token }));
    }
    home(vars).map(Chosen::Home)
}

/// The absolute folder of the local realm: `HAMLET_HOME`, else
/// `.hamlet` in the chief's home directory. An empty variable is
/// unset.
pub fn home(
    vars: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, Error> {
    let var =
        |name: &str| vars(name).filter(|value| !value.is_empty());
    let home = match var("HAMLET_HOME") {
        Some(home) => PathBuf::from(home),
        None => var("HOME")
            .or_else(|| var("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".hamlet"))
            .ok_or(Error::NoHomeDirectory)?,
    };
    std::path::absolute(&home)
        .map_err(|error| Error::Unresolvable { home, error })
}

/// The record a running server keeps in its realm's folder,
/// `server.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct Running {
    pub url: String,
    pub pid: u32,
    /// When it started, in the server's words.
    pub started: String,
    /// What keeps its agents' commands from the rest of the machine;
    /// a server older than the field does not say.
    #[serde(default)]
    pub confinement: Option<crate::client::Confinement>,
}

impl Running {
    /// The record of the server running in the folder `home`, or
    /// `None` when no
    /// server keeps one there.
    pub fn read(home: &Path) -> Result<Option<Self>, Error> {
        let path = home.join("server.json");
        let unreadable = || Error::UnreadableRecord(path.clone());
        let text = match std::fs::read(&path) {
            Ok(text) => text,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(None);
            }
            Err(_) => return Err(unreadable()),
        };
        serde_json::from_slice::<Self>(&text)
            .ok()
            .filter(|running| {
                running.pid > 0 && !running.url.is_empty()
            })
            .map(Some)
            .ok_or_else(unreadable)
    }

    /// This server's address, with the token of the realm in `home`.
    pub fn address(
        &self,
        home: &Path,
    ) -> Result<ServerAddress, Error> {
        let path = home.join("credentials.json");
        let text = std::fs::read(&path).map_err(|_| {
            Error::UnreadableCredentials(path.clone())
        })?;
        let token =
            serde_json::from_slice::<serde_json::Value>(&text)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/connection/token")
                        .and_then(serde_json::Value::as_str)
                        .filter(|token| !token.is_empty())
                        .map(str::to_owned)
                })
                .ok_or(Error::NoConnectionToken(path))?;
        Ok(ServerAddress {
            url: self.url.clone(),
            token,
        })
    }
}
