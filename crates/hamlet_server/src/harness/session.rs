//! The runtime protocol's own bookkeeping: ids of the server's
//! messages, the requests awaiting a runtime's reply, and reading a
//! runtime's lines.

use std::collections::HashMap;

use serde_json::Value;

use super::runtime::{
    self, ProtocolError, RuntimeMessage, ServerMessage,
};
use super::{BindingId, Harnesses};

/// What a request to a runtime was for, until its reply.
pub(super) enum Pending {
    Create(BindingId),
    Submit(BindingId, String),
    Cancel(BindingId),
    History(BindingId),
    /// A page read to catch up on entries kept while the server did
    /// not hear of them.
    Sync(BindingId),
    /// The tools a conversation offers, as they were told.
    Configure(BindingId, String),
}

/// The server's end of the runtime protocol: its message ids and the
/// requests in flight, by the runtime they went to.
#[derive(Default)]
pub(super) struct Session {
    next: u64,
    pending: HashMap<String, (usize, Pending)>,
}

impl Session {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("s{}", self.next)
    }

    /// Takes the request `re` answers, if one is in flight.
    pub fn reply(&mut self, re: &str) -> Option<Pending> {
        self.pending.remove(re).map(|(_, pending)| pending)
    }

    /// Whether a request of runtime `index` that `which` matches is
    /// in flight.
    pub fn in_flight(
        &self,
        index: usize,
        which: impl Fn(&Pending) -> bool,
    ) -> bool {
        self.pending
            .values()
            .any(|(at, pending)| *at == index && which(pending))
    }

    /// Drops the requests in flight to runtime `index`, which ended.
    pub fn forget(&mut self, index: usize) {
        self.pending.retain(|_, (at, _)| *at != index);
    }

    #[cfg(test)]
    pub fn expect(
        &mut self,
        id: &str,
        index: usize,
        pending: Pending,
    ) {
        self.pending.insert(id.into(), (index, pending));
    }
}

impl Harnesses {
    /// Sends the message `make` builds from the protocol version and
    /// a new id; returns the id.
    pub(super) async fn send(
        &mut self,
        index: usize,
        make: impl FnOnce(u64, String) -> ServerMessage,
    ) -> String {
        let id = self.session.id();
        let message = make(runtime::VERSION, id.clone());
        if let (Some(process), Ok(line)) = (
            self.runtimes[index].process.as_mut(),
            serde_json::to_string(&message),
        ) {
            process.write(&line).await;
        }
        id
    }

    /// Sends a request whose reply `pending` awaits.
    pub(super) async fn request(
        &mut self,
        index: usize,
        pending: Pending,
        make: impl FnOnce(u64, String) -> ServerMessage,
    ) {
        let id = self.send(index, make).await;
        self.session.pending.insert(id, (index, pending));
    }

    pub(super) async fn error(
        &mut self,
        index: usize,
        re: Option<String>,
        code: runtime::ErrorCode,
        words: &str,
    ) {
        let message = words.to_owned();
        self.send(index, |v, id| {
            ServerMessage::Error(ProtocolError {
                code,
                id,
                message,
                re,
                v,
            })
        })
        .await;
    }
}

/// A runtime's line as a protocol message, or the words of why it is
/// none with the id it carried.
pub(super) fn parse(
    line: &str,
) -> Result<RuntimeMessage, (String, Option<String>)> {
    let value = serde_json::from_str::<Value>(line)
        .map_err(|error| (format!("Not JSON: {error}."), None))?;
    let re = value["id"].as_str().map(str::to_owned);
    if value["v"] != runtime::VERSION {
        return Err((
            format!(
                "Protocol messages here are version {}.",
                runtime::VERSION
            ),
            re,
        ));
    }
    serde_json::from_value::<RuntimeMessage>(value).map_err(|error| {
        (format!("Not a protocol message: {error}."), re)
    })
}

pub(super) fn id_of(message: &RuntimeMessage) -> String {
    match message {
        RuntimeMessage::ProviderResult(m) => m.id.clone(),
        RuntimeMessage::ProviderEvent(m) => m.id.clone(),
        RuntimeMessage::CredentialRequest(m) => m.id.clone(),
        RuntimeMessage::Hello(m) => m.id.clone(),
        RuntimeMessage::Created(m) => m.id.clone(),
        RuntimeMessage::Admitted(m) => m.id.clone(),
        RuntimeMessage::Cancelled(m) => m.id.clone(),
        RuntimeMessage::Configured(m) => m.id.clone(),
        RuntimeMessage::Page(m) => m.id.clone(),
        RuntimeMessage::Statuses(m) => m.id.clone(),
        RuntimeMessage::Allow(m) => m.id.clone(),
        RuntimeMessage::Operation(m) => m.id.clone(),
        RuntimeMessage::Attempt(m) => m.id.clone(),
        RuntimeMessage::Usage(m) => m.id.clone(),
        RuntimeMessage::Scrub(m) => m.id.clone(),
        RuntimeMessage::Prompt(m) => m.id.clone(),
        RuntimeMessage::Kept(m) => m.id.clone(),
        RuntimeMessage::Live(m) => m.id.clone(),
        RuntimeMessage::Settled(m) => m.id.clone(),
        RuntimeMessage::Yield(m) => m.id.clone(),
        RuntimeMessage::Error(m) => m.id.clone(),
    }
}
