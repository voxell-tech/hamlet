//! The MCP servers other programs list, each read as an entry of
//! Hamlet's list (C36):
//!
//! - Claude Code's in `<config>/.claude.json`, where `<config>` is
//!   `CLAUDE_CONFIG_DIR` or the home folder: its own `mcpServers` and
//!   each project's;
//! - Claude Desktop's `claude_desktop_config.json` in its folder of
//!   the system's application data;
//! - Cursor's `~/.cursor/mcp.json`;
//! - Codex's `[mcp_servers]` in `<home>/config.toml`, where `<home>`
//!   is `CODEX_HOME` or `~/.codex`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::folder;

/// The longest name of a server in Hamlet's list.
const NAME_LENGTH: usize = 24;

/// A program that lists MCP servers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpSource {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    Codex,
}

impl McpSource {
    /// The program's name as a person reads it.
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Cursor => "Cursor",
            Self::Codex => "Codex",
        }
    }
}

/// An MCP server another program lists.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundServer {
    pub source: McpSource,
    /// Its name in Hamlet's list: the program's, in the characters a
    /// server's name takes.
    pub name: String,
    /// Its entry in `mcp/servers.json`, or why Hamlet cannot run it.
    pub entry: Result<Value, Unrunnable>,
}

/// Why a server another program lists cannot be one of Hamlet's.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unrunnable {
    #[error(
        "it speaks the older SSE transport, and Hamlet only \
         streamable HTTP"
    )]
    Sse,
    #[error("it names no command")]
    NoCommand,
    #[error(
        "it runs in a working folder of its own, and a server in \
         Hamlet runs in its own folder"
    )]
    Cwd,
    #[error("its token comes from {0}, which is not set here")]
    NoToken(String),
}

/// Every MCP server Claude Code, Claude Desktop, Cursor and Codex
/// list on this machine, each once: a server two programs list alike
/// is found from the first, and two that differ under one name are
/// named apart.
pub fn mcp_servers() -> Vec<FoundServer> {
    let mut found = Vec::<FoundServer>::new();
    let listed = claude_code()
        .into_iter()
        .chain(claude_desktop())
        .chain(cursor())
        .chain(codex());
    for (source, name, written) in listed {
        let mut server = FoundServer {
            source,
            name: server_name(&name),
            entry: entry(&written),
        };
        let again = found.iter().any(|earlier| {
            earlier.name == server.name
                && earlier.entry == server.entry
        });
        if again {
            continue;
        }
        server.name = apart(&server.name, &found);
        found.push(server);
    }
    found
}

/// `name`, or with `-2`, `-3`… when one of `found` has it already,
/// cut so it fits.
fn apart(name: &str, found: &[FoundServer]) -> String {
    let taken =
        |name: &str| found.iter().any(|each| each.name == name);
    if !taken(name) {
        return name.to_owned();
    }
    (2..)
        .map(|n| {
            let tail = format!("-{n}");
            let head = name
                .chars()
                .take(NAME_LENGTH - tail.len())
                .collect::<String>();
            format!("{}{tail}", head.trim_end_matches('-'))
        })
        .find(|name| !taken(name))
        .expect("some number is free")
}

/// A server as another program lists it, by its name there.
type Listed = (McpSource, String, Map<String, Value>);

fn claude_code() -> Vec<Listed> {
    let Some(config) = folder("CLAUDE_CONFIG_DIR", "") else {
        return Vec::new();
    };
    let Some(file) = json(&config.join(".claude.json")) else {
        return Vec::new();
    };
    let mut lists = vec![&file["mcpServers"]];
    if let Some(projects) = file["projects"].as_object() {
        lists.extend(projects.values().map(|p| &p["mcpServers"]));
    }
    lists
        .into_iter()
        .flat_map(|list| servers_in(McpSource::ClaudeCode, list))
        .collect()
}

fn claude_desktop() -> Vec<Listed> {
    let data = if cfg!(target_os = "macos") {
        std::env::home_dir()
            .map(|home| home.join("Library/Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        folder("XDG_CONFIG_HOME", ".config")
    };
    data.and_then(|data| {
        json(&data.join("Claude/claude_desktop_config.json"))
    })
    .map(|file| {
        servers_in(McpSource::ClaudeDesktop, &file["mcpServers"])
    })
    .unwrap_or_default()
}

fn cursor() -> Vec<Listed> {
    std::env::home_dir()
        .and_then(|home| json(&home.join(".cursor/mcp.json")))
        .map(|file| {
            servers_in(McpSource::Cursor, &file["mcpServers"])
        })
        .unwrap_or_default()
}

fn codex() -> Vec<Listed> {
    let Some(home) = folder("CODEX_HOME", ".codex") else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(home.join("config.toml"))
    else {
        return Vec::new();
    };
    let Ok(config) = toml::from_str::<Value>(&text) else {
        return Vec::new();
    };
    servers_in(McpSource::Codex, &config["mcp_servers"])
        .into_iter()
        .map(|(source, name, mut written)| {
            // Codex names a call's limit so.
            if let Some(secs) = written.remove("tool_timeout_sec") {
                written.insert("timeout".into(), secs);
            }
            (source, name, written)
        })
        .collect()
}

/// The JSON file at `path`; none when it is missing or not JSON.
fn json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn servers_in(source: McpSource, list: &Value) -> Vec<Listed> {
    list.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, written)| {
            Some((source, name.clone(), written.as_object()?.clone()))
        })
        .collect()
}

/// `name` as a server's name in Hamlet's list: lowercase letters,
/// digits and `-`, at most 24 long.
fn server_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        let c = if c.is_ascii_lowercase() || c.is_ascii_digit() {
            c
        } else {
            '-'
        };
        if !(c == '-' && (out.is_empty() || out.ends_with('-'))) {
            out.push(c);
        }
    }
    let out = out.chars().take(NAME_LENGTH).collect::<String>();
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "server".into()
    } else {
        out.into()
    }
}

/// The entry of Hamlet's list for the server `written` as another
/// program lists it.
fn entry(written: &Map<String, Value>) -> Result<Value, Unrunnable> {
    let url = written
        .get("url")
        .or_else(|| written.get("httpUrl"))
        .and_then(Value::as_str);
    let mut entry = match url {
        Some(url) => remote(url, written)?,
        None => program(written)?,
    };
    if let Some(secs) = written
        .get("timeout")
        .and_then(Value::as_u64)
        .filter(|secs| *secs > 0)
    {
        entry.insert("timeout_secs".into(), secs.into());
    }
    Ok(entry.into())
}

/// The entry of a server that is a program on this machine, named by
/// its full path. A program installed in the home folder gets its
/// install folder to read. It reaches the network, as it did there.
fn program(
    written: &Map<String, Value>,
) -> Result<Map<String, Value>, Unrunnable> {
    if written.contains_key("cwd") {
        return Err(Unrunnable::Cwd);
    }
    let program = written
        .get("command")
        .and_then(Value::as_str)
        .filter(|command| !command.is_empty())
        .ok_or(Unrunnable::NoCommand)?;
    let resolved = resolve(program);
    let mut command = vec![Value::String(
        resolved.as_deref().map_or_else(|| program.to_owned(), text),
    )];
    command.extend(
        written
            .get("args")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|arg| Value::String(arg.into())),
    );
    let mut env = BTreeMap::new();
    // Codex names the variables it passes on so.
    let mut inherit = written
        .get("env_vars")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect::<Vec<_>>();
    for (name, value) in written
        .get("env")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let Some(value) = value.as_str() else {
            continue;
        };
        // `${NAME}` alone is the variable of the program's own
        // environment.
        match value
            .strip_prefix("${")
            .and_then(|rest| rest.strip_suffix('}'))
        {
            Some(variable) if variable == name => {
                if !inherit.contains(name) {
                    inherit.push(name.clone());
                }
            }
            _ => {
                env.insert(name.clone(), value.to_owned());
            }
        }
    }
    let mut sandbox = Map::new();
    sandbox.insert("net".into(), true.into());
    let read = resolved.as_deref().map(installed).unwrap_or_default();
    if !read.is_empty() {
        sandbox.insert("fs_ro".into(), json!(read));
    }
    let mut entry = Map::new();
    entry.insert("command".into(), command.into());
    if !env.is_empty() {
        entry.insert("env".into(), json!(env));
    }
    if !inherit.is_empty() {
        entry.insert("inherit_env".into(), json!(inherit));
    }
    entry.insert("sandbox".into(), sandbox.into());
    Ok(entry)
}

/// The entry of a server reached at `url`, with the headers its
/// requests carry. A server that signs in with OAuth is signed in to
/// again in Hamlet.
fn remote(
    url: &str,
    written: &Map<String, Value>,
) -> Result<Map<String, Value>, Unrunnable> {
    if written.get("type").and_then(Value::as_str) == Some("sse") {
        return Err(Unrunnable::Sse);
    }
    let mut entry = Map::new();
    entry.insert("url".into(), url.into());
    // Codex names them so.
    let mut headers = written
        .get("headers")
        .or_else(|| written.get("http_headers"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // Codex takes a bearer token from a variable of its environment;
    // a header holds it here.
    if let Some(variable) =
        written.get("bearer_token_env_var").and_then(Value::as_str)
    {
        let token = std::env::var(variable)
            .map_err(|_| Unrunnable::NoToken(variable.to_owned()))?;
        headers.insert(
            "Authorization".into(),
            format!("Bearer {token}").into(),
        );
    }
    if !headers.is_empty() {
        entry.insert("headers".into(), Value::Object(headers));
    }
    Ok(entry)
}

/// The program `program` names: itself when it is a path, else the
/// first of that name on `PATH`.
fn resolve(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return path.is_absolute().then(|| path.to_owned());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|folder| folder.join(program))
        .find(|candidate| candidate.is_file())
}

/// The folders a program at `path` in the home folder is installed
/// in, as a path and as the file it links to: the folder above its
/// `bin`, else its own. None for a program outside the home folder,
/// which the sandbox already lets every program read.
fn installed(path: &Path) -> Vec<String> {
    let Some(home) = std::env::home_dir() else {
        return Vec::new();
    };
    let mut read = Vec::<String>::new();
    let real = std::fs::canonicalize(path).ok();
    for path in std::iter::once(path).chain(real.as_deref()) {
        if !path.starts_with(&home) {
            continue;
        }
        let Some(folder) = path.parent() else {
            continue;
        };
        let root = match folder.file_name() {
            Some(name) if name == "bin" => {
                folder.parent().unwrap_or(folder)
            }
            _ => folder,
        };
        let root = text(root);
        if !read.contains(&root) {
            read.push(root);
        }
    }
    read
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_servers_that_differ_under_one_name_are_named_apart() {
        let found = |name: &str| FoundServer {
            source: McpSource::Codex,
            name: name.into(),
            entry: Err(Unrunnable::NoCommand),
        };
        let listed = [found("github"), found("github-2")];
        assert_eq!(apart("github", &listed), "github-3");
        assert_eq!(apart("gitlab", &listed), "gitlab");
        let long = "a".repeat(NAME_LENGTH);
        assert_eq!(apart(&long, &[found(&long)]).len(), NAME_LENGTH);
    }

    #[test]
    fn a_name_is_one_a_server_in_the_list_takes() {
        assert_eq!(server_name("GitHub"), "github");
        assert_eq!(server_name("my_server.v2"), "my-server-v2");
        assert_eq!(server_name("__"), "server");
        assert_eq!(server_name(&"a".repeat(30)).len(), NAME_LENGTH);
    }

    #[test]
    fn an_entry_keeps_the_command_and_takes_a_named_variable_from_the_server()
     {
        let written = json!({
            "command": "/opt/server",
            "args": ["--stdio"],
            "env": {"TOKEN": "${TOKEN}", "MODE": "fast"},
            "timeout": 30,
        });
        assert_eq!(
            entry(written.as_object().unwrap()),
            Ok(json!({
                "command": ["/opt/server", "--stdio"],
                "env": {"MODE": "fast"},
                "inherit_env": ["TOKEN"],
                "sandbox": {"net": true},
                "timeout_secs": 30,
            }))
        );
        let remote = json!({
            "type": "http",
            "url": "https://x",
            "headers": {"Authorization": "Bearer t"},
            "timeout": 30,
        });
        assert_eq!(
            entry(remote.as_object().unwrap()),
            Ok(json!({
                "url": "https://x",
                "headers": {"Authorization": "Bearer t"},
                "timeout_secs": 30,
            }))
        );
        let codex = json!({
            "command": "/opt/server",
            "env_vars": ["KEY"],
            "env": {"KEY": "${KEY}"},
        });
        assert_eq!(
            entry(codex.as_object().unwrap()).unwrap()["inherit_env"],
            json!(["KEY"])
        );
        let elsewhere = json!({"command": "python3", "cwd": "/srv"});
        assert_eq!(
            entry(elsewhere.as_object().unwrap()),
            Err(Unrunnable::Cwd)
        );
        let sse = json!({"type": "sse", "url": "https://x/sse"});
        assert_eq!(
            entry(sse.as_object().unwrap()),
            Err(Unrunnable::Sse)
        );
    }
}
