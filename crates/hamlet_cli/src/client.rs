//! Authenticated protocol connections, without the window's ECS.

use anyhow::{Context, Result, bail};
use futures::{SinkExt, StreamExt};
use hamlet_protocol::v0::{
    ClientMessage, Hello, ReadResult, ServerMessage, State,
};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
};

/// The address and connection token of an agent server.
pub struct ServerAddress {
    pub url: String,
    pub token: String,
}

/// A chief's authenticated connection.
pub struct Client(WebSocketStream<MaybeTlsStream<TcpStream>>);

/// Connects and reads the server's initial state.
pub async fn connect(
    address: &ServerAddress,
) -> Result<(Client, State)> {
    let (socket, _) = connect_async(&address.url)
        .await
        .context("Cannot connect to the server")?;
    let mut client = Client(socket);
    client
        .send(ClientMessage::Hello(Hello {
            token: address.token.clone(),
            v: 0.0,
            seen: None,
        }))
        .await?;
    match client.next().await? {
        ServerMessage::State(state) => Ok((client, state)),
        ServerMessage::Fault(fault) => bail!("{}", fault.text),
        _ => bail!("The server sent no state."),
    }
}

impl Client {
    /// Sends one request without retrying it.
    pub async fn send(
        &mut self,
        message: ClientMessage,
    ) -> Result<()> {
        self.0
            .send(Message::Text(
                serde_json::to_string(&message)?.into(),
            ))
            .await?;
        Ok(())
    }

    /// Reads a protocol message, answering WebSocket pings along the
    /// way.
    pub async fn next(&mut self) -> Result<ServerMessage> {
        while let Some(message) = self.0.next().await {
            match message? {
                Message::Text(text) => {
                    return serde_json::from_str(&text)
                        .context("Invalid server message");
                }
                Message::Ping(_) => self.0.flush().await?,
                Message::Close(_) => break,
                _ => {}
            }
        }
        bail!("The server closed the connection.")
    }

    /// Waits for this request's reply, leaving unrelated messages out
    /// of the result.
    pub async fn request(
        &mut self,
        id: &str,
        message: ClientMessage,
    ) -> Result<ServerMessage> {
        self.send(message).await?;
        loop {
            let message = self.next().await?;
            if matches!(message, ServerMessage::Fault(_))
                || reply_to(&message) == Some(id)
            {
                return Ok(message);
            }
        }
    }

    /// Closes the connection without stopping any agent.
    pub async fn close(&mut self) -> Result<()> {
        self.0.close(None).await?;
        Ok(())
    }
}

/// The request answered by a server message.
pub fn reply_to(message: &ServerMessage) -> Option<&str> {
    match message {
        ServerMessage::Reply(reply) => Some(&reply.to),
        ServerMessage::Fault(fault) => fault.to.as_deref(),
        ServerMessage::Transcript(transcript) => {
            transcript.to.as_deref()
        }
        ServerMessage::Login(login) => login.to.as_deref(),
        ServerMessage::Read(read) => Some(match read {
            ReadResult::Prompt { to, .. }
            | ReadResult::History { to, .. }
            | ReadResult::Models { to, .. }
            | ReadResult::Papers { to, .. }
            | ReadResult::Charter { to, .. } => to,
        }),
        _ => None,
    }
}
