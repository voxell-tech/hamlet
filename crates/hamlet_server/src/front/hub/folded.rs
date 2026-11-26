//! What an agent does and says in one chat, folded from the chat's
//! changes.

use std::collections::HashMap;

use hamlet_messages::agent::chat::{ChatItem, ChatOp, ChatPage};
use hamlet_protocol::client::ItemKind;

use super::Mark;
use crate::talk::activity::Activity;

/// What an agent does in one chat, folded from the chat's changes.
pub(super) struct Folded {
    pub(super) activity: Activity,
    /// The text of each item written so far, and whether it is a
    /// thought, by item.
    written: HashMap<String, (bool, String)>,
    /// The change its fold holds up to; an earlier one is in it.
    pub(super) seen: Option<Mark>,
}

impl Folded {
    pub(super) fn new() -> Self {
        Self {
            activity: Activity::of(&[]),
            written: HashMap::new(),
            seen: None,
        }
    }

    /// The fold of `page`, the chat read now: its status, and the
    /// items since the chief's last line.
    pub(super) fn of(page: &ChatPage) -> Self {
        let mut folded = Self::new();
        folded.seen = Some(Mark::of(page));
        folded.status(page.working, page.pending);
        let since = page
            .items
            .iter()
            .rposition(|item| item.kind == ItemKind::User)
            .map_or(0, |at| at + 1);
        for item in &page.items[since..] {
            folded.add(item, None);
        }
        folded
    }

    pub(super) fn fold(&mut self, op: &ChatOp) {
        match op {
            ChatOp::Status { working, pending } => {
                self.status(*working, *pending);
            }
            ChatOp::Add { item, replaces } => {
                self.add(item, replaces.as_deref());
            }
            ChatOp::Append { item, text, .. } => {
                if let Some((thought, written)) =
                    self.written.get_mut(item)
                {
                    written.push_str(text);
                    self.activity.said(text, *thought);
                }
            }
            ChatOp::Usage { .. } | ChatOp::Reset => {}
        }
    }

    /// Takes in whether the chat runs and how many inputs wait: a
    /// turn that begins calls its model.
    fn status(&mut self, working: bool, pending: u32) {
        let begins = working && !self.activity.working;
        self.activity.live(working, u64::from(pending), None);
        if begins {
            self.activity.attempted();
        }
        if !working {
            self.written.clear();
        }
    }

    fn add(&mut self, item: &ChatItem, replaces: Option<&str>) {
        let activity = &mut self.activity;
        match (&item.tool, &item.kind) {
            (Some(tool), _) if replaces == Some(item.id.as_str()) => {
                if tool.exit.is_some() {
                    activity.ended(tool.failure.is_some());
                }
            }
            (Some(tool), _) => {
                activity.called(&tool.name, &tool.summary);
            }
            (None, ItemKind::Assistant | ItemKind::Reasoning) => {
                let thought = item.kind == ItemKind::Reasoning;
                let before = replaces
                    .and_then(|replaced| {
                        self.written.remove(replaced)
                    })
                    .map(|(_, text)| text)
                    .unwrap_or_default();
                let new = item
                    .text
                    .strip_prefix(before.as_str())
                    .unwrap_or(&item.text);
                if !new.is_empty() {
                    activity.said(new, thought);
                }
                self.written.insert(
                    item.id.clone(),
                    (thought, item.text.clone()),
                );
            }
            (None, ItemKind::Error) => activity.faulted = true,
            _ => {}
        }
    }
}
