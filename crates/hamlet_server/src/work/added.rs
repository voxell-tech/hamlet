//! Hand-added tools, their manifests and declared sandbox reach
//! (C17).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy_app::App;
use bevy_ecs::message::Message;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use bevy_log::warn;
use hamlet_os::fs::path::{resolve, text};
use hamlet_protocol::runtime::{
    DiagnosticSeverity, ServerMessage, ToolSpec, ToolSpecReplay,
    Tools,
};
use hamlet_sandbox::{
    Env, ExecError, Launch, Leftovers, LimitError, Policy, Sandbox,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::core::{Catalogue, ModelTools};
use crate::follow_up::InputEffects;
use crate::realm::Realm;
use crate::runtimes::Runtimes;
use crate::server::{AddHandler, InputSender};
use crate::work::tools::{Network, Outcome, diagnostic};

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
    /// Not JSON, or not a manifest's shape; serde's words say where.
    #[error("manifest.json does not fit: {0}")]
    Unfit(serde_json::Error),
    #[error("exec is empty")]
    ExecEmpty,
    #[error("name '{0}' must match [a-zA-Z0-9_-]{{1,64}}")]
    BadName(String),
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

/// The tools added by hand load again whenever their folder changes.
pub(crate) fn plugin(app: &mut App) {
    app.add_handler::<AddedToolsChanged, _>(reload);
}

/// The folder of the tools added by hand changed, and its changes
/// settled.
#[derive(Message)]
pub(crate) struct AddedToolsChanged;

/// The watch on the folder of the tools added by hand, which lasts as
/// long as it is held; none when it could not begin.
#[derive(Resource)]
pub(crate) struct AddedToolsWatch(
    #[expect(dead_code, reason = "held, so the watch goes on")]
    pub  Option<RecommendedWatcher>,
);

/// The time the changes to the tools added by hand settle before they
/// load again.
const SETTLE: Duration = Duration::from_millis(200);

/// Watches the folder of the tools added by hand, sending `inputs`
/// [`AddedToolsChanged`] once what changed in it settles, so they
/// load again while the server runs. The watch ends when the watcher
/// drops.
pub(crate) fn watch(
    folder: &Path,
    inputs: InputSender,
) -> notify::Result<RecommendedWatcher> {
    let (changed, mut changes) = mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(
        move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| !event.kind.is_access()) {
                let _ = changed.send(());
            }
        },
    )?;
    watcher.watch(folder, RecursiveMode::Recursive)?;
    tokio::spawn(async move {
        while changes.recv().await.is_some() {
            // A save is many changes: once they settle, one load
            // takes them all.
            tokio::time::sleep(SETTLE).await;
            while changes.try_recv().is_ok() {}
            if inputs.send(AddedToolsChanged).is_err() {
                return;
            }
        }
    });
    Ok(watcher)
}

/// Loads the tools added by hand again: the catalogue follows them,
/// each ready runtime is told it, and the conversations' tools are
/// narrowed to it once the input is kept.
pub(crate) fn reload(
    realm: Res<Realm>,
    mut tools: ModelTools,
    mut runtimes: Runtimes,
    mut changed: ResMut<InputEffects>,
) {
    let (added, faults) = load(realm.layout().root());
    for fault in faults.iter().filter(|f| !tools.faults.contains(f)) {
        warn!("a tool added by hand did not load: {fault}");
    }
    let catalogue = Catalogue::of(&tools.shells, &added);
    tools.added.0 = added;
    tools.faults.0 = faults;
    // The prompt names the tools that did not load.
    changed.state_changed_outside_change = true;
    if catalogue == tools.catalogue.0 {
        return;
    }
    tools.catalogue.0 = catalogue;
    changed.agents_or_hamlets_changed = true;
    for runtime in runtimes.all() {
        if runtimes.is_ready(runtime) {
            let tools = tools.catalogue.0.clone();
            runtimes.send(runtime, |v, id| {
                ServerMessage::Tools(Tools { id, tools, v })
            });
        }
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
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let reserved = crate::work::tools::reserved_names()
        .chain(
            crate::work::catalogue::Society::names()
                .map(String::from),
        )
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

/// A tool's manifest.json, as a chief writes it. A key it does not
/// name is left alone.
#[derive(Deserialize)]
struct Written {
    name: String,
    #[serde(default)]
    description: String,
    /// The program and its arguments.
    exec: Vec<String>,
    /// The JSON Schema of its arguments; an object of none when left
    /// out.
    #[serde(default = "no_parameters")]
    parameters: Map<String, Value>,
    #[serde(default)]
    sandbox: Reach,
}

/// The reach of a tool's commands beyond its own folder.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Reach {
    /// Paths it may read, from its folder.
    fs_ro: Vec<String>,
    /// Paths it may write, from its folder.
    fs_rw: Vec<String>,
    /// Its environment; a value that is not text is set empty.
    #[serde(deserialize_with = "texts")]
    env: BTreeMap<String, String>,
    net: bool,
    limits: Limits,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Limits {
    wall_secs: Option<u64>,
    cpu_secs: Option<u64>,
}

/// A map of texts, each value that is not text read as empty.
fn texts<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let given = BTreeMap::<String, Value>::deserialize(deserializer)?;
    Ok(given
        .into_iter()
        .map(|(key, value)| {
            (key, value.as_str().unwrap_or_default().to_owned())
        })
        .collect())
}

fn no_parameters() -> Map<String, Value> {
    let mut parameters = Map::new();
    parameters.insert("type".into(), "object".into());
    parameters.insert("properties".into(), Value::Object(Map::new()));
    parameters
}

fn parse(
    data: &str,
    dir: &Path,
    root: &Path,
) -> Result<Manifest, Unloaded> {
    let written = serde_json::from_str::<Written>(data)
        .map_err(Unloaded::Unfit)?;
    let Written {
        name,
        description,
        exec,
        parameters,
        sandbox,
    } = written;
    if exec.is_empty() {
        return Err(Unloaded::ExecEmpty);
    }
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|c| {
            c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
        })
    {
        return Err(Unloaded::BadName(name));
    }
    jsonschema::validator_for(&Value::Object(parameters.clone()))
        .map_err(|error| Unloaded::Uncompiled(Box::new(error)))?;
    let paths = |paths: &[String]| {
        paths
            .iter()
            .map(|path| PathBuf::from(resolve(&text(dir), path)))
            .collect::<Vec<_>>()
    };
    let mut env = hamlet_sandbox::base_env();
    env.extend(sandbox.env);
    let mut policy = Policy {
        ro: paths(&sandbox.fs_ro),
        rw: paths(&sandbox.fs_rw),
        env,
        net: sandbox.net,
        locked: vec![
            root.join("tools"),
            dir.to_owned(),
            root.join(".erasing"),
        ],
        ..Policy::default()
    };
    policy.ro.push(dir.to_owned());
    if let Some(seconds) = sandbox.limits.wall_secs {
        policy.limits.wall_secs = seconds;
    }
    if let Some(seconds) = sandbox.limits.cpu_secs {
        policy.limits.cpu_secs = seconds;
    }
    policy.check_limits()?;
    Ok(Manifest {
        spec: ToolSpec {
            name,
            description,
            parameters: parameters.into_iter().collect(),
            replay: ToolSpecReplay::Unsafe,
            retain: None,
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
    network: Network,
    leftovers: &Leftovers,
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
    manifest.policy.net &= network.is_open();
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
            leftovers,
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
            ("degraded".into(), env.degraded()),
        ])),
        diagnostics,
        images: Vec::new(),
    }
}
