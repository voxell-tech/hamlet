//! The serving state and its loop: the events of connections,
//! runtimes, tool calls and house work, each handled in turn; what
//! follows them sent to clients; and a failed one recovered from.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_protocol::client::{RequestId, ServerMessage, State};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::{AbortHandle, JoinError, JoinSet};

use crate::chats;
use crate::connection::{self, Client, Event, refuse, send};
use crate::diagnostic::{Secrets, tell};
use crate::harness::{self, Harnesses, Notice};
use crate::protocol::{Protocol, Refusable, fault, reply};
use crate::requests::{BEGIN, NotesWritten, PapersRead};
use crate::serve::Signals;
use crate::service::Service;

#[cfg(test)]
mod tests;

pub(crate) const FAILED: &str =
    "The server could not complete this request.";

/// How often the tools folder is read again for what a chief changed
/// in it by hand, to send ready runtimes the tools that changed.
const RESCAN: Duration = Duration::from_secs(2);

/// The request of connection `client` a failed handler was
/// answering, attached to its error as context.
#[derive(Debug)]
pub(crate) struct Asked {
    pub(crate) client: u64,
    pub(crate) request: Option<RequestId>,
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

/// Whether a connection [`Serving::free`] was asked to free is still
/// there.
pub(crate) enum Freed {
    Here,
    Gone,
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
    pub(crate) reading: HashMap<(u64, RequestId), AbortHandle>,
    pub(crate) writes: JoinSet<NotesWritten>,
    /// Messages a busy connection held back, due now.
    pub(crate) ready: VecDeque<(u64, String)>,
}

/// What a tick knows of the state since it last built it, so an idle
/// tick neither builds it nor reads the store.
///
/// What `state()` reads changes through the store, the clock, files
/// edited by hand, or the owners in memory: the service's settings,
/// chief, credentials, houses, models, chats and house move. The
/// state may have changed since a tick built it when an event was
/// handled, since the store and the owners change only in the loop
/// while it handles one; when the tick's own steps found work or
/// changed a row; when the clock passed `turns`, where a road
/// overgrows or a budget's share gone moves or its period ends; when
/// the skills in the library or a house differ from the state's; when
/// a house move's task sent other progress; or when a provider's
/// answer is overdue.
struct Since {
    /// An event was handled.
    stale: bool,
    /// The steps of the last tick found work, or changed a row.
    busy: bool,
    /// When a view of the state next changes with the clock alone.
    turns: i64,
}

impl Since {
    /// Nothing known yet: the first tick builds the state.
    fn new() -> Self {
        Self {
            stale: true,
            busy: false,
            turns: 0,
        }
    }
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
        let mut rescan = tokio::time::interval(RESCAN);
        let mut since = Since::new();
        loop {
            // False when the event was dealt with in full, with
            // nothing to settle after it.
            let mut settles = true;
            // True for the timers, whose turn handles no event.
            let mut timed = false;
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
                _ = tick.tick() => {
                    timed = true;
                    self.on_tick(&mut since).await
                }
                _ = rescan.tick() => {
                    timed = true;
                    self.harnesses.refresh_tools().await;
                    Ok(())
                }
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
                Some(printed) = self.service.terminals.next() => {
                    // Output alone changes no state; an end or a
                    // title refreshes it here.
                    timed = true;
                    self.on_printed(printed)
                }
                Some(_) = self.tasks.join_next() => Ok(()),
            };
            since.stale |= !timed;
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
        self.reading.retain(|(connection, _), task| {
            if *connection == id {
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
        self.service.terminals.gone(id);
        flush_models(&mut self.service, &mut self.harnesses).await
    }

    pub(crate) fn busy(&mut self, id: u64) {
        if let Some(client) = self.clients.get_mut(&id) {
            client.busy = true;
        }
    }

    /// Lets connection `id` take the messages it held back while
    /// busy, if it is still there.
    pub(crate) fn free(&mut self, id: u64) -> Freed {
        let Some(client) = self.clients.get_mut(&id) else {
            return Freed::Gone;
        };
        client.busy = false;
        self.ready
            .extend(client.waiting.drain(..).map(|text| (id, text)));
        Freed::Here
    }

    /// Whether a merge or a gift of connection `id` is under way.
    pub(crate) fn is_working(&self, id: u64) -> bool {
        self.service.merges.is_busy(id)
            || self
                .service
                .gifts
                .working
                .iter()
                .any(|(connection, _)| *connection == id)
    }

    /// Moves on the house work, votes, reviews and merges, answers
    /// overdue provider requests and sends what changed, unless
    /// `since` and the skills on disk show nothing could have. The
    /// skills on disk are compared on every tick, so one a chief puts
    /// in a house or the library by hand shows within a tick.
    async fn on_tick(&mut self, since: &mut Since) -> Result<()> {
        use crate::society::{merging, reviews, votes};
        let clients = &self.clients;
        let caught = self.service.terminals.catch_up(|id| {
            clients.get(&id).map_or(0, |c| c.send.capacity())
        });
        for (id, message) in caught {
            self.send_to(id, message)?;
        }
        self.service.gifts.expire();
        let expired = self.service.models.expired();
        let due = since.stale
            || since.busy
            || !expired.is_empty()
            || crate::clock::now() >= since.turns
            || self.service.moving.view()
                != self.state.settings.workspace.moving;
        if !due
            && !crate::state::skills_moved(
                &mut self.service,
                &self.state,
            )?
        {
            return Ok(());
        }
        let service = &mut self.service;
        let before = service.home.store().changes();
        service.house_work.retire(service.home.connection())?;
        let mut busy =
            votes::sweep(service, &mut self.harnesses).await?;
        busy |= reviews::sweep(service, &mut self.harnesses).await?;
        busy |= merging::lane::advance(service, &mut self.harnesses)
            .await?;
        for result in expired {
            for (id, answer) in
                self.service.provider_result(result)?
            {
                self.send_to(id, answer)?;
            }
        }
        flush_models(&mut self.service, &mut self.harnesses).await?;
        busy |= self.service.home.store().changes() != before;
        since.busy = busy;
        // A rebuild that fails is tried again on the next tick.
        since.stale = true;
        since.turns = self.rebuild()?;
        since.stale = false;
        Ok(())
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
        moved: Result<(u64, RequestId, Refusable<String>), JoinError>,
    ) -> Result<()> {
        if let Ok((connection, id, result)) = moved {
            self.send_to(connection, reply(&id, result))?;
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
        let (Completed::Began { connection, id, .. }
        | Completed::Files { connection, id, .. }
        | Completed::Prepared { connection, id, .. }
        | Completed::Delivered { connection, id, .. }) = &given;
        let asked = Asked {
            client: *connection,
            request: Some(id.clone()),
        };
        let completed = crate::gifts::complete(
            &mut self.service,
            &mut self.harnesses,
            given,
        )
        .await
        .context(asked)?;
        if let Some((connection, answer)) = completed
            && let Freed::Here = self.free(connection)
        {
            self.send_to(connection, answer)?;
        }
        self.refresh()
    }

    /// Sends the terminals' output to the connections that watch
    /// them, with what else was printed by now.
    fn on_printed(
        &mut self,
        first: crate::terminals::Printed,
    ) -> Result<()> {
        let mut printed = vec![first];
        while let Some(more) = self.service.terminals.more() {
            printed.push(more);
        }
        let clients = &self.clients;
        let (out, changed) =
            self.service.terminals.printed(printed, |id| {
                clients.get(&id).map_or(0, |c| c.send.capacity())
            });
        for (id, message) in out {
            self.send_to(id, message)?;
        }
        if changed { self.refresh() } else { Ok(()) }
    }

    /// Frees the connections whose merge finished and sends what the
    /// chats hold, after each event.
    fn settle(&mut self) -> Result<()> {
        for connection in
            std::mem::take(&mut self.service.merges.finished)
        {
            self.free(connection);
        }
        self.deliver()
    }

    /// Logs `error`, a defect, with its context chain.
    pub(crate) fn log_defect(&self, error: &anyhow::Error) {
        tell(
            Secrets::Of(&self.service.credentials),
            format_args!("{error:#}"),
        );
    }

    /// Logs `error`, which ended the handling of one event, and
    /// leaves the loop as handling it in full would. The request it
    /// was answering gets a fault and no longer holds its connection.
    fn recover(&mut self, error: &anyhow::Error) {
        self.log_defect(error);
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
                    fault(asked.request.as_ref(), FAILED),
                ),
                None => Ok(()),
            };
            if let Err(error) = faulted {
                self.log_defect(&error);
            }
            if !self.is_working(client) {
                self.free(client);
            }
        }
        for done in [self.refresh(), self.settle()] {
            if let Err(error) = done {
                self.log_defect(&error);
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
        self.rebuild().map(drop)
    }

    /// Refreshes, and gives the instant one of the state's views
    /// next changes with the clock alone.
    fn rebuild(&mut self) -> Result<i64> {
        let built = crate::state::build(
            &mut self.service,
            self.state.epoch,
            self.state.revision,
        )?;
        let mut after = built.state;
        let agents = after
            .agents
            .iter()
            .filter(|agent| agent.left.is_none())
            .map(|agent| agent.agent.clone())
            .collect::<Vec<_>>();
        let hamlets = after
            .hamlets
            .iter()
            .map(|hamlet| hamlet.id.clone())
            .collect::<Vec<_>>();
        if self.service.terminals.prune(&agents, &hamlets) {
            after.terminals = self.service.terminals.views();
        }
        let mut endings = HashMap::new();
        for wait in &self.state.waits {
            if !after.waits.iter().any(|w| w.id == wait.id) {
                let db = self.service.db();
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
        Ok(built.turns)
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
