//! The link between the Rust host and its Node child, over the
//! child's pipe: the frames each side writes, who each side speaks
//! for, and each host's port to the other.

mod frame;
mod peer;
mod port;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use anyhow::{Context, Result, bail};
pub use frame::{
    Ask, Broken, FRAME_LIMIT, Frame, MAGIC, WireError, read, write,
};
pub(crate) use frame::{AtBreak, Writer};
use hamlet_actor_core::ActorFactory;
use hamlet_protocol::Version;
pub(crate) use peer::{Role, Side, Vetted};
pub use port::LinkState;
pub(crate) use port::{Port, Taken};
use tokio::sync::broadcast::error::RecvError;

use crate::local::Inbound;
use crate::route::Routes;
use crate::{Host, HostOptions, child};

/// The link protocol's version. A host refuses a peer whose version
/// is not compatible with its own by semver's caret.
pub const PROTOCOL: Version = Version::new(0, 1, 0);

/// The Node host's name, so its folder lock is `node.lock`.
const NODE: &str = "node";

/// The fingerprint file, which `cargo test -p hamlet_host` rewrites
/// when the frames' shapes change.
const RECORDED: &str = include_str!("../link.fingerprint");

/// A sha256 of the shapes of the frames, as `link.fingerprint`
/// records it. Two builds link only when theirs are equal, so a Node
/// host built from older source than the Rust host is refused,
/// although every version stays 0.1.0 until Hamlet's first release.
pub fn fingerprint() -> &'static str {
    RECORDED
        .lines()
        .find_map(|line| line.strip_prefix("sha256 "))
        .unwrap_or_default()
}

pub(crate) fn kinds(
    factories: &HashMap<String, ActorFactory>,
) -> Vec<String> {
    let mut kinds = factories.keys().cloned().collect::<Vec<_>>();
    kinds.sort_unstable();
    kinds
}

/// The Rust host's answer to a Node host's hello: a
/// [`Frame::Welcome`] or a [`Frame::Refused`].
pub(crate) fn answer(
    protocol: &Version,
    theirs: &str,
    kinds: &[String],
    port: &Port,
    ours: &[String],
    routes: &Routes,
) -> Frame {
    let held = kinds.iter().collect::<BTreeSet<_>>();
    let expected =
        port.theirs().kinds.iter().collect::<BTreeSet<_>>();
    let reason = if !protocol.is_compatible(&PROTOCOL) {
        format!(
            "the link protocol {protocol} is not compatible with \
             the Rust host's {PROTOCOL}"
        )
    } else if theirs != fingerprint() {
        format!(
            "the link fingerprint {theirs} is not the Rust host's {}",
            fingerprint()
        )
    } else if held != expected {
        format!(
            "the Node host holds the kinds {held:?}, not \
             {expected:?}"
        )
    } else {
        return Frame::Welcome {
            protocol: PROTOCOL,
            fingerprint: fingerprint().to_owned(),
            kinds: ours.to_vec(),
            routes: routes.clone(),
        };
    };
    Frame::Refused { reason }
}

/// Serves the Node host's half of the link until the Rust host's
/// goodbye, on the ends [`child::take_link`] took. Says hello with
/// the kinds of `factories` before it opens any file. Once welcomed,
/// opens the folder `HAMLET_HOME` as the host `node`.
///
/// No actor of this host sees `link.down` or `link.lost`: the link is
/// up before boot starts any actor, and when the pipe fails either
/// way, this process ends at once.
///
/// Fails with the Rust host's reason when it refuses the hello, when
/// the folder does not open or boot, and when the Rust host breaks
/// the link's rules.
pub async fn serve(
    factories: HashMap<String, ActorFactory>,
) -> Result<()> {
    let child::Ends { mut frames, output } = child::take_ends()
        .context(
            "the link's ends are not taken: call take_link first, \
             and serve the link once",
        )?;
    let home = std::env::var_os(child::HOME)
        .with_context(|| format!("{} is not set", child::HOME))?;
    let out =
        Writer::start(output, "link-output", AtBreak::EndProcess)?;
    let kinds = kinds(&factories);
    out.write(&Frame::Hello {
        protocol: PROTOCOL,
        fingerprint: fingerprint().to_owned(),
        kinds,
        pid: std::process::id(),
    })
    .await?;
    let (theirs, routes) = match frames.next().await {
        Frame::Welcome { kinds, routes, .. } => (kinds, routes),
        Frame::Refused { reason } => {
            bail!("the Rust host refused this Node host: {reason}")
        }
        Frame::Goodbye => return Ok(()),
        frame => bail!(Broken::OutOfTurn(frame.name())),
    };
    let port = Port::new(Role::Node, theirs);
    let host = Host::open_linked(
        Path::new(&home),
        factories,
        HostOptions {
            name: NODE.to_owned(),
            routes,
            ..HostOptions::default()
        },
        Some(port.clone()),
    )?;
    port.welcomed(out.clone(), Inbound::of(&host)?);
    port.ready();
    let mut events = host.events();
    let mut reporting = true;
    let taking = async {
        loop {
            tokio::select! {
                frame = frames.next() => match port.take(frame)? {
                    Taken::Handled => {}
                    Taken::Goodbye => return anyhow::Ok(()),
                },
                event = events.recv(), if reporting => match event {
                    Ok(event) => {
                        let _ = out.write(&Frame::HostEvent(event)).await;
                    }
                    Err(RecvError::Lagged(missed)) => tracing::warn!(
                        missed,
                        "the Rust host missed events of this host"
                    ),
                    Err(RecvError::Closed) => reporting = false,
                },
            }
        }
    };
    let booting = async {
        host.boot().await?;
        out.write(&Frame::Ready).await?;
        anyhow::Ok(())
    };
    let mut taking = std::pin::pin!(taking);
    tokio::select! {
        booted = booting => {
            booted?;
            taking.await?;
        }
        taken = &mut taking => taken?,
    }
    host.shutdown().await
}
