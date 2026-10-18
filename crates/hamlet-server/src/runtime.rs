//! The loopback listener, published address and ordered shutdown.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::{SinkExt, StreamExt};
use hamlet_protocol::v0::{ClientMessage, ServerMessage, State};
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{
    CloseFrame, WebSocketConfig,
};
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::credentials::write_whole;
use crate::harness::{self, Harnesses, Notice};
use crate::protocol::{Protocol, fault};
use crate::service::Service;
use crate::state::id_of;
use crate::{Home, chats};

const LARGEST: usize = 1_048_576;
const BEGIN: &str =
    "A connection begins with hello and the home's token.";
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
    crate::sandbox::Sandbox::platform().probe()?;
    let home = Home::open(&options.home)?;
    let mut service = Service::new(home, options.faux.is_some())?;
    chats::load(&mut service)?;
    let layout = service.home.layout().clone();
    let (mut harnesses, notices) = Harnesses::start(
        &layout,
        service.home.store(),
        harness::Config::from_env(options.faux.clone())?,
    )?;
    for notice in notices {
        chats::heard(&mut service, &mut harnesses, notice).await?;
    }
    service.start_models()?;
    flush_models(&mut service, &mut harnesses).await?;
    let started = async {
        while service.models.starting() {
            let event = harnesses.next().await;
            for notice in
                harnesses.handle(service.home.store(), event).await?
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
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::time::timeout(Duration::from_secs(15), started)
        .await
        .context("starting model providers")??;
    chats::start(&mut service, &mut harnesses).await?;
    let protocol = Protocol::new()?;
    let listener = TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        options.port,
    ))
    .await?;
    let url = format!("ws://{}", listener.local_addr()?);
    let published = service.home.layout().root().join("server.json");
    write_whole(
        &published,
        &serde_json::to_vec(
            &json!({"url":url,"pid":std::process::id(),"started":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}),
        )?,
    )?;
    let published = Published(published);
    eprintln!(
        "hamlet-server: holding {}; serving at {url}; Ctrl-C stops",
        service.home.layout().root().display()
    );
    let (events, mut incoming) = mpsc::channel(128);
    let (shutdown, closing) = watch::channel(false);
    let mut tasks = JoinSet::new();
    let mut clients = HashMap::<u64, Client>::new();
    let mut next = 0;
    let epoch = chrono::Utc::now().timestamp_millis();
    let mut state = service.state(epoch, 0)?;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let result = async {
        loop {
            tokio::select! {
                biased;
                _ = signals.next() => break,
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    let (send, receive) = mpsc::channel(128);
                    let id = next;
                    next += 1;
                    clients.insert(id, Client { send, welcomed:false, refused:false });
                    tasks.spawn(connection(stream, id, events.clone(), receive, closing.clone()));
                }
                Some(event) = incoming.recv() => {
                    match event {
                        Event::Gone(id) => { clients.remove(&id); service.models.disconnect(id); service.chats.gone(id); }
                        Event::Late(id) => {
                            if let Some(client) = clients.get_mut(&id) && !client.welcomed && !client.refused { refuse(client, BEGIN, 1008)?; }
                        }
                        Event::Message(id, text) => {
                            refresh(&mut service, &mut state, &mut clients)?;
                            let Some(client) = clients.get_mut(&id).filter(|c|!c.refused) else { continue; };
                            let message = match protocol.parse(&text) {
                                Ok(message) => message,
                                Err(mut error) => {
                                    if !client.welcomed { error.to = None; }
                                    send(client, &service, ServerMessage::Fault(error))?;
                                    if !client.welcomed { client.refused = true; let _ = client.send.try_send(Out::Close(1008)); }
                                    continue;
                                }
                            };
                            if !client.welcomed {
                                if let ClientMessage::Hello(hello) = message {
                                    if service.credentials.accepts(&hello.token) {
                                        send(client, &service, ServerMessage::State(state.clone()))?;
                                        client.welcomed = true;
                                    } else { refuse(client, "That is not this server's token.", 1008)?; }
                                } else { refuse(client, BEGIN, 1008)?; }
                                continue;
                            }
                            let request = serde_json::to_value(&message)?;
                            match chats::converse(&mut service, &mut harnesses, id, &message).await {
                                Ok(None) => {}
                                Ok(Some(answer)) => {
                                    if let Some(answer) = answer && let Some(client) = clients.get_mut(&id) { send(client, &service, answer)?; }
                                    deliver(&mut service, &mut clients)?;
                                    refresh(&mut service, &mut state, &mut clients)?;
                                    continue;
                                }
                                Err(error) => {
                                    failed(&service, &error);
                                    if let Some(client) = clients.get_mut(&id) { send(client, &service, fault(request["id"].as_str(), "The server could not complete this request."))?; }
                                    deliver(&mut service, &mut clients)?;
                                    refresh(&mut service, &mut state, &mut clients)?;
                                    continue;
                                }
                            }
                            let Some(client) = clients.get_mut(&id) else { continue; };
                            if let Some(answers) = service.model_request(id, &message)? {
                                for answer in answers { send(client, &service, answer)?; }
                                refresh(&mut service, &mut state, &mut clients)?;
                                flush_models(&mut service, &mut harnesses).await?;
                                continue;
                            }
                            match service.handle(message) {
                                Ok(Some(answer)) => send(client, &service, answer)?,
                                Ok(None) => {},
                                Err(error) => {
                                    failed(&service, &error);
                                    send(client, &service, fault(request["id"].as_str(), "The server could not complete this request."))?;
                                }
                            }
                            refresh(&mut service, &mut state, &mut clients)?;
                        }
                    }
                }
                _ = tick.tick() => {
                    harnesses.refresh_tools().await;
                    for result in service.models.expired() {
                        for (id, answer) in service.provider_result(result)? {
                            if let Some(client) = clients.get_mut(&id) { send(client, &service, answer)?; }
                        }
                    }
                    refresh(&mut service, &mut state, &mut clients)?;
                },
                event = harnesses.next() => {
                    let notices = harnesses.handle(service.home.store(), event).await?;
                    heard(&mut service, &mut harnesses, &mut clients, notices).await?;
                    refresh(&mut service, &mut state, &mut clients)?;
                }
                Some(ran) = service.chats.tools.join_next() => {
                    if let Ok(ran) = ran { chats::ran(&mut service, &mut harnesses, ran).await?; }
                    deliver(&mut service, &mut clients)?;
                    refresh(&mut service, &mut state, &mut clients)?;
                }
                Some(_) = tasks.join_next() => {},
            }
        }
        Ok::<_, anyhow::Error>(())
    }.await;
    drop(listener);
    let result = match result {
        Ok(()) => {
            close(
                &mut service,
                &mut harnesses,
                &mut signals,
                &mut clients,
            )
            .await
        }
        result => result,
    };
    service.chats.stop_tools();
    shutdown.send_replace(true);
    let drained =
        async { while tasks.join_next().await.is_some() {} };
    tokio::select! {
        _ = signals.next() => std::process::exit(130),
        _ = tokio::time::timeout(GRACE, drained) => {},
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    harnesses.close().await;
    drop(published);
    service.home.close()?;
    result
}

async fn flush_models(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    for request in std::mem::take(&mut service.models.outgoing) {
        harnesses.provider(service.home.store(), request).await?;
    }
    Ok(())
}

/// Takes in what the runtimes told, and sends clients what follows.
async fn heard(
    service: &mut Service,
    harnesses: &mut Harnesses,
    clients: &mut HashMap<u64, Client>,
    notices: Vec<Notice>,
) -> Result<()> {
    for notice in notices {
        match notice {
            Notice::Provider(result) => {
                for (id, answer) in service.provider_result(result)? {
                    if let Some(client) = clients.get_mut(&id) {
                        send(client, service, answer)?;
                    }
                }
            }
            notice => {
                chats::heard(service, harnesses, notice).await?
            }
        }
        deliver(service, clients)?;
    }
    Ok(())
}

/// Sends the messages the chats hold for clients.
fn deliver(
    service: &mut Service,
    clients: &mut HashMap<u64, Client>,
) -> Result<()> {
    for (id, message) in std::mem::take(&mut service.chats.outbox) {
        if let Some(client) =
            clients.get_mut(&id).filter(|c| c.welcomed && !c.refused)
        {
            send(client, service, message)?;
        }
    }
    Ok(())
}

fn failed(service: &Service, error: &anyhow::Error) {
    let mut diagnostic = json!(format!("Request failed: {error:#}"));
    service.credentials.scrub(&mut diagnostic);
    eprintln!("{diagnostic}");
}

/// Closes in order (S3): the reflections owed are asked for and
/// waited on, then each runtime stops its work and exits. A second
/// signal exits at once.
async fn close(
    service: &mut Service,
    harnesses: &mut Harnesses,
    signals: &mut Signals,
    clients: &mut HashMap<u64, Client>,
) -> Result<()> {
    let waits = chats::close(service, harnesses).await?;
    deliver(service, clients)?;
    let settled = |service: &mut Service| -> Result<bool> {
        for (binding, request) in &waits {
            let standing =
                service.home.store().standing(*binding, request)?;
            let open = standing.is_none_or(|standing| {
                !matches!(
                    standing.status.as_deref(),
                    Some("done" | "unanswered")
                )
            });
            if open {
                return Ok(false);
            }
        }
        Ok(true)
    };
    let reflected = async {
        while !settled(service)? {
            run_once(service, harnesses, clients).await?;
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::select! {
        _ = signals.next() => std::process::exit(130),
        done = tokio::time::timeout(REFLECTING, reflected) => {
            if let Ok(done) = done { done?; }
        }
    }
    harnesses.begin_close().await;
    let stopped = async {
        while !harnesses.down() {
            run_once(service, harnesses, clients).await?;
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::select! {
        _ = signals.next() => std::process::exit(130),
        done = tokio::time::timeout(STOPPING, stopped) => {
            if let Ok(done) = done { done?; }
        }
    }
    Ok(())
}

/// Handles one event of a runtime or one finished tool call.
async fn run_once(
    service: &mut Service,
    harnesses: &mut Harnesses,
    clients: &mut HashMap<u64, Client>,
) -> Result<()> {
    tokio::select! {
        event = harnesses.next() => {
            let notices =
                harnesses.handle(service.home.store(), event).await?;
            heard(service, harnesses, clients, notices).await?;
        }
        Some(ran) = service.chats.tools.join_next() => {
            if let Ok(ran) = ran {
                chats::ran(service, harnesses, ran).await?;
            }
            deliver(service, clients)?;
        }
    }
    Ok(())
}

fn refresh(
    service: &mut Service,
    state: &mut State,
    clients: &mut HashMap<u64, Client>,
) -> Result<()> {
    let mut after = service.state(state.epoch, state.revision)?;
    let mut endings = HashMap::new();
    for wait in &state.waits {
        if !after.waits.iter().any(|w| id_of(w) == id_of(wait)) {
            let db = service.home.store().connection();
            endings.insert(
                id_of(wait).to_owned(),
                crate::oversight::ended(db, id_of(wait))?,
            );
        }
    }
    let ending = |id: &str| {
        endings.get(id).cloned().unwrap_or_else(|| "lapsed".into())
    };
    if let Some(change) =
        crate::state::changed(state, &mut after, ending)
    {
        for client in
            clients.values_mut().filter(|c| c.welcomed && !c.refused)
        {
            send(client, service, change.clone())?;
        }
        *state = after;
    }
    // Gone follows the client's last admitted message, even when it
    // no longer reads replies.
    clients.retain(|_, c| !c.refused);
    Ok(())
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
    let result = async {
        let config = WebSocketConfig::default().max_message_size(Some(LARGEST)).max_frame_size(Some(LARGEST));
        let handshake = tokio_tungstenite::accept_async_with_config(stream, Some(config));
        let mut socket = tokio::select! {
            _ = closing.changed() => return Ok::<_, anyhow::Error>(()),
            accepted = tokio::time::timeout(Duration::from_secs(10), handshake) => accepted??,
        };
        let hello = tokio::time::sleep(Duration::from_secs(10));
        tokio::pin!(hello);
        let mut timed = false;
        loop {
            let send = tokio::select! {
                biased;
                _ = closing.changed() => Some(Out::Close(1001)),
                out = outgoing.recv() => Some(out.unwrap_or(Out::Close(1011))),
                _ = &mut hello, if !timed => { timed = true; let _ = events.send(Event::Late(id)).await; None },
                incoming = socket.next() => match incoming {
                    Some(Ok(Message::Text(text))) => { if events.send(Event::Message(id,text.to_string())).await.is_err() { break; } None },
                    Some(Ok(Message::Binary(bytes))) => { if events.send(Event::Message(id,String::from_utf8_lossy(&bytes).into_owned())).await.is_err() { break; } None },
                    Some(Ok(Message::Close(_))) | None => { let _ = socket.flush().await; break; },
                    Some(Ok(_)) => None,
                    Some(Err(Error::Capacity(_))) => {
                        let text = format!("A message is larger than the {LARGEST} bytes this connection takes.");
                        let _ = tokio::time::timeout(GRACE, socket.send(Message::Text(serde_json::to_string(&fault(None,text))?.into()))).await;
                        Some(Out::Close(1009))
                    },
                    Some(Err(_)) => break,
                },
            };
            match send {
                Some(Out::Message(text)) => { tokio::time::timeout(GRACE, socket.send(Message::Text(text.into()))).await??; },
                Some(Out::Close(code)) => {
                    let reason = if code == 1009 { "Message too large." } else { "" };
                    let _ = tokio::time::timeout(GRACE, socket.close(Some(CloseFrame { code:CloseCode::from(code), reason:reason.into() }))).await;
                    // Drain bytes already in flight so closing a large frame does not reset TCP before the client reads the fault.
                    if code == 1009 {
                        use tokio::io::AsyncReadExt;
                        let drain = async {
                            let mut bytes = [0; 8192];
                            while socket.get_mut().read(&mut bytes).await.is_ok_and(|n| n > 0) {}
                        };
                        let _ = tokio::time::timeout(GRACE, drain).await;
                    } else {
                        let _ = tokio::time::timeout(GRACE, socket.next()).await;
                    }
                    break;
                }
                None => {},
            }
        }
        Ok(())
    }.await;
    let _ = result;
    let _ = events.send(Event::Gone(id)).await;
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
            tokio::select! { _ = self.interrupt.recv() => {}, _ = self.terminate.recv() => {} }
        }
        #[cfg(windows)]
        {
            self.interrupt.recv().await;
        }
    }
}
