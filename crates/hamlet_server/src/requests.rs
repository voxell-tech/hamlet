//! What each message of a connection asks, and who answers it: the
//! hello, then each request by the owner one match names. The
//! sockets are the connection's, and the order of start and stop is
//! serve's.

use std::any::Any;
use std::panic::AssertUnwindSafe;

use anyhow::{Context, Result};
use futures::FutureExt;
use hamlet_protocol::client::{
    Address, ClientMessage, Edit, MoveHouses, Read, ServerMessage,
};
use tokio::task::JoinError;

use crate::chats::{self, Request};
use crate::connection::{Out, refuse, send};
use crate::protocol::{Refusal, fault, reply};
use crate::runtime::{Asked, FAILED, Serving, failed, flush_models};

pub(crate) const BEGIN: &str =
    "A connection begins with hello and the home's token.";

/// A read of an agent's papers or prompt, rendered off the loop.
pub(crate) struct PapersRead {
    owner: u64,
    request: String,
    address: Address,
    /// The agent's number when the read was prepared; unset when it
    /// was refused then.
    number: Option<u64>,
    answer: Result<ServerMessage>,
}

/// A write of an agent's NOTES.md, done off the loop.
pub(crate) struct NotesWritten {
    owner: u64,
    request: String,
    result: Result<()>,
}

impl Serving {
    /// Takes one message of connection `id`: held while the
    /// connection is busy, refused or welcomed before its hello,
    /// else answered. False when the message was dealt with in
    /// full here.
    pub(crate) async fn on_message(
        &mut self,
        id: u64,
        text: String,
    ) -> Result<bool> {
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
        let request = message.id().map(str::to_owned);
        let taken = self.take(id, message).await;
        taken.with_context(|| Asked {
            client: id,
            request,
        })
    }

    /// Takes a message of connection `id` that parsed: its hello, or
    /// a request once welcomed.
    async fn take(
        &mut self,
        id: u64,
        message: ClientMessage,
    ) -> Result<bool> {
        self.refresh()?;
        let Some(client) = self.clients.get_mut(&id) else {
            return Ok(false);
        };
        if client.welcomed {
            self.answer(id, message).await?;
            return Ok(true);
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
    /// will, then sends what follows from it.
    async fn answer(
        &mut self,
        id: u64,
        message: ClientMessage,
    ) -> Result<()> {
        // A handler that panics leaves the serving state as one that
        // errs half-way leaves it, which `recover` is written for: an
        // open transaction rolls back as it drops, and no handler
        // holds a lock.
        let answers = AssertUnwindSafe(self.handle(id, &message))
            .catch_unwind()
            .await
            .map_err(panicked)??;
        if self.working(id) {
            self.busy(id);
        }
        for answer in answers {
            self.send_to(id, answer)?;
        }
        self.deliver()?;
        self.refresh()?;
        flush_models(&mut self.service, &mut self.harnesses).await
    }

    /// The answers to `message`, a request of connection `id`, from
    /// the owner that answers it; none yet when its work answers it
    /// later.
    async fn handle(
        &mut self,
        id: u64,
        message: &ClientMessage,
    ) -> Result<Vec<ServerMessage>> {
        Ok(match message {
            ClientMessage::Hello(_) => {
                vec![fault(
                    None,
                    "This connection said hello already.",
                )]
            }
            ClientMessage::Say(said) => {
                self.converse(id, Request::Say(said)).await?
            }
            ClientMessage::Open(open) => {
                self.converse(id, Request::Open(open)).await?
            }
            ClientMessage::Close(close) => {
                self.converse(id, Request::Close(close)).await?
            }
            ClientMessage::New(new) => {
                self.converse(id, Request::New(new)).await?
            }
            ClientMessage::Resume(resume) => {
                self.converse(id, Request::Resume(resume)).await?
            }
            ClientMessage::Retry(retry) => {
                self.converse(id, Request::Retry(retry)).await?
            }
            ClientMessage::Rewind(rewind) => {
                self.converse(id, Request::Rewind(rewind)).await?
            }
            ClientMessage::Stop(stop) => {
                self.converse(id, Request::Stop(stop)).await?
            }
            ClientMessage::Answer(answer) => {
                self.converse(id, Request::Answer(answer)).await?
            }
            ClientMessage::Read(Read::History {
                agent,
                id: request,
            }) => {
                let history = Request::History { agent, id: request };
                self.converse(id, history).await?
            }
            ClientMessage::Give(_) | ClientMessage::GivePart(_) => {
                crate::gifts::request(
                    &mut self.service,
                    &mut self.harnesses,
                    id,
                    message,
                )
                .await?
                .into_iter()
                .collect()
            }
            ClientMessage::Login(_)
            | ClientMessage::Check(_)
            | ClientMessage::Read(Read::Models { .. }) => {
                self.service.model_request(id, message)?.context(
                    "the models answer sign-ins, checks and reads",
                )?
            }
            ClientMessage::Edit(Edit::Notes {
                id: request,
                agent,
                text,
            }) => {
                self.edit_notes(id, request, agent, text)?;
                Vec::new()
            }
            ClientMessage::Edit(edit) => {
                let request = message
                    .id()
                    .context("an edit names its request")?;
                vec![
                    self.service
                        .edit_document(request, edit.clone())
                        .await?,
                ]
            }
            ClientMessage::Read(
                read @ (Read::Papers {
                    id: request, agent, ..
                }
                | Read::Prompt { id: request, agent }),
            ) => {
                self.read_papers(id, read, request, agent)?;
                Vec::new()
            }
            ClientMessage::Read(read @ Read::Charter { .. }) => {
                vec![self.service.read(read.clone()).await?]
            }
            ClientMessage::MoveHouses(MoveHouses { id: request }) => {
                match self.service.start_move(id, request)? {
                    Ok(()) => Vec::new(),
                    Err(error) => vec![reply(request, Err(error))],
                }
            }
        })
    }

    /// The chats' answer to `request` of connection `id`.
    async fn converse(
        &mut self,
        id: u64,
        request: Request<'_>,
    ) -> Result<Vec<ServerMessage>> {
        let answer = chats::converse(
            &mut self.service,
            &mut self.harnesses,
            id,
            request,
        )
        .await?;
        Ok(answer.into_iter().collect())
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

    pub(crate) fn on_read(
        &mut self,
        read: Result<PapersRead, JoinError>,
    ) -> Result<()> {
        let Ok(read) = read else {
            return Ok(());
        };
        self.reading.remove(&(read.owner, read.request.clone()));
        if !self.free(read.owner) {
            return Ok(());
        }
        // A read refused when prepared keeps its refusal.
        let allowed = read.number.is_none()
            || self
                .service
                .agent(&read.address, "read or edit papers")
                .with_context(|| Asked {
                    client: read.owner,
                    request: Some(read.request.clone()),
                })?
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
        self.send_to(read.owner, answer)
    }

    pub(crate) fn on_written(
        &mut self,
        written: Result<NotesWritten, JoinError>,
    ) -> Result<()> {
        let Ok(written) = written else {
            return Ok(());
        };
        if !self.free(written.owner) {
            return Ok(());
        }
        let result =
            written.result.map(|()| "Saved.".into()).map_err(|_| {
                Refusal::Invalid(
                    "The agent's NOTES.md could not be written \
                     safely. Inspect its house before trying again."
                        .into(),
                )
            });
        self.send_to(written.owner, reply(&written.request, result))
    }
}

/// The error of a handler that panicked, with the panic's words.
fn panicked(panic: Box<dyn Any + Send>) -> anyhow::Error {
    let words = panic
        .downcast_ref::<&str>()
        .map(|words| (*words).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no words".into());
    anyhow::anyhow!("the handler panicked: {words}")
}
