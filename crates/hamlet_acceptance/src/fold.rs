//! A chat's items, folded from its pages and live operations as a
//! client keeps them.

use std::collections::HashSet;

use hamlet_protocol::client::transcript::apply;
use hamlet_protocol::client::*;

/// The items of `chat`: its latest opening page, then every operation
/// after it. Older pages fetched with `before`, whose request IDs are
/// in `pages`, change nothing.
pub fn items(
    log: &[ServerMessage],
    chat: &ChatRef,
    pages: &HashSet<RequestId>,
) -> Vec<Item> {
    let mut items = Vec::<Item>::new();
    for message in log {
        match message {
            ServerMessage::Transcript(t)
                if &t.chat == chat
                    && !t
                        .to
                        .as_ref()
                        .is_some_and(|to| pages.contains(to)) =>
            {
                items.clone_from(&t.items);
            }
            ServerMessage::Turn(t) if &t.chat == chat => {
                for operation in &t.operations {
                    apply(&mut items, operation, Item::clone)
                        .unwrap_or_else(|error| panic!("{error}"));
                }
            }
            _ => {}
        }
    }
    items
}

/// Whether `chat` is working, from its latest page or status
/// operation.
pub fn is_working(log: &[ServerMessage], chat: &ChatRef) -> bool {
    let mut working = false;
    for message in log {
        match message {
            ServerMessage::Transcript(t) if &t.chat == chat => {
                working = t.working;
            }
            ServerMessage::Turn(t) if &t.chat == chat => {
                for operation in &t.operations {
                    if let TranscriptOperation::Status {
                        working: w,
                        ..
                    } = operation
                    {
                        working = *w;
                    }
                }
            }
            _ => {}
        }
    }
    working
}
