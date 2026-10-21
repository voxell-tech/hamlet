//! Hand-added tools, their manifests and declared sandbox reach
//! (C17).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_runtime_protocol::{
    DiagnosticSeverity, ToolSpec, ToolSpecReplay,
};
use serde_json::{Map, Value, json};

use crate::sandbox::path::{resolve, text};
use crate::sandbox::{Env, ExecError, Policy, Sandbox};
use crate::tools::{Outcome, diagnostic};

/// A loaded manifest and the program its tool runs.
pub(crate) struct Manifest {
    pub spec: ToolSpec,
    dir: PathBuf,
    exec: Vec<String>,
    policy: Policy,
}

impl Manifest {
    pub fn net(&self) -> bool {
        self.policy.net
    }
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
    entries.sort_by_key(|entry| entry.file_name());
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
                .map_err(|_| "not a folder".to_owned())?;
            if !dir.is_dir() {
                return Err("not a folder".into());
            }
            let data =
                std::fs::read_to_string(dir.join("manifest.json"))
                    .map_err(|error| error.to_string())?;
            let manifest = parse(&data, &dir, root)?;
            let name = &manifest.spec.name;
            if reserved.contains(name) {
                return Err(format!("name '{name}' is reserved"));
            }
            if !names.insert(name.clone()) {
                return Err(format!(
                    "name '{name}' is taken by another tool"
                ));
            }
            Ok(manifest)
        })();
        match loaded {
            Ok(manifest) => tools.push(manifest),
            Err(words) => {
                faults.push(format!("{}: {words}", path.display()))
            }
        }
    }
    (tools, faults)
}

fn parse(
    data: &str,
    dir: &Path,
    root: &Path,
) -> Result<Manifest, String> {
    let value = serde_json::from_str::<Value>(data)
        .map_err(|_| "manifest.json is not valid JSON")?;
    let name = value["name"].as_str().ok_or("missing name")?;
    let exec = value["exec"]
        .as_array()
        .ok_or("exec must be an array")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if exec.is_empty() {
        return Err("exec is empty".into());
    }
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|c| {
            c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
        })
    {
        return Err(format!(
            "name '{name}' must match [a-zA-Z0-9_-]{{1,64}}"
        ));
    }
    let parameters = value
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({"type":"object","properties":{}}));
    let parameters = parameters.as_object().ok_or(
        "parameters do not compile: parameters must be a JSON \
         Schema object",
    )?;
    jsonschema::validator_for(&Value::Object(parameters.clone()))
        .map_err(|error| {
            format!("parameters do not compile: {error}")
        })?;
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
        Err(words) => return Outcome::thrown("tool_error", words),
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
    let quote =
        |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let command = format!(
        "exec {} < {}",
        manifest
            .exec
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(" "),
        quote(&file)
    );
    let mut output = String::new();
    let ran = env
        .exec(
            &command,
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
