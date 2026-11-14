//! What a frame is, and how it is read and written.
//!
//! A frame is the four bytes [`MAGIC`], its body's length as a
//! big-endian `u32`, then the body: one [`Frame`] in CBOR, of at most
//! [`FRAME_LIMIT`] bytes. A reader stops at the first frame that
//! breaks these rules, and the link is then down: a stray line of
//! text on the pipe breaks the link rather than being read as a
//! length to wait for.

use std::io::{self, ErrorKind, PipeWriter, Read, Write};

use hamlet_protocol::Version;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use tokio::sync::mpsc;

use crate::route::Routes;
use crate::{Address, Caller, HostEvent};

/// The four bytes each frame begins with.
pub const MAGIC: [u8; 4] = *b"HLNK";

/// The largest body a frame may have, 32 MiB: a write at the
/// workspace's 16 MiB cap fits with the frame around it.
pub const FRAME_LIMIT: usize = 32 << 20;

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
    /// What a host asks of the actor at `to` in the other host, for
    /// `from` as the asking host stamps it. `id` is the asking host's
    /// own, and the [`Frame::Reply`] carries it back. Every operation
    /// that crosses the link is one of these, so the stamp is
    /// checked, and the answer carried, in one place.
    Request {
        id: u64,
        from: Option<Caller>,
        to: Address,
        ask: Ask,
    },
    /// The outcome of the request `id`: its output, or its error. A
    /// create's output is empty.
    Reply {
        id: u64,
        result: Result<ByteBuf, WireError>,
    },
    /// Something that happened to an actor of the Node host, for the
    /// Rust host's clients. Only the Node host writes it.
    HostEvent(HostEvent),
}

/// What a host asks of an actor in the other host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ask {
    /// Run the action `name` with `args`, answered within
    /// `deadline_ms`. Never makes the actor.
    Call {
        name: String,
        #[serde(with = "serde_bytes")]
        args: Vec<u8>,
        deadline_ms: u64,
    },
    /// Make the actor with `input` and start it. Fails with
    /// `actor.duplicate_key` for a key that holds an actor.
    Create {
        #[serde(with = "serde_bytes")]
        input: Vec<u8>,
    },
}

/// An actor error as it crosses the link, with the fields that give
/// it its identity on the other side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    pub group: String,
    pub code: String,
    pub message: String,
    /// The error's metadata, as the JSON text the error holds.
    pub metadata: Option<String>,
}

impl WireError {
    /// `error` as it crosses the link.
    ///
    /// When `error`'s chain holds an `ActorError` (the first one, as
    /// `ActorError::extract` finds it), that error's group, code,
    /// message and metadata cross. The metadata crosses as the raw
    /// JSON text the error holds, not parsed.
    ///
    /// When the chain holds none, the error crosses as
    /// `core.internal_error`, with the whole chain (`{error:#}`) as
    /// the message and `{"error": "<Debug>"}` as the metadata, not
    /// masked.
    ///
    /// The actor specifier and the `anyhow` context around a coded
    /// error do not cross.
    pub(crate) fn of(error: &anyhow::Error) -> Self {
        let _ = error;
        todo!("U5, the link unit: an error as it crosses the link")
    }

    /// The error for the asking host's caller: a dynamic `ActorError`
    /// with the same group, code, message and metadata, which
    /// `ActorError::extract` and the bridge to JavaScript read as
    /// they read any other.
    pub(crate) fn into_error(self) -> anyhow::Error {
        todo!("U5, the link unit: a crossed error for the caller")
    }
}

/// Why the link is down: the first frame that breaks the framing, or
/// the link's rules.
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
    /// The other host asked for this caller, which it does not speak
    /// for. A correct peer never does: its own link refuses that
    /// before a frame is written.
    Stamp(Option<Caller>),
    /// A frame the other host may not write, or not now, and what it
    /// was.
    OutOfTurn(String),
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
            Self::Stamp(Some(caller)) => write!(
                f,
                "the other host asked as {caller}, which it does \
                 not speak for"
            ),
            Self::Stamp(None) => write!(
                f,
                "the other host asked with no caller, which it does \
                 not speak for"
            ),
            Self::OutOfTurn(frame) => {
                write!(f, "the other host wrote {frame} out of turn")
            }
        }
    }
}

impl std::error::Error for Broken {}

/// Writes `frame` to `out` in one write. A frame whose body would be
/// over [`FRAME_LIMIT`] fails with [`ErrorKind::InvalidInput`], and
/// nothing is written.
pub fn write(out: &mut impl Write, frame: &Frame) -> io::Result<()> {
    out.write_all(&encode(frame)?)?;
    out.flush()
}

/// The bytes of `frame`: the magic, the length, the body. Fails with
/// [`ErrorKind::InvalidInput`] for a body over [`FRAME_LIMIT`].
pub(crate) fn encode(frame: &Frame) -> io::Result<Vec<u8>> {
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
    Ok(bytes)
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

/// What a [`Writer`]'s thread does when the pipe fails.
pub(crate) enum AtBreak {
    /// Stop writing: the Rust host, whose reader sees the child end
    /// and whose supervisor then ends the session.
    Close,
    /// End this process at once: the Node host, which must not
    /// outlive its link, and whose actors must not see it fail.
    EndProcess,
}

/// The one writer of a pipe: a thread that writes each frame whole,
/// in the order they were queued. Every frame a side writes goes
/// through it, the handshake's too, so two frames never interleave
/// and a large frame to a slow peer blocks this thread, not a
/// runtime's.
#[derive(Clone)]
pub(crate) struct Writer(mpsc::Sender<Vec<u8>>);

impl Writer {
    /// Starts the thread named `thread`, which writes to `out` until
    /// the last `Writer` drops, then closes the pipe. When a write
    /// fails, it does as `at_break` says.
    pub(crate) fn start(
        out: PipeWriter,
        thread: &str,
        at_break: AtBreak,
    ) -> io::Result<Self> {
        let _ = (out, thread, at_break);
        todo!("U5, the link unit: the pipe's one writer")
    }

    /// Queues `frame`, waiting while the queue is full. Fails with
    /// [`ErrorKind::InvalidInput`] for a frame over the limit, of
    /// which nothing is queued, and with [`ErrorKind::BrokenPipe`]
    /// once the thread has stopped.
    pub(crate) async fn write(
        &self,
        frame: &Frame,
    ) -> io::Result<()> {
        let _ = frame;
        todo!("U5, the link unit: queue a frame")
    }
}
