//! The link between the Rust host and its Node child, over the
//! child's pipe: the frames each side writes, and how each is framed.
//!
//! A frame is the four bytes [`MAGIC`], its body's length as a
//! big-endian `u32`, then the body: one [`Frame`] in CBOR, of at most
//! [`FRAME_LIMIT`] bytes. A reader stops at the first frame that
//! breaks these rules, and the link is then down: a stray line of
//! text on the pipe breaks the link rather than being read as a
//! length to wait for.

use std::collections::{BTreeSet, HashMap};
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use hamlet_actor_core::ActorFactory;
use hamlet_protocol::Version;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use crate::child::{self, ChildOptions};
use crate::route::Routes;
use crate::{Address, Caller, Host, HostEvent, HostOptions};

/// The link protocol's version. A host refuses a peer whose version
/// is not compatible with its own by semver's caret.
pub const PROTOCOL: Version = Version::new(0, 1, 0);

/// The four bytes each frame begins with.
pub const MAGIC: [u8; 4] = *b"HLNK";

/// The largest body a frame may have, 32 MiB: a write at the
/// workspace's 16 MiB cap fits with the frame around it.
pub const FRAME_LIMIT: usize = 32 << 20;

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

/// What one side of the link writes to the other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frame {
    /// The Node host's first frame, written before it opens any file:
    /// its protocol, its fingerprint, the actor kinds it holds and
    /// its process id.
    Hello {
        protocol: Version,
        fingerprint: String,
        kinds: Vec<String>,
        pid: u32,
    },
    /// The Rust host takes the Node host's hello. It states its own
    /// protocol, fingerprint and kinds, and the route table the Node
    /// host opens with.
    Welcome {
        protocol: Version,
        fingerprint: String,
        kinds: Vec<String>,
        routes: Routes,
    },
    /// The Rust host refuses the Node host's hello, and says why. It
    /// then closes the pipe.
    Refused { reason: String },
    /// The Node host has opened the folder and booted its kinds.
    Ready,
    /// The Rust host asks the Node host to shut down.
    Goodbye,
    /// A call, from either side: the action `name` of the actor at
    /// `to`, with `args`, made by `from` as the calling host stamps
    /// it, and answered within `deadline_ms`. `id` is the calling
    /// host's own, and the [`Frame::Reply`] carries it back.
    Call {
        id: u64,
        from: Option<Caller>,
        to: Address,
        name: String,
        #[serde(with = "serde_bytes")]
        args: Vec<u8>,
        deadline_ms: u64,
    },
    /// The outcome of the call `id`: its output, or its error.
    Reply {
        id: u64,
        result: Result<ByteBuf, WireError>,
    },
    /// Something that happened to an actor of the Node host, for the
    /// Rust host's clients.
    HostEvent(HostEvent),
}

/// An actor error as it crosses the link, with the fields that give
/// it its identity on the other side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    pub group: String,
    pub code: String,
    pub message: String,
    /// The error's metadata, as JSON text.
    pub metadata: Option<String>,
}

/// Why a reader stopped: the link is down from the first frame that
/// breaks the framing.
#[derive(Debug)]
pub enum Broken {
    /// The stream ended inside a frame.
    Cut,
    /// A frame began with these four bytes in place of [`MAGIC`].
    Magic([u8; 4]),
    /// A frame's body was this long, over [`FRAME_LIMIT`]. Its body
    /// is not read.
    TooLong(usize),
    /// A body that is not one [`Frame`] in CBOR, and why.
    Body(String),
    /// The pipe failed.
    Io(io::Error),
}

impl std::fmt::Display for Broken {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Cut => write!(f, "the link ended inside a frame"),
            Self::Magic(bytes) => {
                write!(
                    f,
                    "a frame began with {bytes:?}, not the magic"
                )
            }
            Self::TooLong(length) => write!(
                f,
                "a frame of {length} bytes, over the limit of \
                 {FRAME_LIMIT}"
            ),
            Self::Body(error) => {
                write!(f, "a frame's body is not a frame: {error}")
            }
            Self::Io(error) => write!(f, "the link failed: {error}"),
        }
    }
}

impl std::error::Error for Broken {}

/// Writes `frame` to `out` in one write. A frame whose body would be
/// over [`FRAME_LIMIT`] fails with [`ErrorKind::InvalidInput`], and
/// nothing is written.
pub fn write(out: &mut impl Write, frame: &Frame) -> io::Result<()> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&[0; 4]);
    ciborium::into_writer(frame, &mut bytes).map_err(|error| {
        io::Error::new(ErrorKind::InvalidData, error)
    })?;
    let length = bytes.len() - 8;
    if length > FRAME_LIMIT {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "a frame of {length} bytes, over the limit of \
                 {FRAME_LIMIT}"
            ),
        ));
    }
    let length = u32::try_from(length).expect("the limit fits a u32");
    bytes[4..8].copy_from_slice(&length.to_be_bytes());
    out.write_all(&bytes)?;
    out.flush()
}

/// Reads the next frame from `input`. Returns `None` when the stream
/// ends between frames, and fails when it breaks the framing.
pub fn read(input: &mut impl Read) -> Result<Option<Frame>, Broken> {
    let mut head = [0; 8];
    match fill(input, &mut head)? {
        0 => return Ok(None),
        8 => {}
        _ => return Err(Broken::Cut),
    }
    let magic = [head[0], head[1], head[2], head[3]];
    if magic != MAGIC {
        return Err(Broken::Magic(magic));
    }
    let length =
        u32::from_be_bytes([head[4], head[5], head[6], head[7]])
            as usize;
    if length > FRAME_LIMIT {
        return Err(Broken::TooLong(length));
    }
    let mut body = vec![0; length];
    if fill(input, &mut body)? < length {
        return Err(Broken::Cut);
    }
    let mut rest = body.as_slice();
    let frame = ciborium::from_reader(&mut rest)
        .map_err(|error| Broken::Body(error.to_string()))?;
    if !rest.is_empty() {
        return Err(Broken::Body(format!(
            "{} bytes after the frame",
            rest.len()
        )));
    }
    Ok(Some(frame))
}

/// Reads into `buf` until it is full or the stream ends, and returns
/// how many bytes it read.
fn fill(
    input: &mut impl Read,
    buf: &mut [u8],
) -> Result<usize, Broken> {
    let mut filled = 0;
    while filled < buf.len() {
        match input.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(Broken::Io(error)),
        }
    }
    Ok(filled)
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
/// [`child::take_stdio`] took. Says hello with the kinds of
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
        "the link's ends are not taken: call take_stdio first, and \
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
