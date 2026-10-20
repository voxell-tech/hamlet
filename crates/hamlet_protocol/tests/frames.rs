//! Recorded server messages through the generated wire types.

use hamlet_protocol::v0::*;
use serde_json::Value;

fn messages(name: &str) -> Vec<ServerMessage> {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("frames")
        .join(format!("{name}.jsonl"));
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
