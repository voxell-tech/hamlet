//! The actors' host on the realm folder: the workspace, the profiles,
//! the library and the realm in this process, and the agents in a
//! Node child.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_actors::{library, profile, realm, workspace};
use hamlet_host::child::{self, ChildOptions, LinkState, Supervisor};
use hamlet_host::route::{CallerKind, Routes};
use hamlet_host::{Host, HostOptions};
use hamlet_messages::address::{self, Party};
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::world::{ReachSeen, Seen, Status};
use hamlet_messages::workspace::AgentNumber;
use hamlet_sandbox::Sandbox;

/// The Node host of the agents, in the checkout this server was
/// built from.
const CHECKOUT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../hosts/node/src/main.ts"
);

/// Names the Node host's `src/main.ts` to start in place of the one
/// beside the server or in the checkout.
const NODE_HOST: &str = "HAMLET_NODE_HOST";

/// Set to `1`, the Node host lists two loopback providers to sign in
/// to, for tests.
const SIGN_IN: &str = "HAMLET_DEMO_SIGN_IN";

/// The longest the Node host may take to be ready.
const READY_WITHIN: Duration = Duration::from_secs(20);

/// The longest the Hand may take to be live once the Node host is
/// ready.
const LIVE_WITHIN: Duration = Duration::from_secs(20);

/// The longest a read of the world may take.
const READ_WITHIN: Duration = Duration::from_secs(10);

pub(crate) const HAND: AgentNumber = AgentNumber(1);

/// Opens the folder `root`, with the Node host's model answering
/// from the script `faux` when given. Shut the supervisor down first,
/// then the host.
pub(crate) fn open(
    root: &Path,
    sandbox: Sandbox,
    faux: Option<&Path>,
) -> Result<(Host, Supervisor)> {
    let (node, entry) = node_host()?;
    // Node before 24.3 warns at each start that it strips types.
    let mut argv = vec![
        text(&node)?,
        "--disable-warning=ExperimentalWarning".to_owned(),
        text(&entry)?,
    ];
    if let Some(faux) = faux {
        let script =
            std::fs::canonicalize(faux).with_context(|| {
                format!(
                    "the faux script {} is not there",
                    faux.display()
                )
            })?;
        argv.extend(["--faux".to_owned(), text(&script)?]);
    }
    if std::env::var(SIGN_IN).is_ok_and(|set| set == "1") {
        argv.push("--sign-in".to_owned());
    }
    let factories = HashMap::from([
        (address::Workspace::KIND.to_owned(), workspace::factory()),
        (address::Profile::KIND.to_owned(), profile::factory()),
        (address::Library::KIND.to_owned(), library::factory()),
        (address::Realm::KIND.to_owned(), realm::factory()),
    ]);
    let options = HostOptions {
        routes: routes(),
        ..HostOptions::default()
    }
    .with_value(Arc::new(sandbox))
    .with_value(Arc::new(realm::Options {
        chief: crate::serve::chief_name(),
        faux: faux.is_some(),
        ..realm::Options::default()
    }));
    let child = ChildOptions {
        argv,
        kinds: vec![address::Agent::KIND.to_owned()],
    };
    child::open(root, factories, options, child)
}

/// The Node program and the Node host's entry the environment gives:
/// `HAMLET_NODE` and [`NODE_HOST`], else the Node a release ships
/// beside the server, or Node on the path, and the entry beside the
/// server or in the checkout it was built from.
fn node_host() -> Result<(PathBuf, PathBuf)> {
    let set = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .context("Cannot locate the server's directory")?;
    let entry = if let Some(path) = set(NODE_HOST) {
        let path = PathBuf::from(path);
        ensure!(
            path.is_file(),
            "{NODE_HOST} names {}, which is not a file.",
            path.display()
        );
        path
    } else {
        [
            directory.join("node-host/src/main.ts"),
            directory.join("../Resources/node-host/src/main.ts"),
            PathBuf::from(CHECKOUT),
        ]
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| {
            format!(
                "No Node host beside the server in {} or in the \
                 checkout it was built from. Set {NODE_HOST} to its \
                 src/main.ts.",
                directory.display()
            )
        })?
    };
    let shipped = directory
        .join(format!("node{}", std::env::consts::EXE_SUFFIX));
    let node = match set("HAMLET_NODE") {
        Some(node) => node.into(),
        None if shipped.is_file() => shipped,
        None => "node".into(),
    };
    Ok((node, entry))
}

fn text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .with_context(|| format!("{} is not UTF-8", path.display()))
}

pub(crate) async fn start_realm(host: &Host) -> Result<()> {
    host.get_or_start(&address::Workspace.address()?).await?;
    host.get_or_start(&address::Library.address()?).await?;
    host.get_or_start(&address::Realm.address()?).await?;
    Ok(())
}

/// Once the Node host is ready and the realm shows the Hand live,
/// starts every actor that was running.
pub(crate) async fn start_agents(
    host: &Host,
    supervisor: &Supervisor,
) -> Result<()> {
    let mut link = supervisor.state();
    tokio::time::timeout(
        READY_WITHIN,
        link.wait_for(|now| *now == LinkState::Ready),
    )
    .await
    .map_err(|_| {
        anyhow!(
            "the Node host was not ready within {} s",
            READY_WITHIN.as_secs()
        )
    })??;
    ensure!(
        hand_live(host).await?,
        "the Hand was not live within {} s",
        LIVE_WITHIN.as_secs()
    );
    host.boot().await?;
    Ok(())
}

/// Whether the realm shows the Hand live, with its reach applied,
/// within [`LIVE_WITHIN`].
async fn hand_live(host: &Host) -> Result<bool> {
    let end = tokio::time::Instant::now() + LIVE_WITHIN;
    let realm = address::Realm.address()?;
    loop {
        let answer = host
            .call_for_client(
                &realm,
                ReadWorld::NAME,
                encode_positional(&ReadWorld)?,
                READ_WITHIN,
            )
            .await?;
        let seen: Seen = ciborium::from_reader(answer.as_slice())?;
        if seen.agents.iter().any(|agent| {
            agent.agent == HAND
                && matches!(agent.status, Status::Live)
                && matches!(agent.reach, ReachSeen::Applied { .. })
        }) {
            return Ok(true);
        }
        if tokio::time::Instant::now() >= end {
            return Ok(false);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The parties each kind may reach: a client the realm, the agents,
/// and the profiles and the library to read them.
fn routes() -> Routes {
    let kind = |party: &str| CallerKind::Actor(party.to_owned());
    let peers = [address::Realm::KIND, address::Agent::KIND];
    Routes::only(&[
        (
            CallerKind::Client,
            &[
                address::Realm::KIND,
                address::Agent::KIND,
                address::Profile::KIND,
                address::Library::KIND,
            ],
        ),
        (
            kind(address::Realm::KIND),
            &[
                address::Agent::KIND,
                address::Workspace::KIND,
                address::Profile::KIND,
                address::Library::KIND,
            ],
        ),
        (
            kind(address::Agent::KIND),
            &[
                address::Realm::KIND,
                address::Workspace::KIND,
                address::Profile::KIND,
                address::Library::KIND,
            ],
        ),
        (kind(address::Workspace::KIND), &peers),
        (kind(address::Profile::KIND), &[address::Agent::KIND]),
        (kind(address::Library::KIND), &[address::Agent::KIND]),
    ])
}
