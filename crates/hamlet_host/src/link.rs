//! The link between the Rust host and its Node child, over the
//! child's pipe: the frames each side writes (`frame.rs`), who each
//! side speaks for (`peer.rs`), and each host's port to the other
//! (`port.rs`).

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

use crate::child::{self, ChildOptions};
use crate::{Host, HostOptions};

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

/// The Rust host's answer to a Node host's hello: a
/// [`Frame::Welcome`] stating the Rust host's own kinds and routes,
/// or a [`Frame::Refused`] when the hello's protocol is not
/// compatible with [`PROTOCOL`], its fingerprint is not
/// [`fingerprint`], or its kinds are not the ones the Node host must
/// hold.
pub(crate) fn answer(
    protocol: &Version,
    theirs: &str,
    kinds: &[String],
    options: &ChildOptions,
) -> Frame {
    let held = kinds.iter().collect::<BTreeSet<_>>();
    let expected = options.node_kinds.iter().collect::<BTreeSet<_>>();
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
            kinds: options.kinds.clone(),
            routes: options.routes.clone(),
        };
    };
    Frame::Refused { reason }
}

/// Serves the Node host's half of the link, on the ends
/// [`child::take_link`] took. Says hello with the kinds of
/// `factories` before it opens any file. Once welcomed, opens the
/// folder `HAMLET_HOME` as the host `node` with them and the
/// welcome's routes, boots it, says it is ready, and at the goodbye
/// shuts the host down and returns.
/// Fails with the Rust host's reason when it refuses the hello, and
/// when the folder does not open or boot.
pub async fn serve(
    factories: HashMap<String, ActorFactory>,
) -> Result<()> {
    let child::Ends {
        mut frames,
        mut output,
    } = child::take_ends().context(
        "the link's ends are not taken: call take_link first, and \
         serve the link once",
    )?;
    let home = std::env::var_os(child::HOME)
        .with_context(|| format!("{} is not set", child::HOME))?;
    let mut kinds = factories.keys().cloned().collect::<Vec<_>>();
    kinds.sort_unstable();
    write(
        &mut output,
        &Frame::Hello {
            protocol: PROTOCOL,
            fingerprint: fingerprint().to_owned(),
            kinds,
            pid: std::process::id(),
        },
    )?;
    let routes = match frames.recv().await {
        Some(Frame::Welcome { routes, .. }) => routes,
        Some(Frame::Refused { reason }) => {
            bail!("the Rust host refused this Node host: {reason}")
        }
        Some(Frame::Goodbye) => return Ok(()),
        frame => bail!("the Rust host wrote {frame:?} out of turn"),
    };
    let host = Host::open_with(
        Path::new(&home),
        factories,
        HostOptions {
            name: NODE.to_owned(),
            routes,
            ..HostOptions::default()
        },
    )?;
    host.boot().await?;
    write(&mut output, &Frame::Ready)?;
    match frames.recv().await {
        Some(Frame::Goodbye) => host.shutdown().await,
        frame => bail!("the Rust host wrote {frame:?} out of turn"),
    }
}
