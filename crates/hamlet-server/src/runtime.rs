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

use crate::Home;
use crate::credentials::write_whole;
use crate::harness::{self, Harnesses, Notice};
use crate::protocol::{Protocol, fault};
use crate::service::Service;

const LARGEST: usize = 1_048_576;
const BEGIN: &str =
    "A connection begins with hello and the home's token.";
const GRACE: Duration = Duration::from_secs(1);

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
    let home = Home::open(&options.home)?;
    let mut service = Service::new(home, options.faux.is_some())?;
    // Runtimes with bound conversations carry on their work; protocol
    // v0 serves no conversation yet, so none is bound here.
    let layout = service.home.layout().clone();
    let (mut harnesses, notices) = Harnesses::start(
        &layout,
        service.home.store(),
        harness::Config::from_env(options.faux.clone())?,
    )?;
    report(&notices);
    service.start_models()?;
    flush_models(&mut service, &mut harnesses).await?;
    let started = async {
        while service.models.starting() {
            let event = harnesses.next().await;
            for notice in
                harnesses.handle(service.home.store(), event).await?
            {
                if let Notice::Provider(result) = notice {
                    service.provider_result(result)?;
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::time::timeout(Duration::from_secs(15), started)
        .await
        .context("starting model providers")??;
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
                        Event::Gone(id) => { clients.remove(&id); service.models.disconnect(id); }
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
                                    let mut diagnostic = json!(format!("Request failed: {error:#}"));
                                    service.credentials.scrub(&mut diagnostic);
                                    eprintln!("{diagnostic}");
                                    send(client, &service, fault(request["id"].as_str(), "The server could not complete this request."))?;
                                }
                            }
                            refresh(&mut service, &mut state, &mut clients)?;
                        }
                    }
                }
                _ = tick.tick() => {
                    for result in service.models.expired() {
                        for (id, answer) in service.provider_result(result)? {
                            if let Some(client) = clients.get_mut(&id) { send(client, &service, answer)?; }
                        }
                    }
                    refresh(&mut service, &mut state, &mut clients)?;
                },
                event = harnesses.next() => {
                    let notices = harnesses.handle(service.home.store(), event).await?;
                    report(&notices);
                    for notice in notices {
                        if let Notice::Provider(result) = notice {
                            for (id, answer) in service.provider_result(result)? {
                                if let Some(client) = clients.get_mut(&id) { send(client, &service, answer)?; }
                            }
                        }
                    }
                    refresh(&mut service, &mut state, &mut clients)?;
                }
                Some(_) = tasks.join_next() => {},
            }
        }
        Ok::<_, anyhow::Error>(())
    }.await;
    drop(listener);
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

/// Logs what a chief must hear of the runtimes until protocol v0
/// shows it.
fn report(notices: &[Notice]) {
    for notice in notices {
        match notice {
            Notice::Refused { runtime, words }
            | Notice::Fault { runtime, words } => {
                eprintln!(
                    "hamlet-server: {}: {words}",
                    runtime.as_str()
                );
            }
            _ => {}
        }
    }
}

fn refresh(
    service: &mut Service,
    state: &mut State,
    clients: &mut HashMap<u64, Client>,
) -> Result<()> {
    let mut after = service.state(state.epoch, state.revision)?;
    if let Some(change) = crate::state::changed(state, &mut after) {
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
