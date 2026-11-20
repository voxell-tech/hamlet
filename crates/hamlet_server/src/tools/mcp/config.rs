//! The MCP servers a chief lists in `mcp/servers.json`, and the reach
//! each is given.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_os::fs::path::{resolve, text};
use hamlet_protocol::client::McpEntry;
use hamlet_sandbox::{LimitError, Policy};
use serde::Deserialize;
use serde_json::{Map, Value, json};

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
pub(super) enum Unloaded {
    #[error(
        "its name must be 1 to {NAME_LENGTH} lowercase letters, \
         digits and '-'"
    )]
    BadName,
    /// Not an entry's shape; serde's words say where.
    #[error("the entry does not fit: {0}")]
    Unfit(serde_json::Error),
    #[error("it needs a command or a url")]
    CommandEmpty,
    #[error("it has both a command and a url")]
    CommandAndUrl,
    #[error(
        "a server reached by a url runs nowhere here, so it takes \
         no {0}"
    )]
    RemoteKey(&'static str),
    #[error(
        "a program takes no {0}, which is for a server reached by a \
         url"
    )]
    LocalKey(&'static str),
    #[error("url must be an https address, or http on this machine")]
    BadUrl,
    #[error("timeout_secs must be at least 1")]
    NoTimeout,
    #[error("{0} is in both allow and ask")]
    AllowedAndAsked(String),
    #[error("{0} has no value")]
    NoValue(String),
    #[error(transparent)]
    Limit(#[from] LimitError),
}

/// A server's entry as a chief writes it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    /// The program and its arguments.
    #[serde(default)]
    command: Vec<String>,
    /// The address of a server reached over streamable HTTP, instead
    /// of a program.
    url: Option<String>,
    /// The headers each request to `url` carries, such as a token.
    #[serde(default)]
    headers: BTreeMap<String, String>,
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
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
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
        // A header such as `Bearer <token>` holds its secret last.
        let headers = self.headers.values().flat_map(|value| {
            [value.clone()].into_iter().chain(
                value.split_whitespace().last().map(String::from),
            )
        });
        self.env
            .values()
            .cloned()
            .chain(self.inherited().map(|(_, value)| value))
            .chain(headers)
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

    /// Its entry as a client is shown it: its environment's names,
    /// never their values, and no limit it was not given.
    pub(super) fn entry(&self) -> McpEntry {
        McpEntry {
            command: self.command.clone(),
            url: self.url.clone(),
            headers: self
                .headers
                .keys()
                .map(|name| (name.clone(), None))
                .collect(),
            env: self
                .env
                .keys()
                .map(|name| (name.clone(), None))
                .collect(),
            inherit_env: self.inherit_env.clone(),
            fs_ro: self.fs_ro.clone(),
            fs_rw: self.fs_rw.clone(),
            net: self.net,
            timeout_secs: Some(self.timeout.as_secs())
                .filter(|secs| *secs != CALL_SECS),
            allow: self.allow.clone(),
            ask: self.ask.clone(),
        }
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
    #[error(r#"it must be an object, as {{"servers": {{...}}}}"#)]
    NotObject,
    #[error(r#"it has no servers; {{"servers": {{}}}} lists none"#)]
    NoServers,
    #[error("it holds {0}, and only servers belongs in it")]
    Unknown(String),
}

/// The list `servers.json` in the folder `folder` holds; none when
/// there is no file.
pub(super) fn load(folder: &Path) -> Result<Listed, Unread> {
    let entries = file(folder)?
        .get_mut("servers")
        .map(Value::take)
        .and_then(|entries| match entries {
            Value::Object(entries) => Some(entries),
            _ => None,
        })
        .unwrap_or_default();
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

/// The file `servers.json` in `folder` as written, its servers an
/// object; `{"servers": {}}` when there is no file.
fn file(folder: &Path) -> Result<Value, Unread> {
    let data = match std::fs::read_to_string(folder.join(FILE)) {
        Ok(data) => data,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(json!({"servers": {}}));
        }
        Err(error) => return Err(Unread::Io(error)),
    };
    let file =
        serde_json::from_str::<Value>(&data).map_err(Unread::Json)?;
    // A misspelled or misplaced list would stop every server, so it
    // changes none.
    let Some(object) = file.as_object() else {
        return Err(Unread::NotObject);
    };
    if let Some(key) = object.keys().find(|key| *key != "servers") {
        return Err(Unread::Unknown(key.clone()));
    }
    match object.get("servers") {
        Some(Value::Object(_)) => Ok(file),
        Some(_) => Err(Unread::NotObject),
        None => Err(Unread::NoServers),
    }
}

/// A change to the list a chief asked for that was not made.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Unchanged {
    #[error("{FILE} cannot be read, so it is not changed: {0}")]
    Unread(#[from] Unread),
    #[error("MCP server {0}: {1}")]
    Unfit(String, Unloaded),
    #[error("There is no MCP server {0}.")]
    NoServer(String),
    #[error("{FILE} cannot be written: {0}")]
    Unwritten(#[from] std::io::Error),
}

/// Sets the entry of the server `name` in the list in `folder` to
/// `entry`: an environment value it leaves out is the one the entry
/// has, and what a client is not shown, such as the limits, is kept.
/// The rest of the file stays as written.
pub(super) fn put(
    folder: &Path,
    name: &str,
    entry: &McpEntry,
) -> Result<(), Unchanged> {
    let mut whole = file(folder)?;
    let servers = &mut whole["servers"];
    let before = servers.get(name).cloned().unwrap_or(json!({}));
    let written = written(&before, entry)
        .and_then(|written| {
            parse(name, written.clone()).map(|_| written)
        })
        .map_err(|unloaded| {
            Unchanged::Unfit(name.into(), unloaded)
        })?;
    servers[name] = written;
    write(folder, &whole)
}

/// Takes the server `name` out of the list in `folder`.
pub(super) fn remove(
    folder: &Path,
    name: &str,
) -> Result<(), Unchanged> {
    let mut whole = file(folder)?;
    let servers = whole["servers"]
        .as_object_mut()
        .expect("the file's servers are an object");
    if servers.remove(name).is_none() {
        return Err(Unchanged::NoServer(name.into()));
    }
    write(folder, &whole)
}

/// Adds the entries `added` to the list in `folder`, each unless the
/// list has its name; the names added.
pub(super) fn add(
    folder: &Path,
    added: Vec<(String, Value)>,
) -> Result<Vec<String>, Unchanged> {
    let mut whole = file(folder)?;
    let servers = whole["servers"]
        .as_object_mut()
        .expect("the file's servers are an object");
    let mut names = Vec::new();
    for (name, entry) in added {
        if servers.contains_key(&name) {
            continue;
        }
        parse(&name, entry.clone()).map_err(|unloaded| {
            Unchanged::Unfit(name.clone(), unloaded)
        })?;
        servers.insert(name.clone(), entry);
        names.push(name);
    }
    if !names.is_empty() {
        write(folder, &whole)?;
    }
    Ok(names)
}

/// Why the server `name` with `entry`, written anew, would not load.
pub(super) fn check(
    name: &str,
    entry: &McpEntry,
) -> Result<(), Unloaded> {
    parse(name, written(&json!({}), entry)?).map(|_| ())
}

/// `before`, an entry as written, with what `entry` sets.
fn written(
    before: &Value,
    entry: &McpEntry,
) -> Result<Value, Unloaded> {
    let mut written = before.as_object().cloned().unwrap_or_default();
    let env = kept(&entry.env, &before["env"])?;
    let headers = kept(&entry.headers, &before["headers"])?;
    let mut set = |key: &str, value: Value, empty: bool| {
        if empty {
            written.remove(key);
        } else {
            written.insert(key.into(), value);
        }
    };
    set("command", json!(entry.command), entry.command.is_empty());
    set("url", json!(entry.url), entry.url.is_none());
    set("headers", Value::Object(headers), entry.headers.is_empty());
    set("env", Value::Object(env), entry.env.is_empty());
    set(
        "inherit_env",
        json!(entry.inherit_env),
        entry.inherit_env.is_empty(),
    );
    set(
        "timeout_secs",
        json!(entry.timeout_secs),
        entry.timeout_secs.is_none(),
    );
    set("allow", json!(entry.allow), entry.allow.is_empty());
    set("ask", json!(entry.ask), entry.ask.is_empty());
    let mut sandbox =
        before["sandbox"].as_object().cloned().unwrap_or_default();
    for (key, paths) in
        [("fs_ro", &entry.fs_ro), ("fs_rw", &entry.fs_rw)]
    {
        if paths.is_empty() {
            sandbox.remove(key);
        } else {
            sandbox.insert(key.into(), json!(paths));
        }
    }
    if entry.net {
        sandbox.insert("net".into(), true.into());
    } else {
        sandbox.remove("net");
    }
    let empty = sandbox.is_empty();
    set("sandbox", Value::Object(sandbox), empty);
    Ok(Value::Object(written))
}

/// The values `values` names, each one left out the value `before`
/// has.
fn kept(
    values: &BTreeMap<String, Option<String>>,
    before: &Value,
) -> Result<Map<String, Value>, Unloaded> {
    values
        .iter()
        .map(|(name, value)| {
            let value = match value {
                Some(value) => Value::String(value.clone()),
                None => before
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Unloaded::NoValue(name.clone()))?,
            };
            Ok((name.clone(), value))
        })
        .collect()
}

/// Writes `whole` as the list in `folder`, at once: a reader finds it
/// as it was or as it is.
fn write(folder: &Path, whole: &Value) -> Result<(), Unchanged> {
    std::fs::create_dir_all(folder)?;
    let file = folder.join(FILE);
    let partial = folder.join(format!(".{FILE}.partial"));
    let mut text = serde_json::to_string_pretty(whole)
        .expect("a JSON value is written");
    text.push('\n');
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, &file)?;
    Ok(())
}

/// The server `name` with the entry `entry`, as the list loads it.
pub(super) fn parse(
    name: &str,
    entry: Value,
) -> Result<Server, Unloaded> {
    if !is_name(name) {
        return Err(Unloaded::BadName);
    }
    let written = serde_json::from_value::<Written>(entry)
        .map_err(Unloaded::Unfit)?;
    match (&written.url, written.command.is_empty()) {
        (None, true) => return Err(Unloaded::CommandEmpty),
        (Some(_), false) => return Err(Unloaded::CommandAndUrl),
        (Some(url), true) => remote(url, &written)?,
        (None, false) => {
            if !written.headers.is_empty() {
                return Err(Unloaded::LocalKey("headers"));
            }
        }
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
        // A server reached by a url is reached over the network.
        url: written.url.clone(),
        headers: written.headers,
        env: written.env,
        inherit_env: written.inherit_env,
        fs_ro: written.sandbox.fs_ro,
        fs_rw: written.sandbox.fs_rw,
        net: written.sandbox.net || written.url.is_some(),
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

/// Why the entry `written` of a server reached by `url` does not
/// fit: an address other than https, or http on this machine, or what
/// only a program takes.
fn remote(url: &str, written: &Written) -> Result<(), Unloaded> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| Unloaded::BadUrl)?;
    let local = matches!(
        parsed.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    );
    if !(parsed.scheme() == "https"
        || parsed.scheme() == "http" && local)
    {
        return Err(Unloaded::BadUrl);
    }
    let sandbox = &written.sandbox;
    if !written.env.is_empty() {
        return Err(Unloaded::RemoteKey("env"));
    }
    if !written.inherit_env.is_empty() {
        return Err(Unloaded::RemoteKey("inherit_env"));
    }
    if !sandbox.fs_ro.is_empty()
        || !sandbox.fs_rw.is_empty()
        || sandbox.limits.cpu_secs.is_some()
        || sandbox.limits.max_procs.is_some()
    {
        return Err(Unloaded::RemoteKey("sandbox"));
    }
    Ok(())
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
                "both": {"command": ["server"], "allow": ["x"], "ask": ["x"]},
                "remote": {"url": "https://mcp.example.com/mcp"},
                "remote-env": {"url": "https://mcp.example.com", "env": {"A": "b"}},
                "plain": {"url": "http://example.com/mcp"}
            }}"#,
        );
        assert_eq!(
            servers.keys().map(String::as_str).collect::<Vec<_>>(),
            ["good", "remote"]
        );
        assert_eq!(faults.len(), 8);
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
    fn a_list_that_is_not_all_servers_changes_none() {
        let folder = std::env::temp_dir()
            .join(format!("hamlet-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        for (written, unread) in [
            ("[]", "an object"),
            (r#"{"servres": {}}"#, "servres"),
            ("{}", "no servers"),
        ] {
            std::fs::write(folder.join(FILE), written).unwrap();
            let Err(fault) = load(&folder) else {
                panic!("{written} loads");
            };
            assert!(fault.to_string().contains(unread), "{fault}");
        }
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_change_keeps_what_a_client_is_not_shown() {
        let folder = std::env::temp_dir()
            .join(format!("hamlet-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(FILE),
            r#"{"servers": {"mail": {
                "command": ["mail"],
                "env": {"TOKEN": "a-long-token"},
                "sandbox": {"limits": {"max_procs": 4}}
            }}}"#,
        )
        .unwrap();
        let mut entry =
            load(&folder).unwrap().servers["mail"].entry();
        assert_eq!(entry.env["TOKEN"], None);
        entry.env.insert("MODE".into(), Some("fast".into()));
        entry.net = true;
        put(&folder, "mail", &entry).unwrap();
        let whole = file(&folder).unwrap();
        assert_eq!(
            whole["servers"]["mail"],
            json!({
                "command": ["mail"],
                "env": {"TOKEN": "a-long-token", "MODE": "fast"},
                "sandbox": {"limits": {"max_procs": 4}, "net": true}
            })
        );
        entry.env.insert("NEW".into(), None);
        assert!(matches!(
            put(&folder, "mail", &entry),
            Err(Unchanged::Unfit(_, Unloaded::NoValue(_)))
        ));
        remove(&folder, "mail").unwrap();
        assert!(load(&folder).unwrap().servers.is_empty());
        std::fs::remove_dir_all(&folder).unwrap();
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
