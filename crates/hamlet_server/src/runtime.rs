//! The serving state and its loop: the events of connections,
//! runtimes, tool calls and house work, each handled in turn; what
//! follows them sent to clients; and a failed one recovered from.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_protocol::client::{ServerMessage, State};
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::{AbortHandle, JoinError, JoinSet};

use crate::chats;
use crate::connection::{self, Client, Event, refuse, send};
use crate::harness::{self, Harnesses, Notice};
use crate::protocol::{Protocol, Refusable, fault, reply};
use crate::requests::{BEGIN, NotesWritten, PapersRead};
use crate::serve::Signals;
use crate::service::Service;

pub(crate) const FAILED: &str =
    "The server could not complete this request.";

/// The request of connection `client` a failed handler was
/// answering, attached to its error as context.
#[derive(Debug)]
pub(crate) struct Asked {
    pub(crate) client: u64,
    pub(crate) request: Option<String>,
}

impl std::fmt::Display for Asked {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match &self.request {
            Some(request) => write!(
                f,
                "answering request {request} of connection {}",
                self.client
            ),
            None => write!(f, "answering connection {}", self.client),
        }
    }
}

/// What the serve loop holds between the events it handles.
pub(crate) struct Serving {
    pub(crate) service: Service,
    pub(crate) harnesses: Harnesses,
    pub(crate) protocol: Protocol,
    pub(crate) clients: HashMap<u64, Client>,
    /// The state every welcomed client was last sent.
    pub(crate) state: State,
    /// The id of the next connection.
    pub(crate) next: u64,
    pub(crate) events: mpsc::Sender<Event>,
    pub(crate) closing: watch::Receiver<bool>,
    /// The connections' own tasks.
    pub(crate) tasks: JoinSet<()>,
    pub(crate) reads: JoinSet<PapersRead>,
    /// The reads running, by connection and request id.
    pub(crate) reading: HashMap<(u64, String), AbortHandle>,
    pub(crate) writes: JoinSet<NotesWritten>,
    /// Messages a busy connection held back, due now.
    pub(crate) ready: VecDeque<(u64, String)>,
}

/// The next message due: one a busy connection held back, else the
/// next event of a connection.
async fn next_event(
    ready: &mut VecDeque<(u64, String)>,
    incoming: &mut mpsc::Receiver<Event>,
) -> Option<Event> {
    match ready.pop_front() {
        Some((id, text)) => Some(Event::Message(id, text)),
        None => incoming.recv().await,
    }
}

impl Serving {
    /// Handles events until a signal; an error ends the loop only
    /// when it comes from the listener.
    pub(crate) async fn run(
        &mut self,
        signals: &mut Signals,
        listener: &TcpListener,
        incoming: &mut mpsc::Receiver<Event>,
    ) -> Result<()> {
        let mut tick =
            tokio::time::interval(Duration::from_millis(50));
        loop {
            // False when the event was dealt with in full, with
            // nothing to settle after it.
            let mut settles = true;
            let handled = tokio::select! {
                biased;
                _ = signals.next() => break,
                accepted = listener.accept() => {
                    self.on_accept(accepted?.0);
                    Ok(())
                }
                Some(event) = next_event(
                    &mut self.ready,
                    incoming,
                ) => {
                    self
                        .on_event(event)
                        .await
                        .map(|then| settles = then)
                }
                _ = tick.tick() => self.on_tick().await,
                event = self.harnesses.next() => {
                    self
                        .on_harness(event)
                        .await
                        .and_then(|()| self.refresh())
                }
                finished = self.service.chats.next() => {
                    self
                        .on_finished(finished)
                        .await
                        .and_then(|()| self.refresh())
                }
                Some(work) = self.service.house_work.next() => {
                    self
                        .on_work(work)
                        .await
                        .and_then(|()| self.refresh())
                }
                Some(merged) =
                    self.service.merges.tasks.join_next() => {
                    self
                        .on_merged(merged)
                        .await
                        .and_then(|()| self.refresh())
                }
                Some(moved) =
                    self.service.moving.tasks.join_next() => {
                    self.on_moved(moved)
                }
                Some(read) = self.reads.join_next() => {
                    self.on_read(read)
                }
                Some(written) = self.writes.join_next() => {
                    self.on_written(written)
                }
                Some(given) =
                    self.service.gifts.tasks.join_next() => {
                    self.on_given(given).await
                }
                Some(_) = self.tasks.join_next() => Ok(()),
            };
            let settled = handled.and_then(|()| {
                if settles { self.settle() } else { Ok(()) }
            });
            if let Err(error) = settled {
                self.recover(&error);
            }
        }
        Ok(())
    }

    fn on_accept(&mut self, stream: TcpStream) {
        let id = self.next;
        self.next += 1;
        let client = connection::accept(
            stream,
            id,
            self.events.clone(),
            self.closing.clone(),
            &mut self.tasks,
        );
        self.clients.insert(id, client);
    }

    /// Handles what a connection did. True when the loop should then
    /// settle: free merged connections and deliver what waits to be
    /// sent; false when the event was dealt with in full here.
    async fn on_event(&mut self, event: Event) -> Result<bool> {
        match event {
            Event::Gone(id) => self.on_gone(id).await?,
            Event::Late(id) => {
                if let Some(client) = self.clients.get_mut(&id)
                    && !client.welcomed
                    && !client.refused
                {
                    refuse(client, BEGIN, 1008)?;
                }
            }
            Event::Message(id, text) => {
                return self.on_message(id, text).await;
            }
        }
        Ok(true)
    }

    async fn on_gone(&mut self, id: u64) -> Result<()> {
        self.reading.retain(|(owner, _), task| {
            if *owner == id {
                task.abort();
                false
            } else {
                true
            }
        });
        self.service.merges.gone(id);
        self.clients.remove(&id);
        self.service.models.disconnect(id);
        self.service.chats.gone(id);
        self.service.gifts.gone(id);
        flush_models(&mut self.service, &mut self.harnesses).await
    }

    pub(crate) fn busy(&mut self, id: u64) {
        if let Some(client) = self.clients.get_mut(&id) {
            client.busy = true;
        }
    }

    /// Lets connection `id` take the messages it held back while
    /// busy. False when it is gone.
    pub(crate) fn free(&mut self, id: u64) -> bool {
        let Some(client) = self.clients.get_mut(&id) else {
            return false;
        };
        client.busy = false;
        self.ready
            .extend(client.waiting.drain(..).map(|text| (id, text)));
        true
    }

    /// Whether a merge or a gift of connection `id` is under way.
    pub(crate) fn working(&self, id: u64) -> bool {
        self.service.merges.busy(id)
            || self
                .service
                .gifts
                .working
                .iter()
                .any(|(owner, _)| *owner == id)
    }

    async fn on_tick(&mut self) -> Result<()> {
        use crate::society::{merging, reviews, votes};
        let service = &mut self.service;
        service.gifts.expire();
        service
            .house_work
            .retire(service.home.store().connection())?;
        votes::sweep(service, &mut self.harnesses).await?;
        reviews::sweep(service, &mut self.harnesses).await?;
        merging::lane::advance(service, &mut self.harnesses).await?;
        self.harnesses.refresh_tools().await;
        for result in self.service.models.expired() {
            for (id, answer) in
                self.service.provider_result(result)?
            {
                self.send_to(id, answer)?;
            }
        }
        flush_models(&mut self.service, &mut self.harnesses).await?;
        self.refresh()
    }

    pub(crate) async fn on_harness(
        &mut self,
        event: harness::Event,
    ) -> Result<()> {
        let notices = self
            .harnesses
            .handle(
                self.service.home.store(),
                event,
                &self.service.credentials,
            )
            .await?;
        self.heard(notices).await
    }

    pub(crate) async fn on_finished(
        &mut self,
        finished: chats::Finished,
    ) -> Result<()> {
        chats::finished(
            &mut self.service,
            &mut self.harnesses,
            finished,
        )
        .await?;
        self.deliver()
    }

    pub(crate) async fn on_work(
        &mut self,
        work: Result<crate::society::work::Completed, JoinError>,
    ) -> Result<()> {
        if let Ok(work) = work {
            crate::society::work::complete(
                &mut self.service,
                &mut self.harnesses,
                work,
            )
            .await?;
        }
        self.deliver()
    }

    pub(crate) async fn on_merged(
        &mut self,
        merged: Result<
            crate::society::merging::lane::Completed,
            JoinError,
        >,
    ) -> Result<()> {
        if let Ok(merged) = merged {
            crate::society::merging::lane::complete(
                &mut self.service,
                &mut self.harnesses,
                merged,
            )
            .await?;
        }
        Ok(())
    }

    fn on_moved(
        &mut self,
        moved: Result<(u64, String, Refusable<String>), JoinError>,
    ) -> Result<()> {
        if let Ok((owner, id, result)) = moved {
            self.send_to(owner, reply(&id, result))?;
        }
        self.refresh()
    }

    async fn on_given(
        &mut self,
        given: Result<crate::gifts::Completed, JoinError>,
    ) -> Result<()> {
        use crate::gifts::Completed;
        let Ok(given) = given else {
            return self.refresh();
        };
        let (Completed::Began { owner, id, .. }
        | Completed::Files { owner, id, .. }
        | Completed::Prepared { owner, id, .. }
        | Completed::Delivered { owner, id, .. }) = &given;
        let asked = Asked {
            client: *owner,
            request: Some(id.clone()),
        };
        let completed = crate::gifts::complete(
            &mut self.service,
            &mut self.harnesses,
            given,
        )
        .await
        .context(asked)?;
        if let Some((owner, answer)) = completed
            && self.free(owner)
        {
            self.send_to(owner, answer)?;
        }
        self.refresh()
    }

    /// Frees the connections whose merge finished and sends what the
    /// chats hold, after each event.
    fn settle(&mut self) -> Result<()> {
        for owner in std::mem::take(&mut self.service.merges.finished)
        {
            self.free(owner);
        }
        self.deliver()
    }

    /// Logs `error`, which ended the handling of one event, and
    /// leaves the loop as handling it in full would. The request it
    /// was answering gets a fault and no longer holds its connection.
    fn recover(&mut self, error: &anyhow::Error) {
        failed(&self.service, error);
        if let Some(asked) = error.downcast_ref::<Asked>() {
            let client = asked.client;
            if let Some(request) = &asked.request {
                self.service
                    .gifts
                    .working
                    .remove(&(client, request.clone()));
            }
            let faulted = match self.clients.get_mut(&client) {
                // Its connection is gone: dropped, not faulted.
                Some(connection) if connection.send.is_closed() => {
                    connection.refused = true;
                    Ok(())
                }
                Some(connection) => send(
                    connection,
                    &self.service,
                    fault(asked.request.as_deref(), FAILED),
                ),
                None => Ok(()),
            };
            if let Err(error) = faulted {
                failed(&self.service, &error);
            }
            if !self.working(client) {
                self.free(client);
            }
        }
        for done in [self.refresh(), self.settle()] {
            if let Err(error) = done {
                failed(&self.service, &error);
            }
        }
    }

    /// Sends `message` to connection `id`, if it is still open.
    pub(crate) fn send_to(
        &mut self,
        id: u64,
        message: ServerMessage,
    ) -> Result<()> {
        match self.clients.get_mut(&id) {
            Some(client) => send(client, &self.service, message),
            None => Ok(()),
        }
    }

    /// Takes in what the runtimes told, and sends clients what
    /// follows.
    async fn heard(&mut self, notices: Vec<Notice>) -> Result<()> {
        for notice in notices {
            match notice {
                Notice::ProviderEvent(event) => {
                    for (id, answer) in
                        self.service.provider_event(event)?
                    {
                        self.send_to(id, answer)?;
                    }
                }
                Notice::Provider(result) => {
                    for (id, answer) in
                        self.service.provider_result(result)?
                    {
                        self.send_to(id, answer)?;
                    }
                }
                notice => {
                    chats::heard(
                        &mut self.service,
                        &mut self.harnesses,
                        notice,
                    )
                    .await?;
                }
            }
            self.deliver()?;
        }
        flush_models(&mut self.service, &mut self.harnesses).await
    }

    /// Sends the messages the chats hold for clients.
    pub(crate) fn deliver(&mut self) -> Result<()> {
        for (id, message) in
            std::mem::take(&mut self.service.chats.outbox)
        {
            if let Some(client) = self
                .clients
                .get_mut(&id)
                .filter(|c| c.welcomed && !c.refused)
            {
                send(client, &self.service, message)?;
            }
        }
        Ok(())
    }

    /// Sends welcomed clients what changed in the state since they
    /// were last sent it.
    pub(crate) fn refresh(&mut self) -> Result<()> {
        let mut after = self
            .service
            .state(self.state.epoch, self.state.revision)?;
        let mut endings = HashMap::new();
        for wait in &self.state.waits {
            if !after.waits.iter().any(|w| w.id == wait.id) {
                let db = self.service.home.store().connection();
                endings.insert(
                    wait.id.clone(),
                    crate::oversight::ended(db, &wait.id)?,
                );
            }
        }
        let ending = |id: &str| {
            endings
                .get(id)
                .cloned()
                .unwrap_or_else(|| "lapsed".into())
        };
        if let Some(change) =
            crate::state::changed(&self.state, &mut after, ending)
        {
            for client in self
                .clients
                .values_mut()
                .filter(|c| c.welcomed && !c.refused)
            {
                send(client, &self.service, change.clone())?;
            }
            self.state = after;
        }
        // Gone follows the client's last admitted message, even when
        // it no longer reads replies.
        self.clients.retain(|_, c| !c.refused);
        Ok(())
    }
}

pub(crate) async fn flush_models(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    for request in std::mem::take(&mut service.models.outgoing) {
        harnesses.provider(service.home.store(), request).await?;
    }
    for control in std::mem::take(&mut service.models.controls) {
        harnesses.provider_control(control).await;
    }
    harnesses
        .credentials(service.home.store(), &mut service.credentials)
        .await?;
    Ok(())
}

#[expect(clippy::print_stderr, reason = "stderr is the server's log")]
pub(crate) fn failed(service: &Service, error: &anyhow::Error) {
    let mut diagnostic = json!(format!("hamlet-server: {error:#}"));
    service.credentials.scrub(&mut diagnostic);
    eprintln!("{diagnostic}");
}
