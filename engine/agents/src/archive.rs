//! The past sessions of the open conversation's agent: listed and
//! searched, read, and deleted.

use std::time::UNIX_EPOCH;

use bevy::ecs::world::World;
use hamlet_core::session::archive::{self, Past as Kept};
use hamlet_core::session::{self, Entry, SessionKey};
use hamlet_engine_api::Failure;
use hamlet_engine_api::agents::{Past, Pasts, SessionMessage};
use hamlet_rules::budget::format_tokens;

use crate::conversation::ConversationWorldExt;
use crate::{Chief, views};

/// The most messages of a past session that are told.
const READ_MOST: usize = 400;

/// The archive's methods on [`World`].
pub(crate) trait ArchiveWorldExt {
    /// The past sessions of the open conversation's agent, newest
    /// first.
    fn kept(&mut self) -> Vec<Kept>;

    /// Those with every word of `query`, of all.
    fn pasts(&mut self, query: &str) -> Pasts;

    /// The messages of the past session `name`, without the steps
    /// between them.
    fn read_past(
        &mut self,
        name: &str,
    ) -> Result<Vec<SessionMessage>, Failure>;

    /// Deletes the archived session `name`.
    fn delete_past(&mut self, name: &str) -> Result<(), Failure>;
}

impl ArchiveWorldExt for World {
    fn kept(&mut self) -> Vec<Kept> {
        let open = self.resource::<Chief>().session;
        let live = self
            .get::<SessionKey>(open)
            .cloned()
            .and_then(|key| session::log_of(self, &key));
        archive::pasts(&self.open_house().sessions(), live.as_deref())
    }

    fn pasts(&mut self, query: &str) -> Pasts {
        let kept = self.kept();
        let wide = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Pasts {
            all: wide(kept.len()),
            found: kept
                .iter()
                .filter(|past| past.matches(query))
                .map(|past| Past {
                    name: name_of(past),
                    title: past.title.clone(),
                    by: past.by.clone(),
                    modified: past
                        .modified
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |since| {
                            u64::try_from(since.as_millis())
                                .unwrap_or(u64::MAX)
                        }),
                    number: past.number.clone(),
                    ours: past.ours,
                    messages: wide(past.messages),
                    tokens: match past.tokens {
                        0 => String::new(),
                        tokens => format_tokens(tokens),
                    },
                })
                .collect(),
        }
    }

    fn read_past(
        &mut self,
        name: &str,
    ) -> Result<Vec<SessionMessage>, Failure> {
        let past = self
            .kept()
            .into_iter()
            .find(|past| name_of(past) == name)
            .ok_or(Failure::NotFound)?;
        let said = archive::read(&past.path);
        let messages = said
            .items()
            .iter()
            .filter(|item| {
                matches!(
                    item.entry,
                    Entry::User { .. }
                        | Entry::Assistant(_)
                        | Entry::Agent { .. }
                        | Entry::Error(_)
                )
            })
            .collect::<Vec<_>>();
        let skip = messages.len().saturating_sub(READ_MOST);
        Ok(messages[skip..]
            .iter()
            .map(|item| views::message(&said, item))
            .collect())
    }

    fn delete_past(&mut self, name: &str) -> Result<(), Failure> {
        // A log in use belongs to an open conversation.
        let past = self
            .kept()
            .into_iter()
            .find(|past| {
                name_of(past) == name && past.number.is_some()
            })
            .ok_or(Failure::NotFound)?;
        archive::delete(&past.path)
            .map_err(|why| Failure::Failed(why.to_string()))
    }
}

/// The name of a past session's log in its folder.
fn name_of(past: &Kept) -> String {
    past.path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
