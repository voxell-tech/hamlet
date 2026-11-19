//! The actors' host on the realm folder: the workspace, the profiles,
//! the library and the realm in this process, and the agents in a
//! Node child.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_actor_error::ActorError;
use hamlet_actors::{library, profile, realm, workspace};
use hamlet_host::child::{self, ChildOptions, LinkState, Supervisor};
use hamlet_host::route::{CallerKind, Routes};
use hamlet_host::{Host, HostOptions};
use hamlet_messages::address::{self, Party};
use hamlet_messages::profile::ProfileInput;
use hamlet_messages::realm::ProfileId;
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::world::{ReachSeen, Seen, Status};
use hamlet_messages::workspace::AgentNumber;
use hamlet_sandbox::Sandbox;

/// The Node host of the agents, in the checkout this server was
/// built from.
const ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../hosts/node/src/agent/demo-host.ts"
);

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

/// The profile the realm gives the Hand.
const HAND_PROFILE: ProfileId = ProfileId(1);

/// Opens the folder `root`, with the Node host's model answering
/// from the script `faux` when given. The program is `HAMLET_NODE`,
/// or `node`. Shut the supervisor down first, then the host.
pub(crate) fn open(
    root: &Path,
    sandbox: Sandbox,
    faux: Option<&Path>,
) -> Result<(Host, Supervisor)> {
    let node = std::env::var("HAMLET_NODE")
        .unwrap_or_else(|_| "node".into());
    // Node before 24.3 warns at each start that it strips types.
    let mut argv = vec![
        node,
        "--disable-warning=ExperimentalWarning".to_owned(),
        ENTRY.to_owned(),
    ];
    if let Some(faux) = faux {
        let script =
            std::fs::canonicalize(faux).with_context(|| {
                format!(
                    "the faux script {} is not there",
                    faux.display()
                )
            })?;
        let script = script
            .to_str()
            .context("the faux script's path is not UTF-8")?;
        argv.extend(["--faux".to_owned(), script.to_owned()]);
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
    .with_value(Arc::new(sandbox));
    let child = ChildOptions {
        argv,
        kinds: vec![address::Agent::KIND.to_owned()],
    };
    child::open(root, factories, options, child)
}

/// Starts the workspace, the Hand's profile, then the realm, which
/// answers a client from then on.
pub(crate) async fn start_realm(host: &Host) -> Result<()> {
    host.get_or_start(&address::Workspace.address()?).await?;
    make_hand_profile(host).await?;
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

/// Makes the Hand's profile from the Hand's character, unless it was
/// made before. The realm's first start only starts that profile,
/// and a profile is made with its input.
async fn make_hand_profile(host: &Host) -> Result<()> {
    let input = Some(ProfileInput::Character {
        text: crate::realm::HAND.to_owned(),
    });
    let mut bytes = Vec::new();
    ciborium::into_writer(&input, &mut bytes)?;
    let at = address::Profile(HAND_PROFILE).address()?;
    match host.create(&at, bytes).await {
        Err(error)
            if error
                .chain()
                .filter_map(|cause| {
                    cause.downcast_ref::<ActorError>()
                })
                .any(|found| {
                    found.group() == "actor"
                        && found.code() == "duplicate_key"
                }) =>
        {
            Ok(())
        }
        made => made,
    }
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

/// What each kind may reach: a client the realm and the agents only.
fn routes() -> Routes {
    let kind = |party: &str| CallerKind::Actor(party.to_owned());
    let peers = [address::Realm::KIND, address::Agent::KIND];
    Routes::only(&[
        (CallerKind::Client, &peers),
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
        (kind(address::Profile::KIND), &peers),
        (kind(address::Library::KIND), &peers),
    ])
}
