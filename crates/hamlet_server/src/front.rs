//! `hamlet_server --actors`: the client protocol served from the
//! actors. The realm, the workspace, the profiles and the library run
//! in this process and the agents in a Node child; a client's
//! messages become calls of the realm and the agents, and an agent's
//! chat events become the frames of the clients that watch it.

mod frames;
mod host;
mod view;

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_host::{Address, Host, Item};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::chat::{ChatEvent, ChatPage, ReadChat};
use hamlet_messages::realm::client::Say;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, ClientMessage, Confinement, Open, ServerMessage,
};
use hamlet_sandbox::Sandbox;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::{AbortHandle, JoinSet};
use tracing_subscriber::EnvFilter;

use crate::claim::Claim;
use crate::clients::check::{Protocol, fault, reply};
use crate::clients::connection::{self, Client, Event, GRACE};
use crate::credentials::Credentials;
use crate::diagnostic::{self, Secrets};
use crate::layout::Layout;
use crate::serve::{Options, Signals, announce, chief_name, publish};
use crate::society::refusal::Refusal;

/// Longer than the realm takes to answer a line, at most 8 seconds.
const SAY_WITHIN: Duration = Duration::from_secs(10);

/// The longest an agent may take to answer a read of a chat.
const READ_WITHIN: Duration = Duration::from_secs(60);

/// A frame for a connection, from a task of the server.
type Frames = mpsc::UnboundedSender<(u64, ServerMessage)>;

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
    let credentials = Arc::new(Credentials::open(&root)?);
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

/// The connections and what each watches.
struct Door {
    host: Host,
    protocol: Protocol,
    credentials: Arc<Credentials>,
    own: view::Own,
    clients: BTreeMap<u64, Client>,
    next: u64,
    watches: Vec<Watch>,
    events: mpsc::Sender<Event>,
    frames: Frames,
    closing: watch::Receiver<bool>,
    /// The connections' tasks.
    connections: JoinSet<()>,
    /// The tasks that call the actors for the clients.
    work: JoinSet<()>,
}

/// A chat a connection watches, followed by a task of its own.
struct Watch {
    client: u64,
    chat: protocol::ChatRef,
    task: AbortHandle,
}

impl Door {
    /// Serves the connections `listener` accepts until a signal
    /// comes, then closes them.
    async fn serve(
        host: Host,
        credentials: Arc<Credentials>,
        own: view::Own,
        listener: TcpListener,
        signals: &mut Signals,
    ) -> Result<()> {
        let (events, mut event) = mpsc::channel(1024);
        let (frames, mut frame) = mpsc::unbounded_channel();
        let (shutdown, closing) = watch::channel(false);
        let mut door = Self {
            host,
            protocol: Protocol::new()?,
            credentials,
            own,
            clients: BTreeMap::new(),
            next: 0,
            watches: Vec::new(),
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
                Some((client, message)) = frame.recv() => {
                    door.send(client, message)
                }
                Some(_) = door.work.join_next() => Ok(()),
            };
            if let Err(error) = step {
                break Err(error);
            }
        };
        shutdown.send_replace(true);
        door.work.abort_all();
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
                self.unwatch(|watch| watch.client == client);
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
            Err(failed) => {
                connection::send(
                    found,
                    &self.credentials,
                    ServerMessage::Fault(failed),
                )?;
                if !welcomed {
                    connection::refuse(
                        found,
                        "The first message is a hello.",
                        1008,
                    )?;
                }
                return Ok(());
            }
        };
        match message {
            ClientMessage::Hello(hello) if !welcomed => {
                if !self.credentials.is_connection_token(&hello.token)
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
                let state = view::state(&self.own);
                connection::send(
                    found,
                    &self.credentials,
                    ServerMessage::State(state),
                )
            }
            _ if !welcomed => connection::refuse(
                found,
                "The first message is a hello.",
                1008,
            ),
            message => self.request(client, message),
        }
    }

    fn request(
        &mut self,
        client: u64,
        message: ClientMessage,
    ) -> Result<()> {
        match message {
            ClientMessage::Hello(_) => self.send(
                client,
                fault(None, "This connection said hello already."),
            ),
            ClientMessage::Open(open) => {
                self.open(client, open);
                Ok(())
            }
            ClientMessage::Close(close) => {
                self.unwatch(|watch| {
                    watch.client == client && watch.chat == close.chat
                });
                Ok(())
            }
            ClientMessage::Say(say)
                if !say.text.trim().starts_with('/') =>
            {
                self.say(client, say);
                Ok(())
            }
            message => {
                let what = match &message {
                    ClientMessage::Say(say) => say
                        .text
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    _ => serde_json::to_value(&message)?["type"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                };
                let refusal = Refusal::Unsupported {
                    what: format!("`{what}`"),
                };
                let answer = match message.id() {
                    Some(id) => reply(id, Err(refusal)),
                    None => fault(None, refusal.to_string()),
                };
                self.send(client, answer)
            }
        }
    }

    /// Sends `client` the page `open` asks for, and from then on the
    /// changes of its chat, unless it watches that chat already.
    fn open(&mut self, client: u64, open: Open) {
        let host = self.host.clone();
        let frames = self.frames.clone();
        let credentials = self.credentials.clone();
        if self.watches.iter().any(|watch| {
            watch.client == client && watch.chat == open.chat
        }) {
            self.work.spawn(async move {
                let _ = page(&host, client, &open, &frames).await;
            });
            return;
        }
        let chat = open.chat.clone();
        let task = self.work.spawn(follow(
            host,
            client,
            open,
            frames,
            credentials,
        ));
        self.watches.push(Watch { client, chat, task });
    }

    /// Gives the realm a chief's line. An answer that is a refusal is
    /// sent back with its words.
    fn say(&mut self, client: u64, say: protocol::Say) {
        let host = self.host.clone();
        let frames = self.frames.clone();
        self.work.spawn(async move {
            let id = say.id.clone();
            let said = async {
                let answer = host
                    .call_for_client(
                        &address::Realm.address()?,
                        Say::NAME,
                        encode_positional(&Say(say))?,
                        SAY_WITHIN,
                    )
                    .await?;
                frames::refusal_words(&answer)
            };
            let message = match said.await {
                Ok(None) => return,
                Ok(Some(words)) => {
                    reply(&id, Err(Refusal::Usage { words }))
                }
                Err(error) => failed(Some(&id), &error),
            };
            let _ = frames.send((client, message));
        });
    }

    /// Stops following each watch `picks` holds for.
    fn unwatch(&mut self, picks: impl Fn(&Watch) -> bool) {
        self.watches.retain(|watch| {
            let picked = picks(watch);
            if picked {
                watch.task.abort();
            }
            !picked
        });
    }

    fn send(
        &mut self,
        client: u64,
        message: ServerMessage,
    ) -> Result<()> {
        match self.clients.get_mut(&client) {
            Some(found) => {
                connection::send(found, &self.credentials, message)
            }
            None => Ok(()),
        }
    }
}

/// Follows the chat `open` names for `client`: subscribed first, so
/// no change is missed, then its page, then each change after it.
async fn follow(
    host: Host,
    client: u64,
    open: Open,
    frames: Frames,
    credentials: Arc<Credentials>,
) {
    let tell = |words: String| {
        diagnostic::tell(Secrets::Of(&credentials), words);
    };
    let at = match agent(&open) {
        Ok(at) => at,
        Err(error) => {
            let _ =
                frames.send((client, failed(Some(&open.id), &error)));
            return;
        }
    };
    let mut subscription =
        match host.subscribe_for_client(&at, "chat").await {
            Ok(subscription) => subscription,
            Err(error) => {
                tell(format!("cannot follow {at}'s chat: {error:#}"));
                let _ = frames
                    .send((client, failed(Some(&open.id), &error)));
                return;
            }
        };
    let Ok((epoch, number)) =
        page(&host, client, &open, &frames).await
    else {
        return;
    };
    loop {
        let event = match subscription.next().await {
            Ok(Item::Event(args)) => ciborium::from_reader::<
                (ChatEvent,),
                _,
            >(args.as_slice()),
            Ok(Item::Gap) => {
                tell(format!("changes of {at}'s chat were lost"));
                continue;
            }
            Err(error) => {
                tell(format!("{at}'s chat ended: {error:#}"));
                return;
            }
        };
        let (event,) = match event {
            Ok(event) => event,
            Err(error) => {
                tell(format!("a change of {at}'s chat: {error}"));
                continue;
            }
        };
        if event.epoch == epoch && event.number <= number {
            continue;
        }
        match frames::turn(event, &open.chat) {
            Ok(Some(turn)) => {
                let _ =
                    frames.send((client, ServerMessage::Turn(turn)));
            }
            Ok(None) => {}
            Err(error) => {
                tell(format!("a change of {at}'s chat: {error}"));
            }
        }
    }
}

/// Reads the page `open` asks for and sends it to `client`, or the
/// fault of why it could not be read. Gives the page's epoch and the
/// number of the last change it holds.
async fn page(
    host: &Host,
    client: u64,
    open: &Open,
    frames: &Frames,
) -> Result<(u32, u32)> {
    let read = async {
        let answer = host
            .call_for_client(
                &agent(open)?,
                ReadChat::NAME,
                encode_positional(&ReadChat(open.clone()))?,
                READ_WITHIN,
            )
            .await?;
        let page: ChatPage =
            ciborium::from_reader(answer.as_slice())?;
        let read = (page.epoch, page.number);
        let transcript = frames::transcript(
            page,
            open.chat.clone(),
            Some(open.id.clone()),
        )?;
        Ok::<_, anyhow::Error>((read, transcript))
    };
    match read.await {
        Ok((read, transcript)) => {
            let _ = frames.send((
                client,
                ServerMessage::Transcript(transcript),
            ));
            Ok(read)
        }
        Err(error) => {
            let _ =
                frames.send((client, failed(Some(&open.id), &error)));
            Err(error)
        }
    }
}

/// The address of the agent whose chat `open` names.
fn agent(open: &Open) -> Result<Address> {
    let number = u32::try_from(open.chat.with.0)
        .context("no agent has that number")?;
    Ok(address::Agent(AgentNumber(number)).address()?)
}

/// The fault of a request the server failed.
fn failed(
    to: Option<&protocol::RequestId>,
    error: &anyhow::Error,
) -> ServerMessage {
    fault(to, format!("The server failed: {error}"))
}
