//! The server a client connects to: the one named by the environment,
//! or the one running for the local home.

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

impl ServerAddress {
    /// The server named by the environment, or the one running for
    /// the local home.
    pub fn find(
        vars: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        match choose(vars)? {
            Chosen::At(address) => Ok(address),
            Chosen::Home(home) => Self::in_home(&home),
        }
    }

    /// The server running for `home`.
    pub fn in_home(home: &Path) -> Result<Self, String> {
        Running::read(home)?
            .ok_or_else(|| {
                format!(
                    "No server runs for {}. Start one with hamlet \
                     server.",
                    home.display()
                )
            })?
            .address(home)
    }
}

/// The server the environment points a client at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// `HAMLET_SERVER` with its `HAMLET_TOKEN`.
    At(ServerAddress),
    /// The local home, whose server may start later.
    Home(PathBuf),
}

/// The server chosen by `vars`, where an empty variable is unset.
pub fn choose(
    vars: impl Fn(&str) -> Option<String>,
) -> Result<Chosen, String> {
    let var =
        |name: &str| vars(name).filter(|value| !value.is_empty());
    if let Some(url) = var("HAMLET_SERVER") {
        let token = var("HAMLET_TOKEN").ok_or(
            "HAMLET_SERVER needs HAMLET_TOKEN, its home's token.",
        )?;
        return Ok(Chosen::At(ServerAddress { url, token }));
    }
    home(vars).map(Chosen::Home)
}

/// The absolute local home: `HAMLET_HOME`, else `.hamlet` in the
/// chief's home directory. An empty variable is unset.
pub fn home(
    vars: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, String> {
    let var =
        |name: &str| vars(name).filter(|value| !value.is_empty());
    let home = match var("HAMLET_HOME") {
        Some(home) => PathBuf::from(home),
        None => var("HOME")
            .or_else(|| var("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".hamlet"))
            .ok_or(
                "No home directory found. Set HAMLET_HOME, or \
                 HAMLET_SERVER and HAMLET_TOKEN.",
            )?,
    };
    std::path::absolute(&home)
        .map_err(|error| format!("{}: {error}", home.display()))
}

/// The record a running server keeps in its home, `server.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct Running {
    pub url: String,
    pub pid: u32,
    /// When it started, in the server's words.
    pub started: Option<String>,
}

impl Running {
    /// The record of the server running for `home`, or `None` when no
    /// server keeps one there.
    pub fn read(home: &Path) -> Result<Option<Self>, String> {
        let path = home.join("server.json");
        let unreadable = || {
            format!(
                "Cannot read {}. Inspect it before starting or \
                 stopping its server.",
                path.display()
            )
        };
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

    /// This server's address, with the token of `home`.
    pub fn address(
        &self,
        home: &Path,
    ) -> Result<ServerAddress, String> {
        let path = home.join("credentials.json");
        let text = std::fs::read(&path).map_err(|_| {
            format!("Cannot read {}.", path.display())
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
                .ok_or_else(|| {
                    format!(
                        "Cannot read {}: connection.token must be \
                         nonempty text.",
                        path.display()
                    )
                })?;
        Ok(ServerAddress {
            url: self.url.clone(),
            token,
        })
    }
}
