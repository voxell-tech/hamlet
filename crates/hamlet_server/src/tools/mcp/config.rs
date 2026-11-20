//! The MCP servers a chief lists in `mcp/servers.json`, and the reach
//! each is given.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_os::fs::path::{resolve, text};
use hamlet_sandbox::{LimitError, Policy};
use serde::Deserialize;
use serde_json::Value;

use crate::realm::layout::Layout;

/// The file in the realm's `mcp` folder that lists the servers.
pub(super) const FILE: &str = "servers.json";

/// The longest a server's name is, so every tool's name fits.
const NAME_LENGTH: usize = 24;

/// The name in `allow` or `ask` that stands for every tool of a
/// server.
const EVERY: &str = "*";

/// How long a call may take when the entry says nothing.
const CALL_SECS: u64 = 120;

/// A server runs for as long as it is listed: thirty days of CPU time
/// unless its entry says less.
const CPU_SECS: u64 = 30 * 24 * 60 * 60;

/// The shortest value of a server's environment taken out of what it
/// says. A shorter one is no credential, and taking it out would take
/// out common words.
const SECRET_LENGTH: usize = 8;

/// The fault that keeps a server from loading.
#[derive(Debug, thiserror::Error)]
enum Unloaded {
    #[error(
        "its name must be 1 to {NAME_LENGTH} lowercase letters, \
         digits and '-'"
    )]
    BadName,
    /// Not an entry's shape; serde's words say where.
    #[error("the entry does not fit: {0}")]
    Unfit(serde_json::Error),
    #[error("command is empty")]
    CommandEmpty,
    #[error("timeout_secs must be at least 1")]
    NoTimeout,
    #[error("{0} is in both allow and ask")]
    AllowedAndAsked(String),
    #[error(transparent)]
    Limit(#[from] LimitError),
}

/// A server's entry as a chief writes it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    /// The program and its arguments.
    command: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Names copied from the environment of the server.
    #[serde(default)]
    inherit_env: Vec<String>,
    #[serde(default)]
    sandbox: Reach,
    timeout_secs: Option<u64>,
    /// Tools whose calls run without a chief's answer, though the
    /// server does not say they only read.
    #[serde(default)]
    allow: Vec<String>,
    /// Tools whose calls wait for a chief, though the server says
    /// they only read.
    #[serde(default)]
    ask: Vec<String>,
}

/// The reach of a server beyond its own folder.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Reach {
    /// Paths it may read, from its own folder.
    fs_ro: Vec<String>,
    /// Paths it may write, from its own folder.
    fs_rw: Vec<String>,
    net: bool,
    limits: Limits,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Limits {
    cpu_secs: Option<u64>,
    max_procs: Option<u64>,
}

/// A server as listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Server {
    pub command: Vec<String>,
    env: BTreeMap<String, String>,
    inherit_env: Vec<String>,
    fs_ro: Vec<String>,
    fs_rw: Vec<String>,
    pub net: bool,
    cpu_secs: u64,
    max_procs: Option<u64>,
    pub timeout: Duration,
    allow: Vec<String>,
    ask: Vec<String>,
}

impl Server {
    /// The policy it runs under in the realm `layout`: the reach it
    /// declares, and its own folder `home`, which it works in and
    /// writes. The list and the folder being erased stay out of its
    /// writes, also when a path it declares holds them.
    pub(super) fn policy(
        &self,
        layout: &Layout,
        home: &Path,
    ) -> Result<Policy, LimitError> {
        let paths = |paths: &[String]| {
            paths
                .iter()
                .map(|path| PathBuf::from(resolve(&text(home), path)))
                .collect::<Vec<_>>()
        };
        let mut env = hamlet_sandbox::base_env();
        env.insert("HOME".into(), text(home));
        env.extend(self.inherited());
        env.extend(self.env.clone());
        let mut policy = Policy {
            ro: paths(&self.fs_ro),
            rw: paths(&self.fs_rw),
            env,
            net: self.net,
            locked: vec![
                layout.mcp().join(FILE),
                layout.root().join(".erasing"),
            ],
            ..Policy::default()
        };
        policy.rw.push(home.to_owned());
        policy.limits.cpu_secs = self.cpu_secs;
        if let Some(procs) = self.max_procs {
            policy.limits.max_procs = procs;
        }
        policy.check_limits()?;
        Ok(policy)
    }

    /// The values it is given that must not reach a model or a log.
    pub(super) fn secrets(&self) -> Vec<String> {
        self.env
            .values()
            .cloned()
            .chain(self.inherited().map(|(_, value)| value))
            .filter(|value| value.chars().count() >= SECRET_LENGTH)
            .collect()
    }

    /// Whether `other` runs as this does; they may differ in how long
    /// a call may take and which calls wait for a chief.
    pub(super) fn is_same_process(&self, other: &Self) -> bool {
        *self
            == Self {
                timeout: self.timeout,
                allow: self.allow.clone(),
                ask: self.ask.clone(),
                ..other.clone()
            }
    }

    /// Whether a call of its tool `tool` may change something outside
    /// the realm, so that it waits for a chief. The entry's `ask` and
    /// `allow` decide, by the tool's name before `*`; else the
    /// server's word that it only reads, `read_only`.
    pub(super) fn acts(&self, tool: &str, read_only: bool) -> bool {
        for name in [tool, EVERY] {
            if self.ask.iter().any(|asked| asked == name) {
                return true;
            }
            if self.allow.iter().any(|allowed| allowed == name) {
                return false;
            }
        }
        !read_only
    }

    /// The variables of `inherit_env` the server's environment has.
    fn inherited(&self) -> impl Iterator<Item = (String, String)> {
        self.inherit_env.iter().filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.clone(), value))
        })
    }
}

/// What the list holds: each server that loads, and a fault for each
/// entry that does not.
#[derive(Default)]
pub(super) struct Listed {
    pub servers: BTreeMap<String, Server>,
    pub faults: Vec<String>,
}

/// The fault that keeps the whole list from loading.
#[derive(Debug, thiserror::Error)]
pub(super) enum Unread {
    #[error(transparent)]
    Io(std::io::Error),
    /// Not JSON; serde's words say where.
    #[error(transparent)]
    Json(serde_json::Error),
    #[error("servers must be an object")]
    NotObject,
}

/// The list `servers.json` in the folder `folder` holds; none when
/// there is no file.
pub(super) fn load(folder: &Path) -> Result<Listed, Unread> {
    let data = match std::fs::read_to_string(folder.join(FILE)) {
        Ok(data) => data,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(Listed::default());
        }
        Err(error) => return Err(Unread::Io(error)),
    };
    let entries = match serde_json::from_str::<Value>(&data)
        .map_err(Unread::Json)?
        .get_mut("servers")
        .map(Value::take)
    {
        None => serde_json::Map::new(),
        Some(Value::Object(entries)) => entries,
        Some(_) => return Err(Unread::NotObject),
    };
    let mut listed = Listed::default();
    for (name, entry) in entries {
        match parse(&name, entry) {
            Ok(server) => {
                listed.servers.insert(name, server);
            }
            Err(unloaded) => {
                listed
                    .faults
                    .push(format!("MCP server {name}: {unloaded}"));
            }
        }
    }
    Ok(listed)
}

fn parse(name: &str, entry: Value) -> Result<Server, Unloaded> {
    if !is_name(name) {
        return Err(Unloaded::BadName);
    }
    let written = serde_json::from_value::<Written>(entry)
        .map_err(Unloaded::Unfit)?;
    if written.command.is_empty() {
        return Err(Unloaded::CommandEmpty);
    }
    if written.timeout_secs == Some(0) {
        return Err(Unloaded::NoTimeout);
    }
    if let Some(both) =
        written.allow.iter().find(|tool| written.ask.contains(tool))
    {
        return Err(Unloaded::AllowedAndAsked(both.clone()));
    }
    let server = Server {
        command: written.command,
        env: written.env,
        inherit_env: written.inherit_env,
        fs_ro: written.sandbox.fs_ro,
        fs_rw: written.sandbox.fs_rw,
        net: written.sandbox.net,
        cpu_secs: written.sandbox.limits.cpu_secs.unwrap_or(CPU_SECS),
        max_procs: written.sandbox.limits.max_procs,
        timeout: Duration::from_secs(
            written.timeout_secs.unwrap_or(CALL_SECS),
        ),
        allow: written.allow,
        ask: written.ask,
    };
    // A limit the sandbox refuses is a fault of the entry, found now.
    server.policy(&Layout::new(Path::new("/")), Path::new("/"))?;
    Ok(server)
}

/// Whether `name` can name a server: lowercase letters, digits and
/// `-`, never `_`, so `mcp__<server>__` ends where the server's name
/// does.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_LENGTH
        && name.bytes().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(json: &str) -> Listed {
        let folder = std::env::temp_dir()
            .join(format!("hamlet-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(FILE), json).unwrap();
        let loaded = load(&folder).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        loaded
    }

    #[test]
    fn a_bad_entry_does_not_keep_the_others_from_loading() {
        let Listed { servers, faults } = loaded(
            r#"{"servers": {
                "good": {"command": ["server"]},
                "Bad_Name": {"command": ["server"]},
                "empty": {"command": []},
                "wrong": {"command": "server"},
                "typo": {"command": ["server"], "sandbox": {"nett": true}},
                "zero": {"command": ["server"], "timeout_secs": 0},
                "both": {"command": ["server"], "allow": ["x"], "ask": ["x"]}
            }}"#,
        );
        assert_eq!(
            servers.keys().map(String::as_str).collect::<Vec<_>>(),
            ["good"]
        );
        assert_eq!(faults.len(), 6);
    }

    #[test]
    fn a_tool_acts_unless_it_only_reads_and_the_entry_has_the_last_word()
     {
        let servers = loaded(
            r#"{"servers": {"mail": {
                "command": ["server"],
                "allow": ["draft"],
                "ask": ["*"]
            }, "docs": {
                "command": ["server"],
                "allow": ["*"],
                "ask": ["publish"]
            }, "plain": {"command": ["server"]}}}"#,
        )
        .servers;
        assert!(servers["plain"].acts("send", false));
        assert!(!servers["plain"].acts("search", true));
        assert!(servers["mail"].acts("search", true));
        assert!(!servers["mail"].acts("draft", false));
        assert!(!servers["docs"].acts("send", false));
        assert!(servers["docs"].acts("publish", true));
    }

    #[test]
    fn a_server_writes_its_own_folder_and_what_it_declares() {
        let servers = loaded(
            r#"{"servers": {"files": {
                "command": ["server"],
                "env": {"TOKEN": "a-long-token", "LOG": "warn"},
                "sandbox": {"fs_ro": ["data"], "fs_rw": ["/srv/out"]}
            }}}"#,
        )
        .servers;
        let server = &servers["files"];
        let home = Path::new("/realm/mcp/files");
        let policy = server
            .policy(&Layout::new(Path::new("/realm")), home)
            .unwrap();
        assert_eq!(
            policy.ro,
            [PathBuf::from("/realm/mcp/files/data")]
        );
        assert_eq!(
            policy.rw,
            [PathBuf::from("/srv/out"), home.to_owned()]
        );
        assert!(!policy.net);
        assert_eq!(policy.env["HOME"], "/realm/mcp/files");
        assert!(
            policy
                .locked
                .contains(&PathBuf::from("/realm/mcp/servers.json"))
        );
        assert_eq!(server.secrets(), ["a-long-token"]);
    }
}
