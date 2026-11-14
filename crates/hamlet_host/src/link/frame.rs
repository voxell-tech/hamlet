//! A frame is the four bytes [`MAGIC`], its body's length as a
//! big-endian `u32`, then the body: one [`Frame`] in CBOR, of at most
//! [`FRAME_LIMIT`] bytes. A reader stops at the first frame that
//! breaks these rules, and the link is then down: a stray line of
//! text on the pipe breaks the link rather than being read as a
//! length to wait for.

use std::io::{self, ErrorKind, PipeWriter, Read, Write};
use std::thread;

use hamlet_actor_error::{
    ActorError, ActorErrorKind, INTERNAL_ERROR,
};
use hamlet_os::process;
use hamlet_protocol::Version;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use serde_json::value::RawValue;
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
    /// A request to the actor at `to` in the other host, for `from`
    /// as the asking host stamps it. `id` is the asking host's own,
    /// and the [`Frame::Reply`] carries it back.
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

impl Frame {
    /// The frame's tag as serde writes it.
    pub fn name(&self) -> String {
        match ciborium::Value::serialized(self) {
            Ok(ciborium::Value::Text(name)) => name,
            Ok(ciborium::Value::Map(fields)) => fields
                .into_iter()
                .find_map(|(tag, _)| tag.into_text().ok())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}

/// An operation one host asks of an actor in the other.
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
    pub(crate) fn of(error: &anyhow::Error) -> Self {
        let coded = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<ActorError>());
        let Some(coded) = coded else {
            let debug =
                serde_json::json!({ "error": format!("{error:?}") });
            return Self {
                group: INTERNAL_ERROR.group.to_owned(),
                code: INTERNAL_ERROR.code.to_owned(),
                message: format!("{error:#}"),
                metadata: Some(debug.to_string()),
            };
        };
        Self {
            group: coded.group().to_owned(),
            code: coded.code().to_owned(),
            message: coded.message().to_owned(),
            metadata: coded
                .meta
                .as_deref()
                .map(|meta| meta.get().to_owned()),
        }
    }

    /// The error for the asking host's caller.
    pub(crate) fn into_error(self) -> anyhow::Error {
        let Self {
            group,
            code,
            message,
            metadata,
        } = self;
        anyhow::Error::new(ActorError {
            kind: ActorErrorKind::Dynamic {
                group,
                code,
                default_message: message.clone(),
            },
            meta: metadata
                .and_then(|text| RawValue::from_string(text).ok()),
            message: Some(message),
            actor: None,
        })
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
    /// for.
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

/// The frames a [`Writer`] holds unwritten before a write waits.
const QUEUE: usize = 16;

/// The action of a [`Writer`]'s thread when the pipe fails.
pub(crate) enum AtBreak {
    Close,
    EndProcess,
}

/// The one writer of a pipe: a thread that writes each frame whole,
/// in the order they were queued.
#[derive(Clone)]
pub(crate) struct Writer(mpsc::Sender<Vec<u8>>);

impl Writer {
    /// Starts the thread named `thread`, which writes to `out` until
    /// the last `Writer` drops, then closes the pipe.
    pub(crate) fn start(
        mut out: PipeWriter,
        thread: &str,
        at_break: AtBreak,
    ) -> io::Result<Self> {
        let (queue, mut queued) = mpsc::channel::<Vec<u8>>(QUEUE);
        thread::Builder::new().name(thread.to_owned()).spawn(
            move || {
                while let Some(bytes) = queued.blocking_recv() {
                    if out
                        .write_all(&bytes)
                        .and_then(|()| out.flush())
                        .is_err()
                    {
                        match at_break {
                            AtBreak::Close => return,
                            AtBreak::EndProcess => process::end_now(),
                        }
                    }
                }
            },
        )?;
        Ok(Self(queue))
    }

    /// Queues `frame`, waiting while the queue is full. Fails with
    /// [`ErrorKind::InvalidInput`] for a frame over the limit, of
    /// which nothing is queued, and with [`ErrorKind::BrokenPipe`]
    /// once the thread has stopped.
    pub(crate) async fn write(
        &self,
        frame: &Frame,
    ) -> io::Result<()> {
        self.queue(encode(frame)?).await
    }

    /// Queues the bytes of a frame [`encode`] made, as
    /// [`Writer::write`] does.
    pub(super) async fn queue(
        &self,
        bytes: Vec<u8>,
    ) -> io::Result<()> {
        self.0
            .send(bytes)
            .await
            .map_err(|_| io::Error::from(ErrorKind::BrokenPipe))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use anyhow::anyhow;
    use serde_json::json;

    use super::*;

    const TEN_S: Duration = Duration::from_secs(10);

    fn request(id: u64, args: Vec<u8>) -> Frame {
        Frame::Request {
            id,
            from: None,
            to: Address::new("agent", "a").unwrap(),
            ask: Ask::Call {
                name: "say".to_owned(),
                args,
                deadline_ms: 1_000,
            },
        }
    }

    /// The metadata's text is one a parse would change: its keys out
    /// of order, and a number with a trailing zero.
    #[test]
    fn a_coded_error_keeps_its_four_fields_through_of_and_into_error()
    {
        let meta =
            r#"{"caller":"client","action":"end","weight":1.50}"#;
        let coded = anyhow::Error::new(ActorError {
            kind: ActorErrorKind::Dynamic {
                group: "message".to_owned(),
                code: "wrong_caller".to_owned(),
                default_message: "The caller may not send this."
                    .to_owned(),
            },
            meta: Some(
                RawValue::from_string(meta.to_owned()).unwrap(),
            ),
            message: Some("client may not send 'end'.".to_owned()),
            actor: None,
        })
        .context("call agent/a");
        let crossed =
            ActorError::extract(&WireError::of(&coded).into_error());
        assert_eq!(
            (crossed.group(), crossed.code(), crossed.message()),
            ("message", "wrong_caller", "client may not send 'end'.")
        );
        assert_eq!(
            crossed.meta.as_deref().map(RawValue::get),
            Some(meta)
        );
    }

    #[test]
    fn a_raw_error_crosses_with_its_chain_as_the_message() {
        let raw = anyhow!("the disk is full").context("open agent/a");
        let crossed =
            ActorError::extract(&WireError::of(&raw).into_error());
        assert_eq!(
            (crossed.group(), crossed.code(), crossed.message()),
            (
                "core",
                "internal_error",
                "open agent/a: the disk is full"
            )
        );
        assert_eq!(
            crossed.metadata(),
            Some(json!({ "error": format!("{raw:?}") }))
        );
    }

    /// Each frame is larger than a pipe's buffer, so two frames
    /// written at once in parts would interleave.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn frames_queued_at_once_arrive_whole() {
        let (mut input, output) = io::pipe().unwrap();
        let reader = thread::spawn(move || {
            let mut frames = Vec::new();
            while let Some(frame) = read(&mut input).unwrap() {
                frames.push(frame);
            }
            frames
        });
        let writer =
            Writer::start(output, "test-link-output", AtBreak::Close)
                .unwrap();
        let sent = (0..64u8)
            .map(|id| {
                request(
                    id.into(),
                    vec![id; (100 << 10) + usize::from(id)],
                )
            })
            .collect::<Vec<_>>();
        let tasks = sent
            .chunks(4)
            .map(|frames| {
                let (writer, frames) =
                    (writer.clone(), frames.to_vec());
                tokio::spawn(async move {
                    for frame in &frames {
                        writer.write(frame).await.unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for task in tasks {
            task.await.unwrap();
        }
        drop(writer);
        let mut received = tokio::time::timeout(
            TEN_S,
            tokio::task::spawn_blocking(move || {
                reader.join().unwrap()
            }),
        )
        .await
        .unwrap()
        .unwrap();
        received.sort_by_key(|frame| match frame {
            Frame::Request { id, .. } => *id,
            _ => u64::MAX,
        });
        assert_eq!(received, sent);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_large_frame_to_a_slow_reader_blocks_only_the_writer() {
        let (mut input, output) = io::pipe().unwrap();
        let (go, told) = std::sync::mpsc::channel();
        let reader = thread::spawn(move || {
            let told = told.recv_timeout(TEN_S).is_ok();
            (told, read(&mut input).unwrap())
        });
        let writer =
            Writer::start(output, "test-link-output", AtBreak::Close)
                .unwrap();
        let frame = request(1, vec![7; 24 << 20]);
        writer.write(&frame).await.unwrap();
        let ticking = tokio::spawn(async {
            for _ in 0..3 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
        ticking.await.unwrap();
        let _ = go.send(());
        let (told, read) = tokio::task::spawn_blocking(move || {
            reader.join().unwrap()
        })
        .await
        .unwrap();
        assert!(
            told,
            "the write held the runtime until the frame was read"
        );
        assert_eq!(read, Some(frame));
    }

    #[tokio::test]
    async fn a_writer_queues_no_frame_over_the_limit_and_fails_once_its_pipe_breaks()
     {
        let (mut input, output) = io::pipe().unwrap();
        let writer =
            Writer::start(output, "test-link-output", AtBreak::Close)
                .unwrap();
        let over = writer
            .write(&request(1, vec![0; FRAME_LIMIT]))
            .await
            .unwrap_err();
        assert_eq!(over.kind(), ErrorKind::InvalidInput);
        writer.write(&Frame::Ready).await.unwrap();
        let (input, next) = tokio::task::spawn_blocking(move || {
            let next = read(&mut input).unwrap();
            (input, next)
        })
        .await
        .unwrap();
        assert_eq!(next, Some(Frame::Ready));

        drop(input);
        let broken = tokio::time::timeout(TEN_S, async {
            loop {
                if let Err(error) = writer.write(&Frame::Ready).await
                {
                    return error;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(broken.kind(), ErrorKind::BrokenPipe);
    }
}
