//! Recorded sessions through the generated wire types.

use std::path::{Path, PathBuf};

use hamlet_protocol::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn frames() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("frames")
}

/// Reads `wire` as an `M` and checks it is written back unchanged.
fn round_trip<M: Serialize + DeserializeOwned>(wire: &Value) {
    let message = serde_json::from_value::<M>(wire.clone())
        .unwrap_or_else(|error| panic!("{error}: {wire}"));
    assert_eq!(&serde_json::to_value(&message).unwrap(), wire);
}

#[test]
fn every_recorded_message_reads_and_writes_back_unchanged() {
    let mut files = std::fs::read_dir(frames())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "jsonl"))
        .collect::<Vec<_>>();
    files.sort();
    assert!(files.len() >= 18, "{files:?}");
    for file in files {
        for line in std::fs::read_to_string(&file).unwrap().lines() {
            let frame = serde_json::from_str::<Value>(line).unwrap();
            match frame["from"].as_str() {
                Some("client") => {
                    round_trip::<ClientMessage>(&frame["message"])
                }
                Some("server") => {
                    round_trip::<ServerMessage>(&frame["message"])
                }
                _ => panic!(
                    "{} has a frame from no side",
                    file.display()
                ),
            }
        }
    }
}

fn messages(name: &str) -> Vec<ServerMessage> {
    let file = frames().join(format!("{name}.jsonl"));
    std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let frame = serde_json::from_str::<Value>(line).unwrap();
            (frame["from"] == "server").then(|| {
                serde_json::from_value::<ServerMessage>(
                    frame["message"].clone(),
                )
                .unwrap()
            })
        })
        .collect()
}

#[test]
fn a_recorded_held_write_carries_its_preview_through_the_wait_and_close()
 {
    let messages = messages("approval");
    let preview = messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::Change(changes) => Some(&changes.changes),
            _ => None,
        })
        .flatten()
        .find_map(|update| match update {
            Update::Wait { wait }
                if matches!(
                    wait.detail,
                    WaitDetail::Approval {
                        danger: WaitDanger::Edit,
                        ..
                    }
                ) =>
            {
                wait.change.as_ref()
            }
            _ => None,
        })
        .expect(
            "the approval recording needs a held write with a \
             preview",
        );
    assert_eq!(preview.path, "result.txt");
    assert_eq!(
        preview
            .lines
            .iter()
            .map(|line| (&line.mark, line.text.as_str()))
            .collect::<Vec<_>>(),
        [
            (&ChangeLinesMark::Space, "heading"),
            (&ChangeLinesMark::Minus, "approved"),
            (&ChangeLinesMark::Plus, "reviewed"),
        ]
    );
    let ops = messages
        .iter()
        .filter_map(|message| match message {
            ServerMessage::Turn(turn) => Some(&turn.ops),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    let write = ops
        .iter()
        .find_map(|op| match op {
            Op::Add { item, .. }
                if item
                    .tool
                    .as_ref()
                    .is_some_and(|tool| tool.name == "write") =>
            {
                Some(&item.id)
            }
            _ => None,
        })
        .expect("the held write needs a tool item");
    assert!(ops.iter().any(|op| matches!(op,
        Op::Close { item, exit: 0, change: Some(Some(change)), .. }
            if item == write && change == preview
    )));
}

#[test]
fn a_recorded_rewind_returns_words_and_an_adjacent_reset_and_transcript()
 {
    let messages = messages("rewind");
    assert!(messages.iter().any(|message| matches!(message, ServerMessage::Reply(reply) if reply.kind == ReplyKind::Rewound && reply.text == "Take back this line.")), "the rewind recording needs its receipt");
    let reset = messages
        .iter()
        .position(|message| matches!(message, ServerMessage::Turn(turn) if turn.ops.iter().any(|op| matches!(op, Op::Reset {}))))
        .expect("the rewind recording needs a reset");
    let ServerMessage::Transcript(transcript) = &messages[reset + 1]
    else {
        panic!(
            "a reset must be immediately followed by its \
             replacement transcript"
        );
    };
    let ServerMessage::Turn(turn) = &messages[reset] else {
        unreachable!();
    };
    assert_eq!(transcript.chat, turn.chat);
    assert_eq!(
        transcript
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["Keep this line.", "First answer."]
    );
}
