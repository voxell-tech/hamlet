//! Hand-added tools, their manifests and declared sandbox reach
//! (C17).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_os::fs::path::{resolve, text};
use hamlet_protocol::runtime::{
    DiagnosticSeverity, ToolSpec, ToolSpecReplay,
};
use serde_json::{Map, Value, json};

use crate::sandbox::{
    Env, ExecError, Launch, LimitError, Policy, Sandbox,
};
use crate::tools::{Outcome, diagnostic};

/// A loaded manifest and the program its tool runs.
pub(crate) struct Manifest {
    pub spec: ToolSpec,
    dir: PathBuf,
    exec: Vec<String>,
    policy: Policy,
}

impl Manifest {
    pub fn has_net(&self) -> bool {
        self.policy.net
    }
}

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
    /// Boxed, as the schema's error is larger than the rest of the
    /// manifest's errors together.
    #[error("parameters do not compile: {0}")]
    Uncompiled(Box<jsonschema::ValidationError<'static>>),
    #[error(transparent)]
    Limit(#[from] LimitError),
    #[error("name '{0}' is reserved")]
    Reserved(String),
    #[error("name '{0}' is taken by another tool")]
    Taken(String),
}

/// The tools that load, in folder order, and diagnostics for those
/// that do not.
pub(crate) fn load(root: &Path) -> (Vec<Manifest>, Vec<String>) {
    let folder = root.join("tools");
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
    let reserved = crate::tools::specs()
        .into_iter()
        .chain(crate::harness::tools::specs())
        .map(|tool| tool.name)
        .chain(["powershell".into()])
        .chain(crate::society::SocietyTool::names().map(String::from))
        .collect::<BTreeSet<_>>();
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
            let manifest = parse(&data, &dir, root)?;
            let name = &manifest.spec.name;
            if reserved.contains(name) {
                return Err(Unloaded::Reserved(name.clone()));
            }
            if !names.insert(name.clone()) {
                return Err(Unloaded::Taken(name.clone()));
            }
            Ok(manifest)
        })();
        match loaded {
            Ok(manifest) => tools.push(manifest),
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
    root: &Path,
) -> Result<Manifest, Unloaded> {
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
    let parameters = parameters
        .as_object()
        .ok_or(Unloaded::ParametersNotObject)?;
    jsonschema::validator_for(&Value::Object(parameters.clone()))
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
    let mut env = Sandbox::platform().base_env();
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
        locked: vec![
            root.join("tools"),
            dir.to_owned(),
            root.join(".gifts"),
            root.join(".erasing"),
        ],
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
    Ok(Manifest {
        spec: ToolSpec {
            name: name.into(),
            description: value["description"]
                .as_str()
                .unwrap_or_default()
                .into(),
            parameters: parameters
                .clone()
                .into_iter()
                .collect::<BTreeMap<_, _>>(),
            replay: ToolSpecReplay::Unsafe,
        },
        dir: dir.to_owned(),
        exec,
        policy,
    })
}

/// Runs a hand tool with JSON on stdin and only the reach its
/// manifest declares.
pub(crate) async fn run(
    sandbox: &Sandbox,
    root: &Path,
    net: bool,
    secrets: &[String],
    name: &str,
    args: &Map<String, Value>,
) -> Outcome {
    let Some(mut manifest) =
        load(root).0.into_iter().find(|tool| tool.spec.name == name)
    else {
        return Outcome::thrown(
            "tool_error",
            format!("Tool \"{name}\" not found"),
        );
    };
    let schema = serde_json::to_value(&manifest.spec.parameters)
        .unwrap_or_default();
    let arguments = Value::Object(args.clone());
    if !jsonschema::validator_for(&schema)
        .is_ok_and(|schema| schema.is_valid(&arguments))
    {
        return Outcome::thrown(
            "validation_error",
            format!("Validation failed for tool {name}"),
        );
    }
    manifest.policy.net &= net;
    let wall = manifest.policy.limits.wall_secs;
    let env = match Env::private(
        &text(&manifest.dir),
        manifest.policy,
        *sandbox,
    ) {
        Ok(env) => env,
        Err(error) => {
            return Outcome::thrown("tool_error", error.to_string());
        }
    };
    let file = match env.create_temp_file().and_then(|file| {
        env.write_file(&file, &arguments.to_string())?;
        Ok(file)
    }) {
        Ok(file) => file,
        Err(error) => {
            return Outcome::thrown("tool_error", error.message);
        }
    };
    let mut output = String::new();
    let ran = env
        .exec(
            Launch::Program {
                argv: &manifest.exec,
                input: &file,
            },
            Duration::from_secs(wall),
            secrets,
            &mut |part| {
                if output.len() < 64 * 1024 {
                    let mut end =
                        part.len().min(64 * 1024 - output.len());
                    while !part.is_char_boundary(end) {
                        end -= 1;
                    }
                    output.push_str(&part[..end]);
                }
            },
        )
        .await;
    let (exit, diagnostics) = match ran {
        Ok(ran) => (
            ran.exit_code,
            if ran.exit_code == 0 {
                vec![]
            } else {
                vec![diagnostic(
                    DiagnosticSeverity::Error,
                    "exit_code",
                    format!(
                        "Command exited with code {}",
                        ran.exit_code
                    ),
                )]
            },
        ),
        Err(ExecError::Timeout { .. }) => {
            output.push_str(&format!("\n[timed out after {wall}s]"));
            (-1, vec![])
        }
        Err(ExecError::Spawn(words) | ExecError::Spill(words)) => {
            return Outcome::thrown("tool_error", words);
        }
    };
    Outcome {
        text: output,
        error: exit != 0,
        details: Some(Map::from_iter([
            ("exitCode".into(), json!(exit)),
            ("policy".into(), json!(env.policy_hash())),
            ("degraded".into(), json!(false)),
        ])),
        diagnostics,
    }
}
