//! `hamlet_server --actors`: the client protocol served from the
//! actors. The realm, the workspace, the profiles and the library run
//! in this process and the agents in a Node child; a client's
//! messages become calls of the realm and the agents, and an agent's
//! chat events become the frames of the clients that watch it.

mod call;
mod commands;
mod frames;
mod host;
mod hub;
mod route;
mod view;
mod words;

use std::collections::{BTreeMap, HashMap};
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::{
    Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard,
};

use anyhow::{Context, Result, anyhow};
use hamlet_host::Host;
use hamlet_messages::address;
use hamlet_messages::agent::chat::{Login, LoginAnswer};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, ClientMessage, Confinement, LoginProgress,
    LoginReply, LoginStep, RequestId, ServerMessage,
};
use hamlet_sandbox::Sandbox;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

use self::call::{Calls, failed, failure, server_failed};
use self::commands::Outcome;
use self::host::HAND;
use self::hub::Hub;
use self::route::{After, Asked, Call, Route};
use crate::claim::Claim;
use crate::clients::check::{Protocol, reply};
use crate::clients::connection::{self, Client, Event, GRACE};
use crate::credentials::Credentials;
use crate::layout::Layout;
use crate::providers::no_sign_in;
use crate::serve::{Options, Signals, announce, chief_name, publish};
use crate::society::refusal::Refusal;

/// Who a frame is for.
#[derive(Clone, Copy, Debug)]
enum To {
    One(u64),
    /// Every connection that said hello.
    Welcomed,
}

/// A frame for the connections, from a task of the server.
type Frames = mpsc::UnboundedSender<(To, ServerMessage)>;

/// A request's work, done after the work of the requests its
/// connection sent before it.
type Job = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The realm's credentials, which a key typed in a sign-in joins, so
/// that no frame or log line carries it.
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
/// in that order.
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
    let credentials =
        Keys(Arc::new(RwLock::new(Credentials::open(&root)?)));
    let (host, supervisor) =
        host::open(&root, sandbox, options.faux.as_deref())?;
    let mut published = None;
    let served = async {
        let started = tokio::select! {
            started = host::start(&host, &supervisor) => started,
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
        let own = view::Own {
            chief: chief_name(),
            confinement: confinement.clone(),
            realm: hamlet_os::fs::path::text(&root),
            epoch: crate::clock::now(),
        };
        Door::serve(
            host.clone(),
            credentials,
            own,
            listener,
            &mut signals,
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
    protocol: Protocol,
    credentials: Keys,
    own: Arc<view::Own>,
    clients: BTreeMap<u64, Client>,
    /// Each connection's requests not yet done, in the order sent.
    queues: HashMap<u64, mpsc::UnboundedSender<Job>>,
    /// The revision of the `State` each connection was sent last.
    shown: HashMap<u64, i64>,
    next: u64,
    sign_ins: SignIns,
    events: mpsc::Sender<Event>,
    frames: Frames,
    closing: watch::Receiver<bool>,
    /// The connections' tasks.
    connections: JoinSet<()>,
    /// The tasks that call the actors for the clients.
    work: JoinSet<()>,
}

impl Door {
    /// Serves the connections `listener` accepts until a signal
    /// comes, then closes them.
    async fn serve(
        host: Host,
        credentials: Keys,
        own: view::Own,
        listener: TcpListener,
        signals: &mut Signals,
    ) -> Result<()> {
        let (events, mut event) = mpsc::channel(1024);
        let (frames, mut frame) = mpsc::unbounded_channel();
        let (shutdown, closing) = watch::channel(false);
        let calls = Calls::new(host);
        let mut door = Self {
            hub: Hub::new(
                calls.clone(),
                frames.clone(),
                credentials.clone(),
            ),
            calls,
            protocol: Protocol::new()?,
            credentials,
            own: Arc::new(own),
            clients: BTreeMap::new(),
            queues: HashMap::new(),
            shown: HashMap::new(),
            next: 0,
            sign_ins: SignIns::default(),
            events,
            frames,
            closing,
            connections: JoinSet::new(),
            work: JoinSet::new(),
        };
        let result = loop {
            let step = tokio::select! {
                () = signals.next() => break Ok(()),
                accepted = listener.accept() => {
                    if let Ok((stream, _)) = accepted {
                        door.accept(stream);
                    }
                    Ok(())
                }
                Some(event) = event.recv() => door.connection(event),
                Some((to, message)) = frame.recv() => {
                    door.deliver(to, message)
                }
                Some(_) = door.work.join_next() => Ok(()),
            };
            if let Err(error) = step {
                break Err(error);
            }
        };
        shutdown.send_replace(true);
        door.work.abort_all();
        door.hub.close_all();
        let connections = &mut door.connections;
        let drained = async {
            while connections.join_next().await.is_some() {}
        };
        let _ = tokio::time::timeout(GRACE, drained).await;
        door.connections.abort_all();
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
                self.shown.remove(&client);
                self.hub.forget(client);
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
            Route::Command(asked) => self.command(client, asked),
            Route::Line(say, agent) => self.line(client, say, agent),
            Route::Call(call) => self.call(client, call),
            Route::SignIn(login) => self.sign_in(client, login),
        }
    }

    /// Runs `job` once the jobs of the requests `client` sent before
    /// it are done, so one connection's requests apply in the order
    /// sent.
    fn in_turn(
        &mut self,
        client: u64,
        job: impl Future<Output = ()> + Send + 'static,
    ) {
        let work = &mut self.work;
        let queue = self.queues.entry(client).or_insert_with(|| {
            let (queue, mut jobs) = mpsc::unbounded_channel::<Job>();
            work.spawn(async move {
                while let Some(job) = jobs.recv().await {
                    job.await;
                }
            });
            queue
        });
        let _ = queue.send(Box::pin(job));
    }

    /// Sends `client` `message` in its turn.
    fn answer(&mut self, client: u64, message: ServerMessage) {
        let frames = self.frames.clone();
        self.in_turn(client, async move {
            let _ = frames.send((To::One(client), message));
        });
    }

    /// The door's means for a task that answers `client`.
    fn ask(&self, client: u64) -> Asker {
        Asker {
            client,
            calls: self.calls.clone(),
            hub: self.hub.clone(),
            own: self.own.clone(),
            frames: self.frames.clone(),
        }
    }

    /// Sends `client`, which said hello, the realm as it is now,
    /// before the answer of anything it asks next.
    fn welcome(&mut self, client: u64) {
        let asker = self.ask(client);
        self.in_turn(client, async move {
            asker.show(To::One(client)).await;
        });
    }

    /// Makes the call `call` and sends its answer's frame, after what
    /// the call changed is shown.
    fn call(&mut self, client: u64, call: Call) {
        let asker = self.ask(client);
        self.in_turn(client, async move {
            let Call {
                to,
                action,
                args,
                request,
                answer,
                after,
            } = call;
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
        let asker = self.ask(client);
        self.in_turn(
            client,
            async move { asker.line(say, agent).await },
        );
    }

    /// Runs a chief's command or edit with the realm as it is now,
    /// and sends its reply once what it changed is shown.
    fn command(&mut self, client: u64, asked: Asked) {
        let asker = self.ask(client);
        self.in_turn(client, async move {
            let ran = async {
                let view =
                    view::read(&asker.calls, &asker.own).await?;
                match &asked {
                    Asked::Command(say) => {
                        commands::run(&asker.calls, &view, say).await
                    }
                    Asked::Edit(_, edit) => {
                        commands::edit(&asker.calls, edit).await
                    }
                    Asked::Answer(answer) => commands::answer(
                        &asker.calls,
                        &view,
                        answer.clone(),
                    )
                    .await
                    .map(Outcome::said),
                    Asked::StopAll(id) => {
                        commands::stop_all(&asker.calls, &view, id)
                            .await
                            .map(|turns| {
                                Outcome::said(Ok(
                                    words::stopped_or_nothing(turns),
                                ))
                            })
                    }
                }
            };
            let id = asked.id();
            let message = match ran.await {
                Ok(outcome) => {
                    asker
                        .after(match outcome.reread {
                            Some(agent) => After::Reread(agent),
                            None => After::Show,
                        })
                        .await;
                    reply(id, outcome.reply)
                }
                Err(error) => failed(id, &error),
            };
            asker.send(message);
        });
    }

    /// Takes a step of a sign-in: one its client starts, answers or
    /// cancels. Only the connection that started a sign-in may
    /// answer, cancel or start it again.
    fn sign_in(&mut self, client: u64, login: protocol::Login) {
        let asker = self.ask(client);
        let sign_ins = self.sign_ins.clone();
        match login {
            protocol::Login::Start { id, start } => {
                let login = id.to_string();
                let held = {
                    let mut open = sign_ins.lock();
                    let held = open
                        .get(&login)
                        .is_some_and(|open| open.client != client);
                    if !held {
                        open.insert(
                            login.clone(),
                            SignIn {
                                client,
                                start: id.clone(),
                                provider: start.provider.clone(),
                                last: None,
                                asking: false,
                            },
                        );
                    }
                    held
                };
                if held {
                    asker.send(reply(&id, Err(no_sign_in())));
                    return;
                }
                let start = protocol::Login::Start { id, start };
                self.work.spawn(sign_ins.steps(asker, login, start));
            }
            protocol::Login::Reply { id, reply: answer } => {
                let refusal = {
                    let mut open = sign_ins.lock();
                    match open
                        .get_mut(&answer.login)
                        .filter(|open| open.client == client)
                    {
                        None => Some(no_sign_in()),
                        Some(open) if !open.asking => {
                            Some(Refusal::Usage {
                                words: "That sign-in is not waiting \
                                        for an answer."
                                    .into(),
                            })
                        }
                        Some(open) => {
                            open.asking = false;
                            None
                        }
                    }
                };
                if let Some(refusal) = refusal {
                    asker.send(reply(&id, Err(refusal)));
                    return;
                }
                self.credentials.remember(&answer.value);
                asker.send(reply(
                    &id,
                    Ok("Sign-in answer received.".to_owned()),
                ));
                let login = answer.login.clone();
                let answered =
                    protocol::Login::Reply { id, reply: answer };
                self.work
                    .spawn(sign_ins.steps(asker, login, answered));
            }
            protocol::Login::Cancel { id, cancel } => {
                let open = {
                    let mut open = sign_ins.lock();
                    open.get(&cancel)
                        .is_some_and(|open| open.client == client)
                        .then(|| open.remove(&cancel))
                        .flatten()
                };
                let Some(open) = open else {
                    asker.send(reply(&id, Err(no_sign_in())));
                    return;
                };
                asker.send(open.frame(
                    &cancel,
                    LoginStep::Failed {
                        failed: "Sign-in cancelled.".into(),
                    },
                ));
                asker.send(reply(
                    &id,
                    Ok("Sign-in cancelled.".to_owned()),
                ));
                self.work.spawn(async move {
                    let cancel =
                        protocol::Login::Cancel { cancel, id };
                    let _ = asker
                        .calls
                        .call(&address::Agent(HAND), &Login(cancel))
                        .await;
                });
            }
        }
    }

    fn deliver(
        &mut self,
        to: To,
        message: ServerMessage,
    ) -> Result<()> {
        match to {
            To::One(client) => self.send(client, message),
            To::Welcomed => {
                for (&client, found) in &mut self.clients {
                    if found.welcomed
                        && news(&mut self.shown, client, &message)
                    {
                        connection::send(
                            found,
                            &self.credentials.read(),
                            message.clone(),
                        )?;
                    }
                }
                Ok(())
            }
        }
    }

    fn send(
        &mut self,
        client: u64,
        message: ServerMessage,
    ) -> Result<()> {
        match self.clients.get_mut(&client) {
            Some(found)
                if news(&mut self.shown, client, &message) =>
            {
                connection::send(
                    found,
                    &self.credentials.read(),
                    message,
                )
            }
            _ => Ok(()),
        }
    }
}

/// Whether `message` is news to `client`: a `State` older than the
/// one it was sent last is not, since a `State` replaces the last.
fn news(
    shown: &mut HashMap<u64, i64>,
    client: u64,
    message: &ServerMessage,
) -> bool {
    let ServerMessage::State(state) = message else {
        return true;
    };
    let last = shown.entry(client).or_insert(i64::MIN);
    if state.revision < *last {
        return false;
    }
    *last = state.revision;
    true
}

/// Sends `to` the realm as it is now. A connection that said hello
/// is told why it could not be read.
async fn show(
    calls: &Calls,
    own: &Arc<view::Own>,
    frames: &Frames,
    to: To,
) {
    let shown = match view::read(calls, own).await {
        Ok(view) => ServerMessage::State(view::state(&view)),
        Err(error) if matches!(to, To::One(_)) => {
            server_failed(None, &error)
        }
        Err(_) => return,
    };
    let _ = frames.send((to, shown));
}

/// What a task that answers one client works with.
#[derive(Clone)]
struct Asker {
    client: u64,
    calls: Calls,
    hub: Hub,
    own: Arc<view::Own>,
    frames: Frames,
}

impl Asker {
    fn send(&self, message: ServerMessage) {
        let _ = self.frames.send((To::One(self.client), message));
    }

    async fn show(&self, to: To) {
        show(&self.calls, &self.own, &self.frames, to).await;
    }

    /// Does what a request's answer is followed by, before the
    /// answer is sent.
    async fn after(&self, after: After) {
        match after {
            After::Nothing => {}
            After::Show => self.show(To::Welcomed).await,
            After::Reread(agent) => {
                self.show(To::Welcomed).await;
                self.hub.reread(agent).await;
            }
        }
    }

    async fn line(&self, say: protocol::Say, agent: AgentNumber) {
        let id = say.id.clone();
        if self.hub.owe(self.client, agent, &id).await.is_err() {
            return;
        }
        let settle = |message| {
            self.hub.settle(agent, self.client, &id, message);
        };
        match self.calls.say(say).await {
            Ok(Ok(_)) => {
                self.hub.taken(self.client, agent, &id).await;
            }
            Ok(Err(words)) => {
                settle(reply(&id, Err(Refusal::Usage { words })));
            }
            Err(error) => settle(failed(&id, &error)),
        }
    }
}

/// The sign-ins the door's clients started, by login: the id of the
/// `start` that began each.
#[derive(Clone, Default)]
struct SignIns(Arc<Mutex<HashMap<String, SignIn>>>);

struct SignIn {
    client: u64,
    start: RequestId,
    provider: String,
    /// The step given last, which a step that finds nothing new
    /// gives again.
    last: Option<LoginStep>,
    /// Whether it waits for the chief's typed answer.
    asking: bool,
}

impl SignIn {
    /// `step` of sign-in `login`, as its client is sent it.
    fn frame(&self, login: &str, step: LoginStep) -> ServerMessage {
        ServerMessage::Login(LoginProgress {
            login: login.to_owned(),
            step,
            to: Some(self.start.clone()),
        })
    }
}

impl SignIns {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, SignIn>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Drops the sign-ins `client` started, whose connection closed,
    /// and gives their logins.
    fn forget(&self, client: u64) -> Vec<String> {
        let mut open = self.lock();
        let gone = open
            .iter()
            .filter(|(_, open)| open.client == client)
            .map(|(login, _)| login.clone())
            .collect::<Vec<_>>();
        open.retain(|_, open| open.client != client);
        gone
    }

    /// Sends the Hand `first`, a step of sign-in `login`, then asks
    /// it for each next step and sends each new one to its client,
    /// until the sign-in waits for the chief, ends or is dropped.
    async fn steps(
        self,
        asker: Asker,
        login: String,
        first: protocol::Login,
    ) {
        let hand = address::Agent(HAND);
        let mut next = first;
        loop {
            let answer = asker.calls.call(&hand, &Login(next)).await;
            let step = match answer {
                Ok(LoginAnswer::Step { step, .. }) => step,
                Ok(LoginAnswer::Done { .. }) => {
                    let ended = self.lock().remove(&login);
                    if let Some(ended) = ended {
                        let done = LoginStep::Done {
                            done: ended.provider.clone(),
                        };
                        asker.after(After::Show).await;
                        asker.send(ended.frame(&login, done));
                    }
                    return;
                }
                Ok(LoginAnswer::Failed { words, .. }) => {
                    return self.end(&asker, &login, words);
                }
                Ok(LoginAnswer::NotTheHand) => {
                    let error =
                        anyhow!("agent {} is not the Hand", HAND.0);
                    return self.end(&asker, &login, failure(&error));
                }
                Err(error) => {
                    return self.end(&asker, &login, failure(&error));
                }
            };
            {
                let mut open = self.lock();
                let Some(going) = open
                    .get_mut(&login)
                    .filter(|going| going.client == asker.client)
                else {
                    return;
                };
                if going.last.as_ref() != Some(&step) {
                    asker.send(going.frame(&login, step.clone()));
                    going.last = Some(step.clone());
                }
                if let LoginStep::Prompt { .. } = step {
                    going.asking = true;
                    return;
                }
            }
            next = protocol::Login::Reply {
                id: RequestId::new(),
                reply: LoginReply {
                    login: login.clone(),
                    value: String::new(),
                },
            };
        }
    }

    /// Ends sign-in `login`, unless it ended, with its failure in
    /// `words`.
    fn end(&self, asker: &Asker, login: &str, words: String) {
        if let Some(ended) = self.lock().remove(login) {
            asker.send(
                ended.frame(
                    login,
                    LoginStep::Failed { failed: words },
                ),
            );
        }
    }
}
