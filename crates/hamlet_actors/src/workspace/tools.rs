//! Tools a chief adds by hand (C17): a program and its manifest in a
//! folder of `tools/`, which agents read and never write. A tool runs
//! with the reach its manifest grants, rather than the agent's.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_messages::workspace::{
    AddedTool, CallRefusal, ConversationId, Program, Run, ToolList,
};
use hamlet_os::fs::path::{resolve, text};
use hamlet_sandbox::{LimitError, Policy};
use serde_json::{Value, json};

use super::reach::{Network, Op, Placement, RealmFolder};
use super::run::{Exec, LONGEST};

/// A tool that loaded, with the program it runs.
pub struct Tool {
    pub spec: AddedTool,
    dir: PathBuf,
    exec: Vec<String>,
    policy: Policy,
}

impl Tool {
    /// Whether its manifest reaches the network.
    pub fn has_network(&self) -> bool {
        self.policy.net
    }

    /// Its time limit, from its manifest.
    fn timeout(&self) -> Duration {
        Duration::from_secs(self.policy.limits.wall_secs)
    }

    pub fn placement(&self, network: Network) -> Placement {
        let mut policy = self.policy.clone();
        policy.net &= network == Network::On;
        Placement {
            policy,
            cwd: self.dir.clone(),
        }
    }
}

pub enum Ready<'a> {
    Script {
        script: &'a str,
        network: bool,
        timeout: Duration,
    },
    Tool {
        tool: Box<Tool>,
        arguments: &'a str,
    },
}

impl<'a> Ready<'a> {
    /// The program of `run`, or why it is not run: a tool that does
    /// not load, or arguments its manifest does not take.
    pub fn of(
        home: &RealmFolder,
        run: &'a Run,
    ) -> Result<Self, CallRefusal> {
        let (name, arguments) = match &run.program {
            Program::Script {
                script,
                network,
                timeout_ms,
            } => {
                return Ok(Self::Script {
                    script,
                    network: *network,
                    timeout: timeout_ms
                        .map_or(LONGEST, Duration::from_millis)
                        .min(LONGEST),
                });
            }
            Program::Tool { name, arguments } => (name, arguments),
        };
        let tool = load(home)
            .0
            .into_iter()
            .find(|tool| tool.spec.name == *name)
            .ok_or_else(|| {
                CallRefusal::Invalid(format!(
                    "Tool \"{name}\" not found"
                ))
            })?;
        let schema =
            serde_json::from_str::<Value>(&tool.spec.parameters)
                .ok()
                .and_then(|schema| {
                    jsonschema::validator_for(&schema).ok()
                });
        let valid = serde_json::from_str::<Value>(arguments)
            .is_ok_and(|arguments| {
                arguments.is_object()
                    && schema.is_some_and(|schema| {
                        schema.is_valid(&arguments)
                    })
            });
        if !valid {
            return Err(CallRefusal::Invalid(format!(
                "Validation failed for tool {name}"
            )));
        }
        Ok(Self::Tool {
            tool: Box::new(tool),
            arguments,
        })
    }

    /// The call in `conversation` that runs this, as the gate weighs
    /// it.
    pub fn op(&self, conversation: &'a ConversationId) -> Op<'a> {
        match self {
            Self::Script {
                script, network, ..
            } => Op::Command {
                conversation,
                script,
                network: *network,
            },
            Self::Tool { tool, .. } => Op::Tool {
                conversation,
                network: tool.has_network(),
            },
        }
    }

    pub fn exec(self) -> (Exec, Duration) {
        match self {
            Self::Script {
                script, timeout, ..
            } => (Exec::Script(script.to_owned()), timeout),
            Self::Tool { tool, arguments } => {
                let timeout = tool.timeout();
                (
                    Exec::Tool {
                        argv: tool.exec,
                        arguments: arguments.to_owned(),
                    },
                    timeout,
                )
            }
        }
    }
}

/// `run` in words, for a chief.
pub fn words(run: &Run) -> String {
    match &run.program {
        Program::Script { script, .. } => script.clone(),
        Program::Tool { name, arguments } => {
            format!("{name} {arguments}")
        }
    }
}

/// The tools that load, as a model is offered them, and why the
/// others do not.
pub fn list(home: &RealmFolder) -> ToolList {
    let (tools, faults) = load(home);
    ToolList {
        tools: tools.into_iter().map(|tool| tool.spec).collect(),
        faults,
    }
}

// Copied from `hamlet_server/src/work/added.rs`, with the names it
// reserves from `work/tools.rs` and `work/catalogue.rs`.
/// The names a tool added by hand may not take: the built-in tools
/// and the society's.
const RESERVED: [&str; 35] = [
    "read",
    "write",
    "edit",
    "bash",
    "powershell",
    "search",
    "find",
    "tell",
    "ask",
    "start_worker",
    "submit_skill",
    "ask_librarian",
    "ask_chief",
    "read_budget",
    "object",
    "request_road",
    "create_scholar",
    "hire",
    "end_hire",
    "merge_hires",
    "set_model",
    "set_tools",
    "set_allowance",
    "nickname",
    "lay_road",
    "take_up_road",
    "agree_road",
    "found_hamlet",
    "seat_mayor",
    "remove_hamlet",
    "read_record",
    "read_scholar",
    "edit_scholar",
    "tag",
    "revert",
];

/// The fault in a tool's folder that keeps it from loading.
#[derive(Debug, thiserror::Error)]
enum Unloaded {
    #[error("not a folder")]
    NotFolder,
    #[error(transparent)]
    Unread(std::io::Error),
    #[error("manifest.json is not valid JSON")]
    NotJson,
    #[error("missing name")]
    NoName,
    #[error("exec must be an array")]
    ExecNotArray,
    #[error("exec is empty")]
    ExecEmpty,
    #[error("name '{0}' must match [a-zA-Z0-9_-]{{1,64}}")]
    BadName(String),
    #[error(
        "parameters do not compile: parameters must be a JSON \
         Schema object"
    )]
    ParametersNotObject,
    #[error("parameters do not compile: {0}")]
    Uncompiled(Box<jsonschema::ValidationError<'static>>),
    #[error(transparent)]
    Limit(#[from] LimitError),
    #[error("name '{0}' is reserved")]
    Reserved(String),
    #[error("name '{0}' is taken by another tool")]
    Taken(String),
}

/// The tools that load, in folder order, and why the others do not.
fn load(home: &RealmFolder) -> (Vec<Tool>, Vec<String>) {
    let folder = home.tools();
    let mut tools = Vec::new();
    let mut faults = Vec::new();
    let entries = match std::fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                faults.push(format!("{}: {error}", folder.display()));
            }
            return (tools, faults);
        }
    };
    let mut entries =
        entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut names = BTreeSet::new();
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() && !kind.is_symlink() {
            continue;
        }
        let loaded = (|| {
            let dir = std::fs::canonicalize(&path)
                .map_err(|_| Unloaded::NotFolder)?;
            if !dir.is_dir() {
                return Err(Unloaded::NotFolder);
            }
            let data =
                std::fs::read_to_string(dir.join("manifest.json"))
                    .map_err(Unloaded::Unread)?;
            let tool = parse(&data, &dir, &folder)?;
            let name = &tool.spec.name;
            if RESERVED.contains(&name.as_str()) {
                return Err(Unloaded::Reserved(name.clone()));
            }
            if !names.insert(name.clone()) {
                return Err(Unloaded::Taken(name.clone()));
            }
            Ok(tool)
        })();
        match loaded {
            Ok(tool) => tools.push(tool),
            Err(unloaded) => {
                faults
                    .push(format!("{}: {unloaded}", path.display()));
            }
        }
    }
    (tools, faults)
}

fn parse(
    data: &str,
    dir: &Path,
    tools: &Path,
) -> Result<Tool, Unloaded> {
    let value = serde_json::from_str::<Value>(data)
        .map_err(|_| Unloaded::NotJson)?;
    let name = value["name"].as_str().ok_or(Unloaded::NoName)?;
    let exec = value["exec"]
        .as_array()
        .ok_or(Unloaded::ExecNotArray)?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if exec.is_empty() {
        return Err(Unloaded::ExecEmpty);
    }
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|c| {
            c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
        })
    {
        return Err(Unloaded::BadName(name.into()));
    }
    let parameters = value
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({"type":"object","properties":{}}));
    if !parameters.is_object() {
        return Err(Unloaded::ParametersNotObject);
    }
    jsonschema::validator_for(&parameters)
        .map_err(|error| Unloaded::Uncompiled(Box::new(error)))?;
    let sandbox = &value["sandbox"];
    let paths = |key: &str| {
        sandbox[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|path| PathBuf::from(resolve(&text(dir), path)))
            .collect::<Vec<_>>()
    };
    let mut env = hamlet_sandbox::base_env();
    env.extend(sandbox["env"].as_object().into_iter().flatten().map(
        |(key, value)| {
            (
                key.clone(),
                value.as_str().unwrap_or_default().to_owned(),
            )
        },
    ));
    let mut policy = Policy {
        ro: paths("fs_ro"),
        rw: paths("fs_rw"),
        env,
        net: sandbox["net"] == true,
        locked: vec![tools.to_owned(), dir.to_owned()],
        ..Policy::default()
    };
    policy.ro.push(dir.to_owned());
    for (key, field) in [
        ("wall_secs", &mut policy.limits.wall_secs),
        ("cpu_secs", &mut policy.limits.cpu_secs),
    ] {
        if let Some(seconds) = sandbox["limits"][key].as_u64() {
            *field = seconds;
        }
    }
    policy.check_limits()?;
    Ok(Tool {
        spec: AddedTool {
            name: name.into(),
            description: value["description"]
                .as_str()
                .unwrap_or_default()
                .into(),
            parameters: parameters.to_string(),
        },
        dir: dir.to_owned(),
        exec,
        policy,
    })
}
