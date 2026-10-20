//! The link's own bookkeeping: ids of the server's messages, the
//! requests awaiting a runtime's reply, and reading a runtime's
//! lines.

use std::collections::HashMap;

use serde_json::Value;

use super::link::{self, LinkError, RuntimeMessage, ServerMessage};
use super::{BindingId, Harnesses, LINK_VERSION};

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

/// The server's end of the links: its message ids and the requests in
/// flight, by the runtime they went to.
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
    /// Sends the message `make` builds from the link version and a
    /// new id; returns the id.
    pub(super) async fn send(
        &mut self,
        index: usize,
        make: impl FnOnce(i64, String) -> ServerMessage,
    ) -> String {
        let id = self.session.id();
        let message = make(LINK_VERSION, id.clone());
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
        make: impl FnOnce(i64, String) -> ServerMessage,
    ) {
        let id = self.send(index, make).await;
        self.session.pending.insert(id, (index, pending));
    }

    pub(super) async fn error(
        &mut self,
        index: usize,
        re: Option<String>,
        code: link::ErrorCode,
        words: &str,
    ) {
        let message = words.to_owned();
        self.send(index, |v, id| {
            ServerMessage::Error(LinkError {
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

/// A runtime's line as a link message, or the words of why it is none
/// with the id it carried.
pub(super) fn parse(
    line: &str,
) -> Result<RuntimeMessage, (String, Option<String>)> {
    let value = serde_json::from_str::<Value>(line)
        .map_err(|error| (format!("Not JSON: {error}."), None))?;
    let re = value["id"].as_str().map(str::to_owned);
    if value["v"] != LINK_VERSION {
        return Err((
            format!("Link messages here are version {LINK_VERSION}."),
            re,
        ));
    }
    let message =
        serde_json::from_value::<RuntimeMessage>(value.clone())
            .map_err(|error| {
                (format!("Not a link message: {error}."), re.clone())
            })?;
    // Written back, a field the link does not name is missing.
    if !serde_json::to_value(&message)
        .is_ok_and(|back| same_fields(&back, &value))
    {
        return Err((
            "The message has fields the link does not name.".into(),
            re,
        ));
    }
    Ok(message)
}

/// Whether `a` and `b` have the same fields at every depth; numbers
/// may be written differently.
fn same_fields(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, a)| {
                    b.get(key).is_some_and(|b| same_fields(a, b))
                })
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| same_fields(a, b))
        }
        (Value::Number(_), Value::Number(_)) => true,
        (a, b) => a == b,
    }
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
