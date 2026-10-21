//! The loopback listener, published address and ordered shutdown.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::{SinkExt, StreamExt};
use hamlet_protocol::v0::{
    Address, ClientMessage, Edit, MoveHouses, Read, ServerMessage,
    State,
};
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::{AbortHandle, JoinError, JoinSet};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{
    CloseFrame, WebSocketConfig,
};
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::credentials::write_whole;
use crate::harness::{self, Harnesses, InputStatus, Notice};
use crate::protocol::{Protocol, Refusable, Refusal, fault, reply};
use crate::service::Service;
use crate::{Home, chats};

const LARGEST: usize = 1_048_576;
const BEGIN: &str =
    "A connection begins with hello and the home's token.";
const FAILED: &str = "The server could not complete this request.";
const GRACE: Duration = Duration::from_secs(1);
/// How long a closing home waits for the reflections it owes.
const REFLECTING: Duration = Duration::from_secs(120);
/// How long a closing home waits for its runtimes to stop.
const STOPPING: Duration = Duration::from_secs(30);

/// The server start contract, including the script passed to its
/// harness.
pub struct Options {
    pub home: PathBuf,
    pub port: u16,
    pub faux: Option<PathBuf>,
}

impl Options {
    pub fn parse(
        mut args: impl Iterator<Item = OsString>,
    ) -> Result<Self> {
        let mut home = std::env::var_os("HAMLET_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        let mut port = 7313;
        let mut faux = None;
        while let Some(flag) = args.next() {
            let value = args.next().context(
                "usage: hamlet-server [--home <dir>] [--port <n>] \
                 [--faux <file>]",
            )?;
            match flag.to_str() {
                Some("--home") => home = Some(value.into()),
                Some("--port") => {
                    port = value
                        .to_str()
                        .context("port must be a number")?
                        .parse()
                        .context("port must be 0 to 65535")?
                }
                Some("--faux") => faux = Some(value.into()),
                _ => bail!(
                    "usage: hamlet-server [--home <dir>] [--port \
                     <n>] [--faux <file>]"
                ),
            }
        }
        let home = home
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|p| PathBuf::from(p).join(".hamlet"))
            })
            .context("set --home or HAMLET_HOME")?;
        Ok(Self { home, port, faux })
    }
}

enum Event {
    Message(u64, String),
    Late(u64),
    Gone(u64),
}

enum Out {
    Message(String),
    Close(u16),
}

struct Client {
    send: mpsc::Sender<Out>,
    welcomed: bool,
    refused: bool,
    busy: bool,
    waiting: VecDeque<String>,
}

struct Published(PathBuf);
impl Drop for Published {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Serves until SIGTERM or Ctrl-C, closing every client before
/// releasing the home.
pub async fn serve(options: Options) -> Result<()> {
    let mut signals = Signals::new()?;
    let home = Home::open(&options.home)?;
    let mut service = Service::new(home, options.faux.is_some())?;
    service.houses.probe(service.chats.sandbox)?;
    tokio::select! {
        _ = signals.next() => return service.home.close(),
        initialized = service.initialize_houses() => initialized?,
    }
    service.prepare_gifts()?;
    chats::load(&mut service)?;
    let layout = service.home.layout().clone();
    let (mut harnesses, notices) = Harnesses::start(
        &layout,
        service.home.store(),
        harness::Config::from_env(options.faux.clone())?,
    )?;
    for notice in notices {
        tokio::select! {
            _ = signals.next() => {
                harnesses.close().await;
                return service.home.close();
            }
            result = chats::heard(
                &mut service,
                &mut harnesses,
                notice,
            ) => result?,
        }
    }
    service.start_models()?;
    flush_models(&mut service, &mut harnesses).await?;
    let started = async {
        while service.models.starting() {
            let event = harnesses.next().await;
            for notice in harnesses
                .handle(
                    service.home.store(),
                    event,
                    &service.credentials,
                )
                .await?
            {
                match notice {
                    Notice::Provider(result) => {
                        service.provider_result(result)?;
                    }
                    notice => {
                        chats::heard(
                            &mut service,
                            &mut harnesses,
                            notice,
                        )
                        .await?
                    }
                }
            }
            flush_models(&mut service, &mut harnesses).await?;
        }
        Ok::<_, anyhow::Error>(())
    };
    let started =
        tokio::time::timeout(Duration::from_secs(15), started);
    tokio::select! {
        _ = signals.next() => {
            harnesses.close().await;
            return service.home.close();
        }
        result = started => {
            result.context("starting model providers")??
        }
    }
    tokio::select! {
        _ = signals.next() => {
            harnesses.close().await;
            return service.home.close();
        }
        result = chats::start(&mut service, &mut harnesses) => {
            result?
        }
    }
    let protocol = Protocol::new()?;
    let listener = TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        options.port,
    ))
    .await?;
    let url = format!("ws://{}", listener.local_addr()?);
    let published = service.home.layout().root().join("server.json");
    let started = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let address = json!({
        "url": url,
        "pid": std::process::id(),
        "started": started,
    });
    write_whole(&published, &serde_json::to_vec(&address)?)?;
    let published = Published(published);
    eprintln!(
        "hamlet-server: holding {}; serving at {url}; Ctrl-C stops",
        service.home.layout().root().display()
    );
    let (events, mut incoming) = mpsc::channel(128);
    let (shutdown, closing) = watch::channel(false);
    let epoch = crate::clock::now();
    let state = service.state(epoch, 0)?;
    let mut serving = Serving {
        service,
        harnesses,
        protocol,
        clients: HashMap::new(),
        state,
        next: 0,
        events,
        closing,
        tasks: JoinSet::new(),
        reads: JoinSet::new(),
        reading: HashMap::new(),
        writes: JoinSet::new(),
        ready: VecDeque::new(),
    };
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let result = async {
        loop {
            tokio::select! {
                biased;
                _ = signals.next() => break,
                accepted = listener.accept() => {
                    serving.on_accept(accepted?.0)
                }
                Some(event) = next_event(
                    &mut serving.ready,
                    &mut incoming,
                ) => {
                    if !serving.on_event(event).await? {
                        continue;
                    }
                }
                _ = tick.tick() => serving.on_tick().await?,
                event = serving.harnesses.next() => {
                    serving.on_harness(event).await?;
                    serving.refresh()?;
                }
                finished = serving.service.chats.next() => {
                    serving.on_finished(finished).await?;
                    serving.refresh()?;
                }
                Some(work) = serving.service.house_work.next() => {
                    serving.on_work(work).await?;
                    serving.refresh()?;
                }
                Some(merged) =
                    serving.service.merges.tasks.join_next() => {
                    serving.on_merged(merged).await?;
                    serving.refresh()?;
                }
                Some(moved) =
                    serving.service.moving.tasks.join_next() => {
                    serving.on_moved(moved)?
                }
                Some(read) = serving.reads.join_next() => {
                    serving.on_read(read)?
                }
                Some(written) = serving.writes.join_next() => {
                    serving.on_written(written)?
                }
                Some(given) =
                    serving.service.gifts.tasks.join_next() => {
                    serving.on_given(given).await?
                }
                Some(_) = serving.tasks.join_next() => {}
            }
            serving.settle()?;
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    drop(listener);
    serving.reads.abort_all();
    serving.writes.abort_all();
    let result = match result {
        Ok(()) => serving.close(&mut signals).await,
        result => result,
    };
    serving.service.chats.stop_tools();
    shutdown.send_replace(true);
    let tasks = &mut serving.tasks;
    let drained =
        async { while tasks.join_next().await.is_some() {} };
    tokio::select! {
        _ = signals.next() => std::process::exit(130),
        _ = tokio::time::timeout(GRACE, drained) => {},
    }
    serving.tasks.abort_all();
    while serving.tasks.join_next().await.is_some() {}
    serving.harnesses.close().await;
    drop(published);
    serving.service.home.close()?;
    result
}

/// What the serve loop holds between the events it handles.
struct Serving {
    service: Service,
    harnesses: Harnesses,
    protocol: Protocol,
    clients: HashMap<u64, Client>,
    /// The state every welcomed client was last sent.
    state: State,
    /// The id of the next connection.
    next: u64,
    events: mpsc::Sender<Event>,
    closing: watch::Receiver<bool>,
    /// The connections' own tasks.
    tasks: JoinSet<()>,
    reads: JoinSet<PapersRead>,
    /// The reads running, by connection and request id.
    reading: HashMap<(u64, String), AbortHandle>,
    writes: JoinSet<NotesWritten>,
    /// Messages a busy connection held back, due now.
    ready: VecDeque<(u64, String)>,
}

/// A read of an agent's papers or prompt, rendered off the loop.
struct PapersRead {
    owner: u64,
    request: String,
    address: Address,
    /// The agent's number when the read was prepared; unset when it
    /// was refused then.
    number: Option<u64>,
    answer: Result<ServerMessage>,
}

/// A write of an agent's NOTES.md, done off the loop.
struct NotesWritten {
    owner: u64,
    request: String,
    result: Result<()>,
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
    fn on_accept(&mut self, stream: TcpStream) {
        let (send, receive) = mpsc::channel(128);
        let id = self.next;
        self.next += 1;
        self.clients.insert(
            id,
            Client {
                send,
                welcomed: false,
                refused: false,
                busy: false,
                waiting: VecDeque::new(),
            },
        );
        self.tasks.spawn(connection(
            stream,
            id,
            self.events.clone(),
            receive,
            self.closing.clone(),
        ));
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
        flush_models(&mut self.service, &mut self.harnesses).await?;
        self.service.chats.gone(id);
        self.service.gifts.gone(id);
        Ok(())
    }

    /// Takes one message of connection `id`: held while the
    /// connection is busy, refused or welcomed before its hello,
    /// else answered. False when the message was dealt with in
    /// full here.
    async fn on_message(
        &mut self,
        id: u64,
        text: String,
    ) -> Result<bool> {
        self.refresh()?;
        let Some(client) =
            self.clients.get_mut(&id).filter(|c| !c.refused)
        else {
            return Ok(false);
        };
        if client.busy {
            if client.waiting.len() == 128 {
                refuse(
                    client,
                    "Too many requests are waiting on this \
                     connection.",
                    1008,
                )?;
            } else {
                client.waiting.push_back(text);
            }
            return Ok(false);
        }
        let message = match self.protocol.parse(&text) {
            Ok(message) => message,
            Err(mut error) => {
                if !client.welcomed {
                    error.to = None;
                }
                send(
                    client,
                    &self.service,
                    ServerMessage::Fault(error),
                )?;
                if !client.welcomed {
                    client.refused = true;
                    let _ = client.send.try_send(Out::Close(1008));
                }
                return Ok(false);
            }
        };
        if client.welcomed {
            return self.answer(id, message).await;
        }
        match message {
            ClientMessage::Hello(hello)
                if self.service.credentials.accepts(&hello.token) =>
            {
                let state = ServerMessage::State(self.state.clone());
                send(client, &self.service, state)?;
                client.welcomed = true;
            }
            ClientMessage::Hello(_) => refuse(
                client,
                "That is not this server's token.",
                1008,
            )?,
            _ => refuse(client, BEGIN, 1008)?,
        }
        Ok(false)
    }

    /// Answers a welcomed client's request, or starts the work that
    /// will. True, as [`Self::on_event`], when the loop should then
    /// settle.
    async fn answer(
        &mut self,
        id: u64,
        message: ClientMessage,
    ) -> Result<bool> {
        let request = serde_json::to_value(&message)?;
        let handled = chats::converse(
            &mut self.service,
            &mut self.harnesses,
            id,
            &message,
        )
        .await;
        match handled {
            Ok(chats::Handled::NotMine) => {}
            Ok(handled) => {
                let busy = self.service.merges.busy(id)
                    || self
                        .service
                        .gifts
                        .working
                        .iter()
                        .any(|(owner, _)| *owner == id);
                if busy
                    && let Some(client) = self.clients.get_mut(&id)
                {
                    client.busy = true;
                }
                if let chats::Handled::Reply(answer) = handled {
                    self.send_to(id, *answer)?;
                }
                self.deliver()?;
                self.refresh()?;
                return Ok(false);
            }
            Err(error) => {
                failed(&self.service, &error);
                self.send_to(
                    id,
                    fault(request["id"].as_str(), FAILED),
                )?;
                self.deliver()?;
                self.refresh()?;
                return Ok(false);
            }
        }
        if let Some(answers) =
            self.service.model_request(id, &message)?
        {
            for answer in answers {
                self.send_to(id, answer)?;
            }
            self.refresh()?;
            flush_models(&mut self.service, &mut self.harnesses)
                .await?;
            return Ok(false);
        }
        match &message {
            ClientMessage::MoveHouses(MoveHouses { id: request }) => {
                if let Err(error) =
                    self.service.start_move(id, request)?
                {
                    self.send_to(id, reply(request, Err(error)))?;
                }
                self.refresh()?;
                return Ok(false);
            }
            ClientMessage::Edit(Edit::Notes {
                id: request,
                agent,
                text,
            }) => {
                self.edit_notes(id, request, agent, text)?;
                return Ok(false);
            }
            ClientMessage::Read(
                read @ (Read::Papers {
                    id: request, agent, ..
                }
                | Read::Prompt { id: request, agent }),
            ) => {
                self.read_papers(id, read, request, agent)?;
                return Ok(false);
            }
            _ => {}
        }
        let answer = match self.service.handle(message).await {
            Ok(answer) => answer,
            Err(error) => {
                failed(&self.service, &error);
                Some(fault(request["id"].as_str(), FAILED))
            }
        };
        if let Some(answer) = answer {
            self.send_to(id, answer)?;
        }
        self.refresh()?;
        Ok(true)
    }

    /// Starts writing an agent's NOTES.md for connection `id`, which
    /// is busy until it is written.
    fn edit_notes(
        &mut self,
        id: u64,
        request: &str,
        agent: &Address,
        text: &str,
    ) -> Result<()> {
        let (house, text) =
            match self.service.prepare_notes(agent, text)? {
                Ok(prepared) => prepared,
                Err(error) => {
                    return self
                        .send_to(id, reply(request, Err(error)));
                }
            };
        let request = request.to_owned();
        self.writes.spawn(async move {
            let result = house
                .replace(crate::papers::NOTES, text.as_bytes())
                .await;
            NotesWritten {
                owner: id,
                request,
                result,
            }
        });
        self.busy(id);
        Ok(())
    }

    /// Starts rendering an agent's papers or prompt for connection
    /// `id`, which is busy until they are read.
    fn read_papers(
        &mut self,
        id: u64,
        read: &Read,
        request: &str,
        agent: &Address,
    ) -> Result<()> {
        let number = self
            .service
            .agent(agent, "read or edit papers")?
            .ok()
            .map(|agent| agent.number);
        let prepared = crate::documents::prepare_read(
            &mut self.service,
            read.clone(),
        )?;
        let key = (id, request.to_owned());
        if self.reading.contains_key(&key) {
            let running = "That read request is already running.";
            return self.send_to(id, fault(Some(request), running));
        }
        let (request, address) = (request.to_owned(), agent.clone());
        let task = self.reads.spawn(async move {
            PapersRead {
                owner: id,
                request,
                address,
                number,
                answer: prepared.render().await,
            }
        });
        self.reading.insert(key, task);
        self.busy(id);
        Ok(())
    }

    fn busy(&mut self, id: u64) {
        if let Some(client) = self.clients.get_mut(&id) {
            client.busy = true;
        }
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

    async fn on_harness(
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

    async fn on_finished(
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

    async fn on_work(
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

    async fn on_merged(
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

    fn on_read(
        &mut self,
        read: Result<PapersRead, JoinError>,
    ) -> Result<()> {
        let Ok(read) = read else {
            return Ok(());
        };
        self.reading.remove(&(read.owner, read.request.clone()));
        let Some(client) = self.clients.get_mut(&read.owner) else {
            return Ok(());
        };
        client.busy = false;
        self.ready.extend(
            client.waiting.drain(..).map(|text| (read.owner, text)),
        );
        // A read refused when prepared keeps its refusal.
        let allowed = read.number.is_none()
            || self
                .service
                .agent(&read.address, "read or edit papers")?
                .ok()
                .is_some_and(|agent| {
                    Some(agent.number) == read.number
                });
        let answer = match read.answer {
            _ if !allowed => fault(
                Some(&read.request),
                "The agent's papers are no longer available.",
            ),
            Ok(answer) => answer,
            Err(error) => {
                failed(&self.service, &error);
                fault(Some(&read.request), FAILED)
            }
        };
        send(client, &self.service, answer)
    }

    fn on_written(
        &mut self,
        written: Result<NotesWritten, JoinError>,
    ) -> Result<()> {
        let Ok(written) = written else {
            return Ok(());
        };
        let owner = written.owner;
        let Some(client) = self.clients.get_mut(&owner) else {
            return Ok(());
        };
        client.busy = false;
        self.ready.extend(
            client.waiting.drain(..).map(|text| (owner, text)),
        );
        let result =
            written.result.map(|()| "Saved.".into()).map_err(|_| {
                Refusal::Invalid(
                    "The agent's NOTES.md could not be written \
                     safely. Inspect its house before trying again."
                        .into(),
                )
            });
        send(client, &self.service, reply(&written.request, result))
    }

    async fn on_given(
        &mut self,
        given: Result<crate::gifts::Completed, JoinError>,
    ) -> Result<()> {
        if let Ok(given) = given
            && let Some((owner, answer)) = crate::gifts::complete(
                &mut self.service,
                &mut self.harnesses,
                given,
            )
            .await?
            && let Some(client) = self.clients.get_mut(&owner)
        {
            client.busy = false;
            self.ready.extend(
                client.waiting.drain(..).map(|text| (owner, text)),
            );
            send(client, &self.service, answer)?;
        }
        self.refresh()
    }

    /// Frees the connections whose merge finished and sends what the
    /// chats hold, after each event.
    fn settle(&mut self) -> Result<()> {
        for owner in std::mem::take(&mut self.service.merges.finished)
        {
            if let Some(client) = self.clients.get_mut(&owner) {
                client.busy = false;
                self.ready.extend(
                    client
                        .waiting
                        .drain(..)
                        .map(|text| (owner, text)),
                );
            }
        }
        self.deliver()
    }

    /// Sends `message` to connection `id`, if it is still open.
    fn send_to(
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
                    .await?
                }
            }
            self.deliver()?;
        }
        flush_models(&mut self.service, &mut self.harnesses).await
    }

    /// Sends the messages the chats hold for clients.
    fn deliver(&mut self) -> Result<()> {
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
    fn refresh(&mut self) -> Result<()> {
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

    /// Closes in order (S3): the reflections owed are asked for and
    /// waited on, then each runtime stops its work and exits. A
    /// second signal exits at once.
    async fn close(&mut self, signals: &mut Signals) -> Result<()> {
        let waits =
            chats::close(&mut self.service, &mut self.harnesses)
                .await?;
        for client in self.clients.keys() {
            self.service.models.disconnect(*client);
        }
        flush_models(&mut self.service, &mut self.harnesses).await?;
        self.deliver()?;
        let settled = |service: &mut Service| -> Result<bool> {
            for (binding, request) in &waits {
                let standing = service
                    .home
                    .store()
                    .standing(*binding, request)?;
                let open = standing.is_none_or(|standing| {
                    !standing
                        .status
                        .is_some_and(InputStatus::is_ended)
                });
                if open {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        let reflected = async {
            while !settled(&mut self.service)? {
                self.run_once().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::select! {
            _ = signals.next() => std::process::exit(130),
            done = tokio::time::timeout(REFLECTING, reflected) => {
                if let Ok(done) = done {
                    done?;
                }
            }
        }
        self.harnesses.begin_close().await;
        let stopped = async {
            while !self.harnesses.down() {
                self.run_once().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::select! {
            _ = signals.next() => std::process::exit(130),
            done = tokio::time::timeout(STOPPING, stopped) => {
                if let Ok(done) = done {
                    done?;
                }
            }
        }
        Ok(())
    }

    /// Handles one event of a runtime or one finished tool call.
    async fn run_once(&mut self) -> Result<()> {
        crate::society::merging::lane::advance(
            &mut self.service,
            &mut self.harnesses,
        )
        .await?;
        tokio::select! {
            event = self.harnesses.next() => {
                self.on_harness(event).await?
            }
            finished = self.service.chats.next() => {
                self.on_finished(finished).await?
            }
            Some(work) = self.service.house_work.next() => {
                self.on_work(work).await?
            }
            Some(merged) = self.service.merges.tasks.join_next() => {
                self.on_merged(merged).await?;
                self.deliver()?;
            }
        }
        Ok(())
    }
}

async fn flush_models(
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

fn failed(service: &Service, error: &anyhow::Error) {
    let mut diagnostic = json!(format!("Request failed: {error:#}"));
    service.credentials.scrub(&mut diagnostic);
    eprintln!("{diagnostic}");
}

fn send(
    client: &mut Client,
    service: &Service,
    message: ServerMessage,
) -> Result<()> {
    let mut value = serde_json::to_value(message)?;
    service.credentials.scrub(&mut value);
    if let Err(mpsc::error::TrySendError::Full(_)) =
        client.send.try_send(Out::Message(value.to_string()))
    {
        client.refused = true;
    }
    Ok(())
}

fn refuse(client: &mut Client, text: &str, code: u16) -> Result<()> {
    client.refused = true;
    let _ = client.send.try_send(Out::Message(
        serde_json::to_string(&fault(None, text))?,
    ));
    let _ = client.send.try_send(Out::Close(code));
    Ok(())
}

async fn connection(
    stream: TcpStream,
    id: u64,
    events: mpsc::Sender<Event>,
    mut outgoing: mpsc::Receiver<Out>,
    mut closing: watch::Receiver<bool>,
) {
    let _ =
        talk(stream, id, &events, &mut outgoing, &mut closing).await;
    let _ = events.send(Event::Gone(id)).await;
}

/// What woke a connection's task.
enum Woke {
    Out(Out),
    Late,
    Frame(Option<Result<Message, Error>>),
}

/// Carries one connection: its frames to the loop as events, and what
/// the loop sends back, until either side closes.
async fn talk(
    stream: TcpStream,
    id: u64,
    events: &mpsc::Sender<Event>,
    outgoing: &mut mpsc::Receiver<Out>,
    closing: &mut watch::Receiver<bool>,
) -> Result<()> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(LARGEST))
        .max_frame_size(Some(LARGEST));
    let handshake = tokio_tungstenite::accept_async_with_config(
        stream,
        Some(config),
    );
    let handshake =
        tokio::time::timeout(Duration::from_secs(10), handshake);
    let mut socket = tokio::select! {
        _ = closing.changed() => return Ok(()),
        accepted = handshake => accepted??,
    };
    let hello = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(hello);
    let mut timed = false;
    loop {
        let woke = tokio::select! {
            biased;
            _ = closing.changed() => Woke::Out(Out::Close(1001)),
            out = outgoing.recv() => {
                Woke::Out(out.unwrap_or(Out::Close(1011)))
            }
            _ = &mut hello, if !timed => Woke::Late,
            frame = socket.next() => Woke::Frame(frame),
        };
        let out = match woke {
            Woke::Out(out) => out,
            Woke::Late => {
                timed = true;
                let _ = events.send(Event::Late(id)).await;
                continue;
            }
            Woke::Frame(Some(Ok(Message::Text(text)))) => {
                let text = text.to_string();
                if events
                    .send(Event::Message(id, text))
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            }
            Woke::Frame(Some(Ok(Message::Binary(bytes)))) => {
                let text =
                    String::from_utf8_lossy(&bytes).into_owned();
                if events
                    .send(Event::Message(id, text))
                    .await
                    .is_err()
                {
                    break;
                }
                continue;
            }
            Woke::Frame(Some(Ok(Message::Close(_))) | None) => {
                let _ = socket.flush().await;
                break;
            }
            Woke::Frame(Some(Ok(_))) => continue,
            Woke::Frame(Some(Err(Error::Capacity(_)))) => {
                let text = format!(
                    "A message is larger than the {LARGEST} bytes \
                     this connection takes."
                );
                let text = serde_json::to_string(&fault(None, text))?;
                let sent = socket.send(Message::Text(text.into()));
                let _ = tokio::time::timeout(GRACE, sent).await;
                Out::Close(1009)
            }
            Woke::Frame(Some(Err(_))) => break,
        };
        match out {
            Out::Message(text) => {
                let sent = socket.send(Message::Text(text.into()));
                tokio::time::timeout(GRACE, sent).await??;
            }
            Out::Close(code) => {
                let reason = if code == 1009 {
                    "Message too large."
                } else {
                    ""
                };
                let frame = CloseFrame {
                    code: CloseCode::from(code),
                    reason: reason.into(),
                };
                let _ = tokio::time::timeout(
                    GRACE,
                    socket.close(Some(frame)),
                )
                .await;
                // Drain bytes already in flight so closing a large
                // frame does not reset TCP before the
                // client reads the fault.
                if code == 1009 {
                    use tokio::io::AsyncReadExt;
                    let drain = async {
                        let mut bytes = [0; 8192];
                        while socket
                            .get_mut()
                            .read(&mut bytes)
                            .await
                            .is_ok_and(|n| n > 0)
                        {
                        }
                    };
                    let _ = tokio::time::timeout(GRACE, drain).await;
                } else {
                    let _ =
                        tokio::time::timeout(GRACE, socket.next())
                            .await;
                }
                break;
            }
        }
    }
    Ok(())
}

struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
}

impl Signals {
    fn new() -> Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                interrupt: tokio::signal::windows::ctrl_c()?,
            })
        }
    }
    async fn next(&mut self) {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.interrupt.recv() => {}
                _ = self.terminate.recv() => {}
            }
        }
        #[cfg(windows)]
        {
            self.interrupt.recv().await;
        }
    }
}
