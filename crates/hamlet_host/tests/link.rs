//! The link between the Rust host and its Node child.

use std::io::{ErrorKind, pipe};
use std::path::Path;
use std::thread;

use hamlet_host::link::{
    self, Broken, FRAME_LIMIT, Frame, PROTOCOL, WireError,
};
use hamlet_host::{Address, Caller, HostEvent};
use serde_bytes::ByteBuf;
use serde_reflection::{Tracer, TracerConfig};
use sha2::{Digest, Sha256};

/// The largest write the workspace takes, 16 MiB.
const WRITE_CAP: usize = 16 << 20;

/// The recorded fingerprint, in the crate's folder.
const FINGERPRINT: &str = "link.fingerprint";

fn call(args: Vec<u8>) -> Frame {
    Frame::Call {
        id: 7,
        from: Some(Caller::Actor(Address::new("agent", "a1"))),
        to: Address::new("workspace", "w1"),
        name: "write".to_owned(),
        args,
        deadline_ms: 30_000,
    }
}

/// One frame of each kind, with each field set.
fn every_frame() -> Vec<Frame> {
    vec![
        Frame::Hello {
            protocol: PROTOCOL,
            fingerprint: "f1".to_owned(),
            kinds: vec!["agent".to_owned()],
            pid: 4242,
        },
        Frame::Welcome {
            protocol: PROTOCOL,
            fingerprint: "f1".to_owned(),
            kinds: vec!["realm".to_owned(), "workspace".to_owned()],
        },
        Frame::Refused {
            reason: "another fingerprint".to_owned(),
        },
        Frame::Ready,
        Frame::Goodbye,
        call(vec![0, 1, 2, 255]),
        Frame::Call {
            id: 8,
            from: Some(Caller::Client),
            to: Address::new("agent", "a2"),
            name: "say".to_owned(),
            args: Vec::new(),
            deadline_ms: 1,
        },
        Frame::Call {
            id: 9,
            from: None,
            to: Address::new("agent", "a3"),
            name: "stop".to_owned(),
            args: vec![9],
            deadline_ms: u64::MAX,
        },
        Frame::Reply {
            id: 7,
            result: Ok(ByteBuf::from(vec![3, 4])),
        },
        Frame::Reply {
            id: 8,
            result: Err(WireError {
                group: "actor".to_owned(),
                code: "not_found".to_owned(),
                message: "no actor".to_owned(),
                metadata: Some(r#"{"key":"a2"}"#.to_owned()),
            }),
        },
        Frame::HostEvent(HostEvent::TimerSetAside {
            actor: Address::new("agent", "a1"),
            event_id: "e1".to_owned(),
            action: "wake".to_owned(),
            due_at: 1_700_000_000_000,
            tries: 5,
            error: Some("actor.action_timed_out".to_owned()),
        }),
        Frame::HostEvent(HostEvent::UnreadableFile {
            actor: Address::new("agent", "a4"),
            error: "not a database".to_owned(),
        }),
    ]
}

fn encoded(frame: &Frame) -> Vec<u8> {
    let mut out = Vec::new();
    link::write(&mut out, frame).unwrap();
    out
}

#[test]
fn every_frame_round_trips_and_the_stream_ends_cleanly() {
    let mut stream = Vec::new();
    for frame in every_frame() {
        link::write(&mut stream, &frame).unwrap();
    }
    let mut input = stream.as_slice();
    for frame in every_frame() {
        assert_eq!(link::read(&mut input).unwrap(), Some(frame));
    }
    assert_eq!(link::read(&mut input).unwrap(), None);
}

#[test]
fn a_write_at_the_workspace_cap_round_trips_through_a_pipe() {
    let frame = call(vec![7; WRITE_CAP]);
    let (mut reader, mut writer) = pipe().unwrap();
    let sent = frame.clone();
    let writing =
        thread::spawn(move || link::write(&mut writer, &sent));
    assert_eq!(link::read(&mut reader).unwrap(), Some(frame));
    writing.join().unwrap().unwrap();
    assert_eq!(link::read(&mut reader).unwrap(), None);
}

#[test]
fn a_frame_over_the_limit_is_not_written() {
    let mut out = Vec::new();
    let error = link::write(&mut out, &call(vec![0; FRAME_LIMIT]))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(out.is_empty());
}

#[test]
fn a_bad_magic_breaks_the_link() {
    let mut bytes = encoded(&Frame::Ready);
    bytes[..4].copy_from_slice(b"INFO");
    let read = link::read(&mut bytes.as_slice());
    assert!(
        matches!(read, Err(Broken::Magic(magic)) if &magic == b"INFO"),
        "{read:?}"
    );
}

#[test]
fn a_length_over_the_limit_breaks_the_link_before_its_body() {
    let mut bytes = encoded(&Frame::Ready);
    let length = u32::try_from(FRAME_LIMIT + 1).unwrap();
    bytes[4..8].copy_from_slice(&length.to_be_bytes());
    let read = link::read(&mut bytes.as_slice());
    assert!(
        matches!(read, Err(Broken::TooLong(n)) if n == FRAME_LIMIT + 1),
        "{read:?}"
    );
}

#[test]
fn a_cut_frame_breaks_the_link() {
    let bytes = encoded(&call(vec![1; 100]));
    for cut in [1, 4, 7, 8, bytes.len() - 1] {
        let read = link::read(&mut &bytes[..cut]);
        assert!(
            matches!(read, Err(Broken::Cut)),
            "cut at {cut}: {read:?}"
        );
    }
}

#[test]
fn a_body_that_is_not_one_frame_breaks_the_link() {
    let mut garbage = encoded(&Frame::Goodbye);
    garbage.truncate(8);
    garbage.extend_from_slice(&[0xff; 4]);
    garbage[4..8].copy_from_slice(&4u32.to_be_bytes());

    let mut trailing = encoded(&Frame::Goodbye);
    trailing.push(0);
    let length = u32::try_from(trailing.len() - 8).unwrap();
    trailing[4..8].copy_from_slice(&length.to_be_bytes());

    for bytes in [garbage, trailing] {
        let read = link::read(&mut bytes.as_slice());
        assert!(matches!(read, Err(Broken::Body(_))), "{read:?}");
    }
}

/// The shapes of the frames, as serde writes them, in a stable text.
fn shapes() -> String {
    // Each string the tracer reads is a version, so that a version
    // field reads as one; any other string field takes it as well.
    let config = TracerConfig::default()
        .default_string_value(PROTOCOL.to_string());
    let mut tracer = Tracer::new(config);
    tracer.trace_simple_type::<Caller>().unwrap();
    tracer.trace_simple_type::<HostEvent>().unwrap();
    tracer
        .trace_simple_type::<Result<ByteBuf, WireError>>()
        .unwrap();
    tracer.trace_simple_type::<Frame>().unwrap();
    serde_json::to_string_pretty(&tracer.registry().unwrap()).unwrap()
}

/// The fingerprint file for the frames' shapes `shapes`.
fn fingerprint(shapes: &str) -> String {
    let hash = Sha256::digest(format!("v {PROTOCOL}\n{shapes}"));
    format!(
        "# The link protocol's version and a sha256 of the shapes \
         of its frames\n# with that version, written by `cargo test \
         -p hamlet_host`. Do not edit.\nv {PROTOCOL}\nsha256 \
         {hash:x}\n"
    )
}

#[test]
fn the_frames_shapes_are_recorded() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(FINGERPRINT);
    let expected = fingerprint(&shapes());
    if std::fs::read_to_string(&path).ok().as_deref()
        == Some(&expected)
    {
        assert!(
            expected.ends_with(&format!("{}\n", link::fingerprint()))
        );
        return;
    }
    std::fs::write(&path, expected).unwrap();
    panic!(
        "The link's frames changed, so \
         crates/hamlet_host/{FINGERPRINT} is rewritten; commit it. \
         A Node host built before the change is refused at its \
         hello."
    );
}
