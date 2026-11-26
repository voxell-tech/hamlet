//! The reply a client is owed for its line to an agent, held until
//! the agent's work on the line ends.

use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{RequestId, ServerMessage};
use tokio::sync::oneshot;
use tokio::task::AbortHandle;

use super::{Hub, Job, Mark};

/// The reply of a line a client is owed.
pub(super) struct Owed {
    pub(super) client: u64,
    pub(super) id: RequestId,
    pub(super) taken: Taken,
    /// The task that reads what became of the line, aborted when
    /// the reply is no longer owed.
    pub(super) reading: Option<AbortHandle>,
}

impl Drop for Owed {
    fn drop(&mut self) {
        if let Some(reading) = &self.reading {
            reading.abort();
        }
    }
}

pub(super) enum Taken {
    /// The realm has not said the agent took the line.
    Not,
    /// It took it: the first idle status after this change ends its
    /// work, or any idle status when the change is not known.
    After(Option<Mark>),
}

impl Hub {
    /// Owes `client` the reply of its line `id` to `agent`, unless
    /// its connection closed. The receiver resolves once `agent`'s
    /// chat is followed, so no change after it is missed, and fails
    /// when the hub answered the line already.
    pub(crate) fn owe(
        &self,
        client: u64,
        agent: AgentNumber,
        id: &RequestId,
    ) -> Option<oneshot::Receiver<()>> {
        let shared = &self.shared;
        let mut followed = shared.followed();
        if !shared.clients().contains(&client) {
            return None;
        }
        let (held, holds) = oneshot::channel();
        let found = shared.follow(&mut followed, agent);
        found.owed.push(Owed {
            client,
            id: id.clone(),
            taken: Taken::Not,
            reading: None,
        });
        let _ = found.jobs.send(Job::Held(held));
        Some(holds)
    }

    /// Keeps `reading`, the task that reads what became of the line
    /// `id` of `client` to `agent`, while its reply is owed; aborts
    /// it when it is not.
    pub(crate) fn reading(
        &self,
        client: u64,
        agent: AgentNumber,
        id: &RequestId,
        reading: AbortHandle,
    ) {
        let mut followed = self.shared.followed();
        let owed = followed.get_mut(&agent).and_then(|found| {
            found
                .owed
                .iter_mut()
                .find(|owed| owed.client == client && owed.id == *id)
        });
        match owed {
            Some(owed) => owed.reading = Some(reading),
            None => reading.abort(),
        }
    }

    /// Takes in that `agent` took the line `id` of `client`: reads
    /// the chief's chat to learn where the agent's work on it stands.
    pub(crate) async fn taken(
        &self,
        client: u64,
        agent: AgentNumber,
        id: &RequestId,
    ) {
        let read = self.shared.current(agent).await.ok();
        let shared = &self.shared;
        let mut followed = shared.followed();
        let Some(found) = followed.get_mut(&agent) else {
            return;
        };
        let Some(at) = found
            .owed
            .iter()
            .position(|owed| owed.client == client && owed.id == *id)
        else {
            return;
        };
        let ended = match (read, found.status) {
            (Some((_, true)), _) => true,
            (Some((mark, false)), Some((then, idle))) => {
                idle && then.after(mark)
            }
            _ => false,
        };
        if ended {
            let owed = found.owed.remove(at);
            shared.settle(&owed);
            let _ = found.jobs.send(Job::Look);
        } else {
            found.owed[at].taken =
                Taken::After(read.map(|(mark, _)| mark));
        }
    }

    /// Answers the line `id` of `client` to `agent` with `message`,
    /// unless it was answered.
    pub(crate) fn settle(
        &self,
        agent: AgentNumber,
        client: u64,
        id: &RequestId,
        message: ServerMessage,
    ) {
        let mut followed = self.shared.followed();
        let Some(found) = followed.get_mut(&agent) else {
            return;
        };
        let before = found.owed.len();
        found.owed.retain(|owed| {
            !(owed.client == client && owed.id == *id)
        });
        if found.owed.len() < before {
            self.shared.send(client, message);
        }
        let _ = found.jobs.send(Job::Look);
    }
}
