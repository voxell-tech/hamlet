//! `hamlet_server --actors`: the client protocol served from the
//! actors. The realm, the workspace, the profiles and the library run
//! in this process and the agents in a Node child; a client's
//! messages become calls of the realm and the agents, and an agent's
//! chat events become the frames of the clients that watch it.

mod answers;
mod call;
mod commands;
mod frames;
mod host;
mod hub;
mod route;
mod sign_in;
mod uploads;
mod view;
mod words;

use std::collections::{BTreeMap, HashMap};
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard};

use anyhow::{Context, Result};
use hamlet_host::Host;
use hamlet_messages::address;
use hamlet_messages::agent::chat::{Login, Settle};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, ClientMessage, Confinement, Link, LinkState,
    RequestId, ServerMessage,
};
use hamlet_sandbox::Sandbox;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

use self::call::{
    Calls, Fate, Place, Said, Sent, failed, server_failed,
};
use self::commands::Outcome;
use self::host::HAND;
use self::hub::{Hub, ended_line};
use self::route::{After, Asked, Call, Route};
use self::sign_in::SignIns;
use self::uploads::Uploads;
use self::view::Shows;
use crate::claim::Claim;
use crate::clients::check::{Protocol, reply};
use crate::clients::connection::{self, Client, Event, GRACE};
use crate::credentials::Credentials;
use crate::layout::Layout;
use crate::serve::{
    Options, ROTATING, Signals, announce, chief_name, publish,
};
use crate::society::refusal::Refusal;

/// A frame for a connection, from a task of the server.
type Frames = mpsc::UnboundedSender<(u64, ServerMessage)>;

/// A request's work, done after the work of the requests its
/// connection sent before it.
type Job = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The realm's credentials, which each key typed in a sign-in joins.
#[derive(Clone)]
struct Keys(Arc<RwLock<Credentials>>);

impl Keys {
    fn read(&self) -> RwLockReadGuard<'_, Credentials> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn remember(&self, secret: &str) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remember(secret);
    }
}

/// Serves the realm at `options.home` from the actors until SIGTERM
/// or Ctrl-C, then closes the clients, the Node host and the host,
/// in that order. It publishes its address once the realm answers.
pub(crate) async fn serve(options: Options) -> Result<()> {
    let mut signals = Signals::new()?;
    hamlet_os::process::reap_with_server()?;
    log_host();
    let sandbox = Sandbox::open(options.isolation)?;
    sandbox.probe()?;
    let confinement = Confinement::Sandbox {
        name: sandbox.name().into(),
    };
    let home = &options.home;
    std::fs::create_dir_all(home)
        .with_context(|| format!("making {}", home.display()))?;
    let root = home
        .canonicalize()
        .with_context(|| format!("resolving {}", home.display()))?;
    let _claim = Claim::take(&root)?;
    let credentials = Keys(Arc::new(RwLock::new(
        Credentials::open_beside_actors(&root)?,
    )));
    let (host, supervisor) =
        host::open(&root, sandbox, options.faux.as_deref())?;
    let mut published = None;
    let served = async {
        let started = tokio::select! {
            started = host::start_realm(&host) => started,
            () = signals.next() => return Ok(()),
        };
        started?;
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, options.port))
                .await?;
        let url = format!("ws://{}", listener.local_addr()?);
        published = Some(publish(
            Layout::new(&root).server(),
            &url,
            &confinement,
        )?);
        announce(&root, &url);
        let uploads = Uploads::at(&root)?;
        let own = view::Own {
            chief: chief_name(),
            confinement: confinement.clone(),
            root: root.clone(),
            epoch: crate::clock::now(),
            link: watch::Sender::new(crate::settings::untested()),
            moving: watch::Sender::new(None),
            faux: options.faux.is_some(),
        };
        Door::serve(
            host.clone(),
            credentials,
            own,
            uploads,
            listener,
            &mut signals,
            host::start_agents(&host, &supervisor),
        )
        .await
    }
    .await;
    supervisor.shutdown().await;
    let shut = host.shutdown().await;
    drop(published);
    served.and(shut)
}

/// Writes the host's warnings and errors to stderr, the server's
/// log.
fn log_host() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("warn"))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}

/// The connections, the chats they follow and the sign-ins they
/// started.
struct Door {
    calls: Calls,
    hub: Hub,
    shows: Shows,
    protocol: Protocol,
    credentials: Keys,
    own: Arc<view::Own>,
    uploads: Arc<Uploads>,
    clients: BTreeMap<u64, Client>,
    queues: HashMap<u64, Queue>,
    next: u64,
    sign_ins: SignIns,
    events: mpsc::Sender<Event>,
    frames: Frames,
    closing: watch::Receiver<bool>,
    connections: JoinSet<()>,
    /// The tasks that call the actors for the clients.
    work: JoinSet<()>,
}

/// One connection's requests not yet done, in the order sent, and
/// what it sent.
struct Queue {
    jobs: mpsc::UnboundedSender<Job>,
    sent: Sent,
}

impl Door {
    /// Serves the connections `listener` accepts while `agents`
    /// starts the agents, until a signal comes or that start fails,
    /// then closes them.
    async fn serve(
        host: Host,
        credentials: Keys,
        own: view::Own,
        uploads: Uploads,
        listener: TcpListener,
        signals: &mut Signals,
        agents: impl Future<Output = Result<()>>,
    ) -> Result<()> {
        let (events, mut event) = mpsc::channel(1024);
        let (frames, mut frame) = mpsc::unbounded_channel();
        let (shutdown, closing) = watch::channel(false);
        let calls = Calls::new(host);
        let own = Arc::new(own);
        let (shows, jobs) = Shows::new();
        let hub = Hub::new(
            calls.clone(),
            frames.clone(),
            credentials.clone(),
            shows.clone(),
        );
        let mut work = JoinSet::new();
        work.spawn(view::serve(
            jobs,
            shows.clone(),
            calls.clone(),
            own.clone(),
            hub.clone(),
            credentials.clone(),
            frames.clone(),
        ));
        let mut door = Self {
            hub,
            shows,
            calls,
            protocol: Protocol::new()?,
            credentials,
            own,
            uploads: Arc::new(uploads),
            clients: BTreeMap::new(),
            queues: HashMap::new(),
            next: 0,
            sign_ins: SignIns::default(),
            events,
            frames,
            closing,
            connections: JoinSet::new(),
            work,
        };
        let mut agents = std::pin::pin!(agents);
        let mut starting = true;
        let result = loop {
            let step = tokio::select! {
                () = signals.next() => break Ok(()),
                started = &mut agents, if starting => {
                    starting = false;
                    started.map(|()| {
                        door.shows.read_providers();
                        door.work.spawn(view::check_at_start(
                            door.calls.clone(),
                            door.own.clone(),
                        ));
                    })
                }
                accepted = listener.accept() => {
                    if let Ok((stream, _)) = accepted {
                        door.accept(stream);
                    }
                    Ok(())
                }
                Some(event) = event.recv() => door.connection(event),
                Some((client, message)) = frame.recv() => {
                    door.send(client, message)
                }
                Some(_) = door.work.join_next() => Ok(()),
            };
            if let Err(error) = step {
                break Err(error);
            }
        };
        // New connections are refused from here, as today's close
        // drops its listener first.
        drop(listener);
        shutdown.send_replace(true);
        door.work.abort_all();
        door.hub.close_all();
        let connections = &mut door.connections;
        let drained = async {
            while connections.join_next().await.is_some() {}
        };
        let _ = tokio::time::timeout(GRACE, drained).await;
        door.connections.abort_all();
        // The agents' host stops after this, so a rotated login has
        // to reach the realm first, as today's close waits for it.
        let settled = door.calls.call(&address::Agent(HAND), &Settle);
        let _ = tokio::time::timeout(ROTATING, settled).await;
        result
    }

    fn accept(&mut self, stream: TcpStream) {
        let id = self.next;
        self.next += 1;
        let client = connection::accept(
            stream,
            id,
            self.events.clone(),
            self.closing.clone(),
            &mut self.connections,
        );
        self.clients.insert(id, client);
        self.hub.join(id);
    }

    fn connection(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Message(client, text) => {
                self.message(client, &text)
            }
            Event::Late(client) => {
                if let Some(found) = self.clients.get_mut(&client)
                    && !found.welcomed
                {
                    connection::refuse(
                        found,
                        "No hello came in time.",
                        1008,
                    )?;
                }
                Ok(())
            }
            Event::Gone(client) => {
                self.clients.remove(&client);
                self.queues.remove(&client);
                self.shows.gone(client);
                self.hub.forget(client);
                self.uploads.disconnect(client);
                for login in self.sign_ins.forget(client) {
                    let calls = self.calls.clone();
                    self.work.spawn(async move {
                        let cancel = protocol::Login::Cancel {
                            cancel: login,
                            id: RequestId::new(),
                        };
                        let _ = calls
                            .call(
                                &address::Agent(HAND),
                                &Login(cancel),
                            )
                            .await;
                    });
                }
                Ok(())
            }
        }
    }

    fn message(&mut self, client: u64, text: &str) -> Result<()> {
        let Some(found) = self.clients.get_mut(&client) else {
            return Ok(());
        };
        if found.refused {
            return Ok(());
        }
        let welcomed = found.welcomed;
        let message = match self.protocol.parse(text) {
            Ok(message) => message,
            Err(failed) if welcomed => {
                self.answer(client, ServerMessage::Fault(failed));
                return Ok(());
            }
            Err(failed) => {
                connection::send(
                    found,
                    &self.credentials.read(),
                    ServerMessage::Fault(failed),
                )?;
                return connection::refuse(
                    found,
                    "The first message is a hello.",
                    1008,
                );
            }
        };
        match message {
            ClientMessage::Hello(hello) if !welcomed => {
                if !self
                    .credentials
                    .read()
                    .is_connection_token(&hello.token)
                {
                    return connection::refuse(
                        found,
                        "That token is not this server's.",
                        4001,
                    );
                }
                let ours = protocol::VERSION;
                if !ours.is_compatible(&hello.v) {
                    let words = format!(
                        "This `hello` message does not fit the \
                         protocol: `v` {} is not compatible with \
                         {ours}.",
                        hello.v
                    );
                    return connection::refuse(found, &words, 4002);
                }
                found.welcomed = true;
                self.welcome(client);
                Ok(())
            }
            _ if !welcomed => connection::refuse(
                found,
                "The first message is a hello.",
                1008,
            ),
            message => {
                let id = message.id().cloned();
                match route::route(message) {
                    Ok(route) => self.request(client, route),
                    Err(error) => self.answer(
                        client,
                        server_failed(id.as_ref(), &error),
                    ),
                }
                Ok(())
            }
        }
    }

    fn request(&mut self, client: u64, route: Route) {
        match route {
            Route::Now(answer) => self.answer(client, *answer),
            Route::Open(open) => self.hub.watch(client, open),
            Route::Close(chat) => self.hub.close(client, &chat),
            Route::Command(asked) => {
                let job = self.ask_in_turn(client).command(asked);
                self.in_turn(client, job);
            }
            Route::Stop(asked, stopping) => {
                let sent = self.queue(client).sent.clone();
                let asker = self.ask_at(client, sent.stop(stopping));
                self.in_turn(client, async move {
                    asker.command(asked).await;
                    sent.answered();
                });
            }
            Route::Answer(asked) => {
                let place = self.queue(client).sent.place();
                let asker = self.ask(client);
                self.work.spawn(async move {
                    place.stops_answered().await;
                    asker.command(asked).await;
                });
            }
            Route::Line(say, agent) => self.line(client, say, agent),
            Route::Call(call) => self.call(client, call),
            Route::Check(call) => {
                if !self.own.faux {
                    self.own.link.send_replace(Link {
                        models: None,
                        state: LinkState::Testing,
                        words: None,
                    });
                }
                self.call(client, call);
            }
            Route::SignIn(login) => self.sign_in(client, login),
            Route::Read(reading) => {
                let asker = self.ask_in_turn(client);
                self.in_turn(client, async move {
                    asker.read(reading).await;
                });
            }
            Route::Upload(upload) => {
                let (uploads, frames) =
                    (self.uploads.clone(), self.frames.clone());
                self.in_turn(client, async move {
                    let id = upload.id().clone();
                    let message =
                        uploads.take(client, upload).unwrap_or_else(
                            |error| server_failed(Some(&id), &error),
                        );
                    let _ = frames.send((client, message));
                });
            }
        }
    }

    fn queue(&mut self, client: u64) -> &Queue {
        let work = &mut self.work;
        self.queues.entry(client).or_insert_with(|| {
            let (queue, mut jobs) = mpsc::unbounded_channel::<Job>();
            work.spawn(async move {
                while let Some(job) = jobs.recv().await {
                    job.await;
                }
            });
            Queue {
                jobs: queue,
                sent: Sent::new(),
            }
        })
    }

    /// Runs `job` once the jobs of the requests `client` sent before
    /// it are done, so one connection's requests apply in the order
    /// sent.
    fn in_turn(
        &mut self,
        client: u64,
        job: impl Future<Output = ()> + Send + 'static,
    ) {
        let _ = self.queue(client).jobs.send(Box::pin(job));
    }

    /// Sends `client` `message` in its turn.
    fn answer(&mut self, client: u64, message: ServerMessage) {
        let frames = self.frames.clone();
        self.in_turn(client, async move {
            let _ = frames.send((client, message));
        });
    }

    /// The door's means for a task that answers `client`.
    fn ask(&self, client: u64) -> Asker {
        Asker {
            client,
            calls: self.calls.clone(),
            hub: self.hub.clone(),
            shows: self.shows.clone(),
            own: self.own.clone(),
            uploads: self.uploads.clone(),
            frames: self.frames.clone(),
        }
    }

    /// The door's means for the request `client` sent now, whose
    /// waits a stop it sends next ends.
    fn ask_in_turn(&mut self, client: u64) -> Asker {
        let place = self.queue(client).sent.place();
        self.ask_at(client, place)
    }

    /// The door's means for a request of `client` at `place`.
    fn ask_at(&self, client: u64, place: Place) -> Asker {
        Asker {
            calls: self.calls.at(place),
            ..self.ask(client)
        }
    }

    /// Sends `client`, which said hello, the realm as it is now,
    /// before the answer of anything it asks next.
    fn welcome(&mut self, client: u64) {
        let shows = self.shows.clone();
        self.in_turn(client, async move {
            shows.welcome(client).await;
        });
    }

    /// Makes the call `call` and sends its answer's frame, after what
    /// the call changed is shown.
    fn call(&mut self, client: u64, call: Call) {
        let asker = self.ask_in_turn(client);
        self.in_turn(client, async move {
            let Call {
                to,
                action,
                args,
                request,
                answer,
                after,
            } = call;
            let held = match after {
                After::Began(agent) => {
                    Some(asker.hub.hold_resets(agent))
                }
                _ => None,
            };
            let message =
                match asker.calls.send(&to, action, args).await {
                    Ok(answered) => {
                        asker.after(after).await;
                        answer(&request, &answered, &asker.own)
                            .unwrap_or_else(|error| {
                                server_failed(Some(&request), &error)
                            })
                    }
                    Err(error) => failed(&request, &error),
                };
            asker.send(message);
            if let Some(held) = held {
                held.release().await;
            }
        });
    }

    /// Gives the realm a chief's line to `agent`, whose reply the hub
    /// owes from before the line is sent.
    fn line(
        &mut self,
        client: u64,
        say: protocol::Say,
        agent: AgentNumber,
    ) {
        let asker = self.ask_in_turn(client);
        self.in_turn(
            client,
            async move { asker.line(say, agent).await },
        );
    }

    fn send(
        &mut self,
        client: u64,
        message: ServerMessage,
    ) -> Result<()> {
        match self.clients.get_mut(&client) {
            Some(found) => connection::send(
                found,
                &self.credentials.read(),
                message,
            ),
            None => Ok(()),
        }
    }
}

/// What a task that answers one client works with.
#[derive(Clone)]
struct Asker {
    client: u64,
    calls: Calls,
    hub: Hub,
    shows: Shows,
    own: Arc<view::Own>,
    uploads: Arc<Uploads>,
    frames: Frames,
}

impl Asker {
    fn send(&self, message: ServerMessage) {
        let _ = self.frames.send((self.client, message));
    }

    /// Does what a request's answer is followed by, before the
    /// answer is sent.
    async fn after(&self, after: After) {
        match after {
            After::Nothing => {}
            After::Show | After::Began(_) => self.shows.read().await,
            After::Reread(agent) => {
                self.shows.read().await;
                self.hub.reread(agent).await;
            }
        }
    }

    /// Runs `asked` with the realm as it is then, and sends its reply
    /// once what it changed is shown.
    async fn command(self, asked: Asked) {
        let ran = async {
            let view = view::read(&self.calls, &self.own).await?;
            match &asked {
                Asked::Command(say) => {
                    commands::run(&self.calls, &view, say).await
                }
                Asked::Edit(_, edit) => {
                    answers::edit(&self.calls, edit).await
                }
                Asked::Answer(answer) => commands::answer(
                    &self.calls,
                    &view,
                    answer.clone(),
                )
                .await
                .map(Outcome::said),
                Asked::Stop(_, agent) => {
                    commands::stop_one(&self.calls, &view, *agent)
                        .await
                        .map(Outcome::said)
                }
                Asked::MoveLodges(_) => {
                    answers::move_lodges(&self.calls, &self.own).await
                }
                Asked::StopAll(_) => {
                    commands::stop_all(&self.calls, &view).await.map(
                        |turns| {
                            Outcome::said(Ok(
                                words::stopped_or_nothing(turns),
                            ))
                        },
                    )
                }
            }
        };
        let id = asked.id();
        let held = match &asked {
            Asked::Command(say)
                if commands::parse::is_new(&say.text) =>
            {
                route::number(say.to)
                    .map(|agent| self.hub.hold_resets(agent))
            }
            _ => None,
        };
        let message = match ran.await {
            Ok(outcome) => {
                self.after(match outcome.reread {
                    Some(agent) => After::Reread(agent),
                    None => After::Show,
                })
                .await;
                reply(id, outcome.reply)
            }
            Err(error) => failed(id, &error),
        };
        self.send(message);
        if let Some(held) = held {
            held.release().await;
        }
    }

    async fn read(&self, reading: answers::Reading) {
        let id = reading.id().clone();
        let message =
            answers::read(&self.calls, &self.uploads, reading)
                .await
                .unwrap_or_else(|error| {
                    server_failed(Some(&id), &error)
                });
        self.send(message);
    }

    /// Gives the realm a chief's line to `agent`, and reads what
    /// became of it outside the connection's turn while its reply is
    /// owed.
    async fn line(&self, say: protocol::Say, agent: AgentNumber) {
        let id = say.id.clone();
        let owed = self.hub.owe(self.client, agent, &id);
        let said = match uploads::refusal(
            &self.calls,
            &self.own,
            &say,
            agent,
        )
        .await
        {
            Ok(Some(refusal)) => {
                Ok(Said::Refused(refusal.to_string()))
            }
            Ok(None) => self.calls.say(say).await,
            Err(error) => Err(error),
        };
        let Some(held) = owed else {
            return;
        };
        let settle = |message| {
            self.hub.settle(agent, self.client, &id, message);
        };
        match said {
            Ok(Said::Accepted) => {
                let (calls, hub) =
                    (self.calls.clone(), self.hub.clone());
                let (client, line) = (self.client, id.clone());
                let unsettled = self.calls.unsettled(agent);
                let reading = tokio::spawn(async move {
                    let fate = calls.given(agent, &line).await;
                    drop(unsettled);
                    let message = match fate {
                        Ok(Fate::Given) => {
                            if held.await.is_ok() {
                                hub.taken(client, agent, &line).await;
                            }
                            return;
                        }
                        Ok(Fate::Withdrawn) => ended_line(&line),
                        Err(error) => failed(&line, &error),
                    };
                    hub.settle(agent, client, &line, message);
                });
                self.hub.reading(
                    self.client,
                    agent,
                    &id,
                    reading.abort_handle(),
                );
            }
            Ok(Said::Left(_)) => settle(ended_line(&id)),
            Ok(Said::Refused(words)) => {
                settle(reply(&id, Err(Refusal::Usage { words })));
            }
            Err(error) => settle(failed(&id, &error)),
        }
    }
}
