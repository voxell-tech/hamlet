//! A chat's items, folded from its pages and live ops as a client
//! keeps them.

use std::collections::HashSet;

use hamlet_protocol::transcript::apply;
use hamlet_protocol::v0::*;

/// The items of `chat`: its latest opening page, then every op after
/// it. Older pages fetched with `before`, whose request IDs are in
/// `pages`, change nothing.
pub fn items(
    log: &[ServerMessage],
    chat: &ChatRef,
    pages: &HashSet<String>,
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
                for op in &t.ops {
                    apply(&mut items, op, Item::clone)
                        .unwrap_or_else(|error| panic!("{error}"));
                }
            }
            _ => {}
        }
    }
    items
}

/// Whether `chat` is working, from its latest page or status op.
pub fn working(log: &[ServerMessage], chat: &ChatRef) -> bool {
    let mut working = false;
    for message in log {
        match message {
            ServerMessage::Transcript(t) if &t.chat == chat => {
                working = t.working;
            }
            ServerMessage::Turn(t) if &t.chat == chat => {
                for op in &t.ops {
                    if let Op::Status { working: w, .. } = op {
                        working = *w;
                    }
                }
            }
            _ => {}
        }
    }
    working
}
