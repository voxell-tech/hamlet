//! A chat's items, folded from its pages and live ops as a client
//! keeps them.

use std::collections::HashSet;

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
                    apply(&mut items, op);
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

fn apply(items: &mut Vec<Item>, op: &Op) {
    let find = |items: &mut Vec<Item>, id: &str| {
        items.iter().position(|i| i.id == id).unwrap_or_else(|| {
            panic!("an op names {id}, which is no item")
        })
    };
    match op {
        Op::Add { item, replaces } => {
            if let Some(old) = replaces {
                let at = find(items, old);
                items.remove(at);
            }
            items.push(item.clone());
        }
        Op::Append { item, text } => {
            let at = find(items, item);
            items[at].text.push_str(text);
        }
        Op::Close {
            item,
            exit,
            ms,
            failure,
            change,
        } => {
            let at = find(items, item);
            let tool = items[at].tool.as_mut().unwrap_or_else(|| {
                panic!("{item} closes but is no tool")
            });
            tool.exit = Some(*exit);
            tool.ms = *ms;
            tool.failure.clone_from(failure);
            if let Some(change) = change {
                tool.change.clone_from(change);
            }
        }
        Op::Reset {} => items.clear(),
        Op::Status { .. } | Op::Usage { .. } => {}
    }
}
