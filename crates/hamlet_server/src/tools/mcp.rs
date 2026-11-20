//! MCP servers, whose tools agents are offered beside the built-in
//! ones (C36, ADR 0017).
//!
//! `mcp/servers.json` in the realm lists the servers (`config`). Each
//! runs under the sandbox for as long as it is listed, its client
//! kept by `session`, and each tool it lists is offered as
//! `mcp__<server>__<tool>`. A call runs once under its key after the
//! gate has ruled on it, and `result` makes what the model is given.

mod config;
mod result;
mod session;
mod sign_in;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use bevy_app::App;
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use hamlet_import::FoundServer;
use hamlet_protocol::client::{
    McpAct, McpFound, McpServerView, McpState, McpToolView, McpView,
};
use hamlet_protocol::runtime::{ToolSpec, ToolSpecReplay};
use hamlet_sandbox::Sandbox;
use hamlet_sandbox::scrub::scrub_text;
use notify::{RecommendedWatcher, RecursiveMode};
use rmcp::model::CallToolRequestParams;
use rmcp::service::ServiceError;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tokio::task::AbortHandle;
use tokio::time::Instant;

use self::config::Server;
pub(crate) use self::session::Report;
use self::session::{Launch, Peer, Status, Tool, Way};
pub(crate) use self::sign_in::SignInStep;
use self::sign_in::{Signing, Step};
use crate::app::follow_up::InputEffects;
use crate::app::input::{AddHandler, InputSender};
use crate::core::{Core, ModelTools};
use crate::realm::Realm;
use crate::realm::layout::Layout;
use crate::runtimes::Runtimes;
use crate::society::oversight::Asked;
use crate::society::oversight::gate::Reach;
use crate::society::rejection::Rejection;
use crate::tools::builtin::{Outcome, Sight};

/// Every MCP tool's name begins so, and no other tool's does.
pub(crate) const PREFIX: &str = "mcp__";

/// The longest a tool's name is that a model takes.
const NAME_LENGTH: usize = 64;

/// The most tools of one server offered.
const MOST_TOOLS: usize = 128;

/// The most tools left out that the prompt names.
const NAMED: usize = 10;

/// The longest description of a tool offered, in characters.
const DESCRIPTION: usize = 4096;

/// The longest schema of a tool's arguments offered, as JSON.
const SCHEMA: usize = 16 * 1024;

/// Takes in what the servers' sessions report, and follows their
/// list.
pub(crate) fn plugin(app: &mut App) {
    app.add_handler::<Report, _>(take_in_reports)
        .add_handler::<ServersChanged, _>(follow_list)
        .add_handler::<SignInStep, _>(take_in_sign_ins);
}

/// The list of the servers changed, and its changes settled.
#[derive(Message)]
pub(crate) struct ServersChanged;

/// The watch on the list of the servers, which lasts as long as it is
/// held; none when it could not begin.
#[derive(Resource)]
pub(crate) struct ServersWatch(
    #[expect(dead_code, reason = "held, so the watch goes on")]
    pub  Option<RecommendedWatcher>,
);

/// Watches the list of the servers in `folder`, sending `inputs`
/// [`ServersChanged`] once a change settles. The watch ends when the
/// watcher drops.
pub(crate) fn watch(
    folder: &Path,
    inputs: InputSender,
) -> notify::Result<RecommendedWatcher> {
    std::fs::create_dir_all(folder).map_err(notify::Error::io)?;
    super::watch(
        folder,
        RecursiveMode::NonRecursive,
        |path| path.file_name() == Some(config::FILE.as_ref()),
        inputs,
        || ServersChanged,
    )
}

/// Starts, restarts and stops the servers as their list now says.
pub(crate) fn follow_list(
    realm: Res<Realm>,
    inputs: Res<InputSender>,
    mut tools: ModelTools,
    mut runtimes: Runtimes,
    mut changed: ResMut<InputEffects>,
) {
    let sandbox = tools.sandbox.0;
    tools.mcp.follow(realm.layout(), sandbox, &inputs);
    super::custom::refresh(&mut tools, &mut runtimes, &mut changed);
}

/// Takes in the reports of the sessions; the catalogue and the
/// prompts follow what they change.
fn take_in_reports(
    mut reports: ResMut<Messages<Report>>,
    mut tools: ModelTools,
    mut runtimes: Runtimes,
    mut changed: ResMut<InputEffects>,
) {
    let before = (tools.mcp.specs(), tools.mcp.faults());
    for report in reports.drain() {
        tools.mcp.record(report);
        // The clients show how each server is.
        changed.state_changed_outside_change = true;
    }
    if (tools.mcp.specs(), tools.mcp.faults()) != before {
        super::custom::refresh(
            &mut tools,
            &mut runtimes,
            &mut changed,
        );
    }
}

/// Takes in how the chief's sign-ins go: a server signed in to starts
/// again on its tokens.
fn take_in_sign_ins(
    mut steps: ResMut<Messages<SignInStep>>,
    realm: Res<Realm>,
    inputs: Res<InputSender>,
    mut tools: ModelTools,
    mut changed: ResMut<InputEffects>,
) {
    let sandbox = tools.sandbox.0;
    for SignInStep { server, step } in steps.drain() {
        changed.state_changed_outside_change = true;
        let mcp = &mut tools.mcp;
        match step {
            Step::Address(address) => {
                if let Some(signing) = mcp.signing.get_mut(&server) {
                    signing.address = Some(address);
                }
            }
            Step::Done => {
                mcp.signing.remove(&server);
                mcp.sign_in_failed.remove(&server);
                mcp.restart(
                    &server,
                    realm.layout(),
                    sandbox,
                    &inputs,
                );
            }
            Step::Failed(why) => {
                mcp.signing.remove(&server);
                mcp.sign_in_failed.insert(server, why);
            }
        }
    }
}

/// The MCP servers the chief lists, running or not.
#[derive(Resource, Default)]
pub(crate) struct Mcp {
    running: BTreeMap<String, Running>,
    /// What in the list does not load.
    unloaded: Vec<String>,
    /// The generation the latest session started with.
    latest: u64,
    /// The servers other programs list, as a chief last found them.
    found: Vec<FoundServer>,
    /// The chief's sign-ins under way, by server.
    signing: BTreeMap<String, Signing>,
    /// Why the last sign-in to a server failed.
    sign_in_failed: BTreeMap<String, String>,
}

/// A listed server, its session, and what it last listed.
struct Running {
    generation: u64,
    server: Server,
    session: AbortHandle,
    standing: Standing,
    /// Its tools as last listed, still offered while it starts again
    /// so that a restart changes no agent's tools.
    tools: Vec<Listed>,
    /// The tools it lists that are left out, and why.
    left_out: Vec<String>,
    /// It listed its tools since it was listed.
    listed: bool,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.session.abort();
    }
}

/// How a server is now.
#[derive(Clone)]
enum Standing {
    Starting,
    Up {
        peer: Peer,
        policy: String,
        degraded: Value,
    },
    Down {
        reason: String,
        again: Instant,
    },
}

/// A tool of a server, under the name a model calls it by.
struct Listed {
    spec: ToolSpec,
    /// Its name on the server.
    tool: String,
    /// The server says it only reads.
    read_only: bool,
}

impl Mcp {
    /// Starts the servers the list of the realm `layout` has that are
    /// not running as it lists them, and stops those it no longer
    /// lists. A list that cannot be read as a whole changes no
    /// server, so that a half-saved file stops nothing.
    fn follow(
        &mut self,
        layout: &Layout,
        sandbox: Sandbox,
        inputs: &InputSender,
    ) {
        let folder = layout.mcp();
        let listed = match config::load(&folder) {
            Ok(listed) => listed,
            Err(unread) => {
                let file = folder.join(config::FILE);
                self.unloaded =
                    vec![format!("{}: {unread}", file.display())];
                return;
            }
        };
        self.unloaded = listed.faults;
        let servers = listed.servers;
        self.running.retain(|name, _| servers.contains_key(name));
        for (name, server) in servers {
            if let Some(running) = self.running.get_mut(&name)
                && running.server.is_same_process(&server)
            {
                running.server = server;
                continue;
            }
            let Some((generation, session)) =
                self.launch(&name, &server, layout, sandbox, inputs)
            else {
                self.running.remove(&name);
                continue;
            };
            // A changed entry starts again, the tools it last listed
            // still offered meanwhile, as when it ends.
            if let Some(running) = self.running.get_mut(&name) {
                running.session.abort();
                running.session = session;
                running.generation = generation;
                running.server = server;
                running.standing = Standing::Starting;
                continue;
            }
            self.running.insert(
                name,
                Running {
                    generation,
                    server,
                    session,
                    standing: Standing::Starting,
                    tools: Vec::new(),
                    left_out: Vec::new(),
                    listed: false,
                },
            );
        }
    }

    /// Stops the server `name` and starts it again at once, its tools
    /// still offered meanwhile; false when it is not listed.
    fn restart(
        &mut self,
        name: &str,
        layout: &Layout,
        sandbox: Sandbox,
        inputs: &InputSender,
    ) -> bool {
        let Some(server) = self
            .running
            .get(name)
            .map(|running| running.server.clone())
        else {
            return false;
        };
        match self.launch(name, &server, layout, sandbox, inputs) {
            Some((generation, session)) => {
                let running = self
                    .running
                    .get_mut(name)
                    .expect("the server is listed");
                running.session.abort();
                running.session = session;
                running.generation = generation;
                running.standing = Standing::Starting;
            }
            None => {
                self.running.remove(name);
            }
        }
        true
    }

    /// Starts a session of the server `name`, under the next
    /// generation; none when its policy does not hold, which is named
    /// in the prompt.
    fn launch(
        &mut self,
        name: &str,
        server: &Server,
        layout: &Layout,
        sandbox: Sandbox,
        inputs: &InputSender,
    ) -> Option<(u64, AbortHandle)> {
        let way = match &server.url {
            Some(url) => Way::Remote {
                url: url.clone(),
                headers: server.headers.clone(),
                signed_in: sign_in::tokens(layout, name),
            },
            None => {
                let home = layout.mcp().join(name);
                let policy = match server.policy(layout, &home) {
                    Ok(policy) => policy,
                    Err(error) => {
                        self.unloaded.push(format!(
                            "MCP server {name}: {error}"
                        ));
                        return None;
                    }
                };
                Way::Program {
                    argv: server.command.clone(),
                    policy,
                    sandbox,
                    home,
                }
            }
        };
        self.latest += 1;
        let session = session::start(
            Launch {
                name: name.to_owned(),
                generation: self.latest,
                way,
                secrets: server.secrets(),
            },
            inputs.clone(),
        );
        Some((self.latest, session))
    }

    /// Keeps what the session of a server reported, unless a later
    /// session replaced it.
    fn record(&mut self, report: Report) {
        let Some(running) = self.running.get_mut(&report.server)
        else {
            return;
        };
        if running.generation != report.generation {
            return;
        }
        running.standing = match report.status {
            Status::Up {
                tools,
                peer,
                policy,
                degraded,
            } => {
                (running.tools, running.left_out) =
                    listed(&report.server, tools);
                running.listed = true;
                Standing::Up {
                    peer,
                    policy,
                    degraded,
                }
            }
            Status::Down { reason, again } => Standing::Down {
                reason,
                again: Instant::now() + again,
            },
        };
    }

    /// The servers, as the clients show them.
    pub(crate) fn view(&self) -> McpView {
        let now = Instant::now();
        let servers = self
            .running
            .iter()
            .map(|(name, running)| McpServerView {
                name: name.clone(),
                entry: running.server.entry(),
                state: match &running.standing {
                    Standing::Starting => McpState::Starting,
                    Standing::Up { .. } => McpState::Up,
                    Standing::Down { reason, again } => {
                        McpState::Down {
                            reason: reason.clone(),
                            again: crate::app::clock::now()
                                + i64::try_from(
                                    again
                                        .saturating_duration_since(
                                            now,
                                        )
                                        .as_millis(),
                                )
                                .unwrap_or(i64::MAX / 2),
                        }
                    }
                },
                tools: running
                    .tools
                    .iter()
                    .map(|listed| McpToolView {
                        name: listed.spec.name.clone(),
                        tool: listed.tool.clone(),
                        description: listed.spec.description.clone(),
                        acts: running
                            .server
                            .acts(&listed.tool, listed.read_only),
                    })
                    .collect(),
                sign_in: self
                    .signing
                    .get(name)
                    .and_then(|signing| signing.address.clone()),
            })
            .collect();
        let found = self
            .found
            .iter()
            .filter(|found| !self.running.contains_key(&found.name))
            .map(|found| {
                let (entry, unrunnable) = match &found.entry {
                    Ok(entry) => {
                        match config::parse(
                            &found.name,
                            entry.clone(),
                        ) {
                            Ok(server) => {
                                (Some(server.entry()), None)
                            }
                            Err(unloaded) => {
                                (None, Some(unloaded.to_string()))
                            }
                        }
                    }
                    Err(unrunnable) => {
                        (None, Some(unrunnable.to_string()))
                    }
                };
                McpFound {
                    source: found.source.name().into(),
                    name: found.name.clone(),
                    entry,
                    unrunnable,
                }
            })
            .collect();
        McpView {
            servers,
            faults: self.faults(),
            found,
        }
    }

    /// The tools offered.
    pub(crate) fn specs(&self) -> Vec<ToolSpec> {
        self.running
            .values()
            .flat_map(|running| &running.tools)
            .map(|listed| listed.spec.clone())
            .collect()
    }

    /// What in the list failed, for the prompts of the agents that
    /// would have had its tools (C15). A server that stops after it
    /// listed its tools is not named, so that a restart changes no
    /// prompt; a call to it says why it failed.
    pub(crate) fn faults(&self) -> Vec<String> {
        let mut faults = self.unloaded.clone();
        for (name, running) in &self.running {
            if let Standing::Down { reason, .. } = &running.standing
                && !running.listed
            {
                faults.push(format!(
                    "MCP server {name} did not start, so its tools \
                     are missing: {reason}"
                ));
            }
            if !running.left_out.is_empty() {
                faults.push(format!(
                    "MCP server {name}: {}",
                    running.left_out.join("; ")
                ));
            }
        }
        for (name, why) in &self.sign_in_failed {
            faults.push(format!(
                "MCP server {name}: the sign-in failed: {why}"
            ));
        }
        faults
    }

    /// Whether the list has a server named `name` that loads.
    pub(crate) fn is_listed(&self, name: &str) -> bool {
        self.running.contains_key(name)
    }

    /// What the tool `name` reaches, as the gate weighs it; none when
    /// no MCP server offers it. A server's network is the chief's,
    /// given when it was listed, so it holds no call.
    pub(crate) fn reach_of(&self, name: &str) -> Option<Reach> {
        self.owner(name).map(|(_, running, listed)| Reach {
            net: false,
            acts: running.server.acts(&listed.tool, listed.read_only),
        })
    }

    /// How to call the tool `name`; none when no MCP server offers
    /// it.
    pub(crate) fn route(&self, name: &str) -> Option<Route> {
        let (server, running, listed) = self.owner(name)?;
        Some(Route {
            server: server.to_owned(),
            name: name.to_owned(),
            tool: listed.tool.clone(),
            parameters: serde_json::to_value(&listed.spec.parameters)
                .unwrap_or_default(),
            timeout: running.server.timeout,
            secrets: running.server.secrets(),
            standing: running.standing.clone(),
        })
    }

    /// The server that offers the tool `name`, and the tool.
    fn owner(&self, name: &str) -> Option<(&str, &Running, &Listed)> {
        self.running.iter().find_map(|(server, running)| {
            running
                .tools
                .iter()
                .find(|listed| listed.spec.name == name)
                .map(|listed| (server.as_str(), running, listed))
        })
    }
}

impl Core<'_, '_> {
    /// A chief's change to the list of MCP servers, made at once: the
    /// servers and the catalogue follow the list without waiting for
    /// its watch.
    pub(crate) fn mcp(
        &mut self,
        client: u64,
        request: hamlet_protocol::client::Mcp,
    ) -> anyhow::Result<()> {
        let answer = self.mcp_act(request.act);
        self.follow_mcp();
        self.answer(
            client,
            crate::clients::check::reply(&request.id, answer),
        )
    }

    /// Adds the MCP server an agent asked for, `asked`, once a chief
    /// allowed it: each value of its environment empty, for the chief
    /// to give.
    pub(crate) fn add_asked(
        &mut self,
        asked: &Asked,
    ) -> Result<(), Rejection> {
        if self.tools.mcp.running.contains_key(&asked.name) {
            return Err(Rejection::BadMcp {
                why: format!(
                    "MCP server {} is listed already.",
                    asked.name
                ),
            });
        }
        let folder = self.realm.layout().mcp();
        config::put(&folder, &asked.name, &asked.entry).map_err(
            |unchanged| Rejection::BadMcp {
                why: unchanged.to_string(),
            },
        )?;
        self.follow_mcp();
        Ok(())
    }

    /// The servers and the catalogue as the list now says.
    fn follow_mcp(&mut self) {
        let layout = self.realm.layout().clone();
        let sandbox = self.tools.sandbox.0;
        self.tools.mcp.follow(&layout, sandbox, &self.inputs);
        super::custom::refresh(
            &mut self.tools,
            &mut self.runtimes,
            &mut self.changed,
        );
    }

    fn mcp_act(&mut self, act: McpAct) -> Result<String, Rejection> {
        let folder = self.realm.layout().mcp();
        let refused =
            |unchanged: config::Unchanged| Rejection::BadMcp {
                why: unchanged.to_string(),
            };
        Ok(match act {
            McpAct::Put { name, entry } => {
                config::put(&folder, &name, &entry)
                    .map_err(refused)?;
                format!("MCP server {name} is saved.")
            }
            McpAct::Remove { name } => {
                config::remove(&folder, &name).map_err(refused)?;
                // Its sign-in goes with it.
                self.tools.mcp.signing.remove(&name);
                self.tools.mcp.sign_in_failed.remove(&name);
                let tokens =
                    sign_in::tokens(self.realm.layout(), &name);
                if let Err(error) = std::fs::remove_file(&tokens)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    return Err(Rejection::BadMcp {
                        why: format!(
                            "MCP server {name} is removed, but its \
                             sign-in is not: {error}"
                        ),
                    });
                }
                format!("MCP server {name} is removed.")
            }
            McpAct::SignIn {
                name,
                pasted: Some(address),
            } => {
                let Some(signing) = self.tools.mcp.signing.get(&name)
                else {
                    return Err(Rejection::BadMcp {
                        why: format!(
                            "No sign-in to MCP server {name} waits."
                        ),
                    });
                };
                signing.paste(address);
                format!(
                    "Signing in to MCP server {name} with that \
                     address."
                )
            }
            McpAct::SignIn { name, pasted: None } => {
                let url = self
                    .tools
                    .mcp
                    .running
                    .get(&name)
                    .ok_or_else(|| {
                        refused(config::Unchanged::NoServer(
                            name.clone(),
                        ))
                    })?
                    .server
                    .url
                    .clone()
                    .ok_or_else(|| Rejection::BadMcp {
                        why: format!(
                            "MCP server {name} is a program, and \
                             only a server reached by a url signs \
                             in."
                        ),
                    })?;
                let file =
                    sign_in::tokens(self.realm.layout(), &name);
                let signing = sign_in::begin(
                    name.clone(),
                    url,
                    file,
                    self.inputs.clone(),
                );
                self.tools.mcp.sign_in_failed.remove(&name);
                self.tools.mcp.signing.insert(name.clone(), signing);
                format!(
                    "Signing in to MCP server {name}: its address \
                     shows once its server gives it."
                )
            }
            McpAct::Restart { name } => {
                let layout = self.realm.layout().clone();
                let sandbox = self.tools.sandbox.0;
                let inputs = &self.inputs;
                if !self
                    .tools
                    .mcp
                    .restart(&name, &layout, sandbox, inputs)
                {
                    return Err(refused(
                        config::Unchanged::NoServer(name),
                    ));
                }
                format!("MCP server {name} starts again.")
            }
            McpAct::Find => {
                self.tools.mcp.found = hamlet_import::mcp_servers();
                let found = self
                    .tools
                    .mcp
                    .found
                    .iter()
                    .filter(|found| {
                        !self
                            .tools
                            .mcp
                            .running
                            .contains_key(&found.name)
                    })
                    .map(|found| {
                        let source = found.source.name();
                        match &found.entry {
                            Ok(_) => {
                                format!("- {} ({source})", found.name)
                            }
                            Err(why) => {
                                format!(
                                    "- {} ({source}): {why}",
                                    found.name
                                )
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                if found.is_empty() {
                    "No other program lists an MCP server this list \
                     does not."
                        .into()
                } else {
                    format!(
                        "Other programs list these MCP servers:\n{}",
                        found.join("\n")
                    )
                }
            }
            McpAct::Import { names } => {
                let entries = self
                    .tools
                    .mcp
                    .found
                    .iter()
                    .filter(|found| {
                        names.is_empty()
                            || names.contains(&found.name)
                    })
                    .filter_map(|found| {
                        Some((
                            found.name.clone(),
                            found.entry.clone().ok()?,
                        ))
                    })
                    .collect::<Vec<_>>();
                let added =
                    config::add(&folder, entries).map_err(refused)?;
                let left = names
                    .iter()
                    .filter(|name| !added.contains(name))
                    .cloned()
                    .collect::<Vec<_>>();
                let mut words = if added.is_empty() {
                    "No MCP server was added.".to_owned()
                } else {
                    format!("Added {}.", added.join(", "))
                };
                if !left.is_empty() {
                    words.push_str(&format!(
                        " Not added, as they are listed already, \
                         cannot run here or were not found: {}.",
                        left.join(", ")
                    ));
                }
                words
            }
        })
    }
}

/// The server an agent asks for, as the list would hold it: its
/// environment's values empty, for a chief to give. Why it would not
/// load, when it would not.
pub(crate) fn asked(
    name: String,
    command: Vec<String>,
    env: Vec<String>,
    net: bool,
    fs_ro: Vec<String>,
    fs_rw: Vec<String>,
) -> Result<Asked, String> {
    let entry = hamlet_protocol::client::McpEntry {
        command,
        env: env
            .into_iter()
            .map(|key| (key, Some(String::new())))
            .collect(),
        net,
        fs_ro,
        fs_rw,
        ..hamlet_protocol::client::McpEntry::default()
    };
    config::check(&name, &entry).map_err(|unloaded| {
        format!("MCP server {name}: {unloaded}")
    })?;
    Ok(Asked { name, entry })
}

/// The tools of `server` as models are offered them, and words for
/// those left out.
fn listed(
    server: &str,
    tools: Vec<Tool>,
) -> (Vec<Listed>, Vec<String>) {
    let mut names = BTreeSet::new();
    let mut offered = Vec::new();
    let mut uncompiled = BTreeSet::new();
    let mut more = 0;
    for tool in tools {
        if offered.len() == MOST_TOOLS {
            more += 1;
            continue;
        }
        // The prompt names a tool left out by the name a model would
        // call it by, never by the server's own words.
        let name = tool_name(server, &tool.name);
        // Two tools that come to one name leave the later out.
        if !names.insert(name.clone()) {
            uncompiled.insert(name);
            continue;
        }
        let parameters = parameters(&tool.input_schema);
        let fits = serde_json::to_string(&parameters)
            .is_ok_and(|text| text.len() <= SCHEMA);
        if !fits
            || jsonschema::validator_for(&Value::Object(
                parameters.clone(),
            ))
            .is_err()
        {
            uncompiled.insert(name);
            continue;
        }
        let description = tool
            .description
            .as_deref()
            .or(tool.title.as_deref())
            .unwrap_or_default()
            .chars()
            .take(DESCRIPTION)
            .collect();
        offered.push(Listed {
            spec: ToolSpec {
                name,
                description,
                parameters: parameters.into_iter().collect(),
                // A server's word that a tool only reads is a hint
                // from a program, so every call runs once at most.
                replay: ToolSpecReplay::Unsafe,
                retain: None,
            },
            read_only: tool
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.read_only_hint)
                == Some(true),
            tool: tool.name.into_owned(),
        });
    }
    let mut left_out = Vec::new();
    if !uncompiled.is_empty() {
        // A server can list any number of them: the prompt names a
        // few.
        let mut named = uncompiled
            .iter()
            .take(NAMED)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if uncompiled.len() > NAMED {
            named.push_str(&format!(
                " and {} more",
                uncompiled.len() - NAMED
            ));
        }
        left_out.push(format!(
            "the tools {named} are left out, as their arguments' \
             schema does not compile or is too long, or their name \
             is another's"
        ));
    }
    if more > 0 {
        left_out.push(format!(
            "it lists {more} tools past {MOST_TOOLS}, which are \
             left out"
        ));
    }
    (offered, left_out)
}

/// A tool's schema of its arguments as a model is offered it: an
/// object, with properties, even when the server says less.
fn parameters(schema: &Map<String, Value>) -> Map<String, Value> {
    let mut schema = schema.clone();
    schema.entry("type").or_insert_with(|| "object".into());
    if schema.get("type") == Some(&Value::String("object".into())) {
        schema
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()));
    }
    schema
}

/// The name a model calls `tool` of `server` by: letters, digits, `_`
/// and `-`, at most 64 long, kept apart from others by a hash when
/// shortened or changed.
fn tool_name(server: &str, tool: &str) -> String {
    let prefix = format!("{PREFIX}{server}__");
    let clean = tool
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if clean == tool && prefix.len() + clean.len() <= NAME_LENGTH {
        return prefix + &clean;
    }
    let hash = Sha256::digest(tool.as_bytes())[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let room = NAME_LENGTH - prefix.len() - hash.len() - 1;
    let kept = clean.chars().take(room).collect::<String>();
    format!("{prefix}{kept}_{hash}")
}

/// How to call one tool of a listed server.
pub(crate) struct Route {
    server: String,
    /// The tool's name as models call it.
    name: String,
    /// Its name on the server.
    tool: String,
    /// The JSON Schema of its arguments.
    parameters: Value,
    timeout: Duration,
    secrets: Vec<String>,
    standing: Standing,
}

impl Route {
    /// Calls the tool with `arguments`, and gives its result without
    /// `secrets` or the server's own, with the images the model of
    /// `sight` takes.
    pub(crate) async fn run(
        self,
        secrets: &[String],
        arguments: Map<String, Value>,
        sight: &Sight,
    ) -> Outcome {
        let secrets = secrets
            .iter()
            .chain(&self.secrets)
            .cloned()
            .collect::<Vec<_>>();
        let server = &self.server;
        let thrown = |code: &str, words: String| {
            Outcome::thrown(code, scrub_text(&secrets, &words))
        };
        let given = Value::Object(arguments);
        let invalid = jsonschema::validator_for(&self.parameters)
            .ok()
            .and_then(|schema| {
                schema
                    .iter_errors(&given)
                    .next()
                    .map(|e| e.to_string())
            });
        if let Some(error) = invalid {
            return thrown(
                "validation_error",
                format!(
                    "Validation failed for tool {}: {error}",
                    self.name
                ),
            );
        }
        let (peer, policy, degraded) = match self.standing {
            Standing::Up {
                peer,
                policy,
                degraded,
            } => (peer, policy, degraded),
            Standing::Starting => {
                return thrown(
                    "tool_error",
                    format!("The MCP server {server} is starting."),
                );
            }
            Standing::Down { reason, again } => {
                let wait =
                    again.saturating_duration_since(Instant::now());
                let when = if wait.is_zero() {
                    "It is starting again.".to_owned()
                } else {
                    format!("It starts again in {}.", seconds(wait))
                };
                return thrown(
                    "tool_error",
                    format!(
                        "The MCP server {server} stopped: {reason}. \
                         {when}"
                    ),
                );
            }
        };
        let Value::Object(arguments) = given else {
            unreachable!("the arguments were an object");
        };
        let call = peer.call_tool(
            CallToolRequestParams::new(self.tool.clone())
                .with_arguments(arguments),
        );
        // A call that outlasts its time, or whose turn is stopped, is
        // dropped, which tells the server it was cancelled.
        let mut outcome =
            match tokio::time::timeout(self.timeout, call).await {
                Ok(Ok(result)) => {
                    result::outcome(result, &secrets, sight).await
                }
                Ok(Err(error)) => {
                    thrown("tool_error", failure(server, error))
                }
                Err(_) => thrown(
                    "tool_error",
                    format!(
                        "The MCP server {server} did not answer \
                         within {} seconds, and the call was \
                         cancelled.",
                        self.timeout.as_secs()
                    ),
                ),
            };
        outcome.details.get_or_insert_with(Map::new).extend(
            result::origin(server, &self.tool, &policy, &degraded),
        );
        outcome
    }
}

/// `time` in whole seconds, rounded up, as words.
fn seconds(time: Duration) -> String {
    match time.as_secs() + u64::from(time.subsec_nanos() > 0) {
        1 => "1 second".to_owned(),
        seconds => format!("{seconds} seconds"),
    }
}

/// The words of a call to `server` that failed with `error`.
fn failure(server: &str, error: ServiceError) -> String {
    match error {
        ServiceError::McpError(error) => format!(
            "The MCP server {server} refused the call: {}",
            error.message
        ),
        ServiceError::TransportClosed
        | ServiceError::TransportSend(_) => {
            format!(
                "The MCP server {server} stopped while the call ran."
            )
        }
        ServiceError::UnexpectedResponse => format!(
            "The MCP server {server} asked for an answer or began a \
             task, and Hamlet answers a server nothing."
        ),
        error => format!(
            "The call to the MCP server {server} failed: {error}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_name_fits_what_models_take_and_stays_apart() {
        assert_eq!(
            tool_name("github", "create_issue"),
            "mcp__github__create_issue"
        );
        let spaced = tool_name("github", "create issue");
        let dotted = tool_name("github", "create.issue");
        assert!(spaced.starts_with("mcp__github__create_issue_"));
        assert_ne!(spaced, dotted);
        let long = tool_name("github", &"x".repeat(100));
        assert_eq!(long.len(), NAME_LENGTH);
        assert!(long.bytes().all(|c| {
            c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
        }));
    }
}
