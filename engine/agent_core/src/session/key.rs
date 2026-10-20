//! The key a session is found by, and the agent it reaches.

use std::path::{Path, PathBuf};

use anyhow::Result;
use bevy_ecs::prelude::*;
use hamlet_rules::hamlet::{HOME, MAYOR};
use hamlet_rules::realm::Realm;

/// The channel, chat and thread a session is found by.
#[derive(Component, Clone, PartialEq, Eq, Hash, Debug)]
pub struct SessionKey {
    pub channel: String,
    pub chat: String,
    pub thread: String,
}

/// The channel of sessions that hold messages between agents.
pub const MAIL: &str = "hamlet";

/// The channel of sessions that hold what one agent asked another
/// and waited on.
pub const ASKED: &str = "asked";

/// The start of the thread of a session with one of the hamlet's
/// agents.
const AGENT_THREAD: &str = "agent:";

/// The start of the thread of a session with the mayor of a hamlet
/// other than the first.
const MAYOR_THREAD: &str = "mayor:";

/// The agent a session's worker runs, and the hamlet it lives in.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub struct Seat {
    pub hamlet: String,
    pub name: String,
}

impl SessionKey {
    /// A session with the agent `name`.
    pub fn with_agent(channel: &str, chat: &str, name: &str) -> Self {
        SessionKey {
            channel: channel.into(),
            chat: chat.into(),
            thread: format!("{AGENT_THREAD}{name}"),
        }
    }

    /// A session with whoever is mayor of `hamlet`.
    pub fn with_mayor(
        channel: &str,
        chat: &str,
        hamlet: &str,
    ) -> Self {
        SessionKey {
            channel: channel.into(),
            chat: chat.into(),
            thread: if hamlet == HOME {
                "main".into()
            } else {
                format!("{MAYOR_THREAD}{hamlet}")
            },
        }
    }

    /// The session `to` keeps for what the agent at `from` asks it
    /// and waits on. `to` is an agent's name, `mayor` in `hamlet`,
    /// or `mayor@<hamlet>`.
    pub(super) fn asked(from: &str, to: &str, hamlet: &str) -> Self {
        match to.strip_prefix(MAYOR) {
            Some("") => Self::with_mayor(ASKED, from, hamlet),
            Some(place) if place.starts_with('@') => {
                Self::with_mayor(ASKED, from, &place[1..])
            }
            _ => Self::with_agent(ASKED, from, to),
        }
    }

    /// True when this is the session an agent keeps with another
    /// agent that writes to it.
    pub fn is_mail(&self) -> bool {
        self.channel == MAIL
    }

    /// True when this is the session an agent keeps with another
    /// agent that asks it and waits.
    pub fn is_asked(&self) -> bool {
        self.channel == ASKED
    }

    /// The agent this session talks to. `None` for a mayor.
    pub fn agent(&self) -> Option<&str> {
        self.thread.strip_prefix(AGENT_THREAD)
    }

    /// The name its agent goes by between agents: the office for a
    /// mayor.
    pub fn address(&self) -> &str {
        self.agent().unwrap_or(MAYOR)
    }

    /// The hamlet whose mayor this session talks to. `None` when it
    /// is with a named agent.
    pub fn office(&self) -> Option<&str> {
        match self.agent() {
            Some(_) => None,
            None => Some(
                self.thread
                    .strip_prefix(MAYOR_THREAD)
                    .unwrap_or(HOME),
            ),
        }
    }

    /// The agent the session reaches now, with the hamlet it lives
    /// in. `Err` says why nobody is there.
    pub(super) fn seat(&self, realm: &Realm) -> Result<Seat> {
        let found = match (self.agent(), self.office()) {
            (Some(name), _) => realm.resolve(name),
            (None, Some(hamlet)) => realm
                .hamlet(hamlet)
                .ok()
                .and_then(|h| Some((h.again(), h.resolve(MAYOR)?))),
            (None, None) => None,
        };
        let Some((hamlet, resident)) = found else {
            let gone = self.agent().and_then(|name| {
                realm.hamlets().into_iter().find_map(|place| {
                    realm.hamlet(&place.name).ok()?.whereabouts(name)
                })
            });
            match (gone, self.agent()) {
                (Some(gone), _) => anyhow::bail!(gone),
                (None, Some(name)) => {
                    anyhow::bail!("no agent named `{name}`")
                }
                (None, None) => {
                    anyhow::bail!(
                        "nobody is there: `{}`",
                        self.thread
                    )
                }
            }
        };
        Ok(Seat {
            hamlet: hamlet.name().to_string(),
            name: resident.name,
        })
    }

    pub(super) fn log_path(&self, sessions: &Path) -> PathBuf {
        // An asked agent's log is named for who asks.
        if self.is_asked() {
            return sessions.join(format!("{}.jsonl", self.chat));
        }
        let clean = |s: &str| {
            s.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        };
        sessions.join(format!(
            "{}_{}_{}.jsonl",
            clean(&self.channel),
            clean(&self.chat),
            clean(&self.thread)
        ))
    }
}
