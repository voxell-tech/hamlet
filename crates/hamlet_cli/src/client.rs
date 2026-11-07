//! Authenticated protocol connections, without the window's ECS.

use std::path::Path;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use futures::{SinkExt, StreamExt};
pub use hamlet_protocol::client::discovery::ServerAddress;
use hamlet_protocol::client::{
    ClientMessage, Hello, ImageId, PutImage, PutImagePart, Reply,
    ReplyKind, RequestId, ServerMessage, State, VERSION,
};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
};

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
            v: VERSION,
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
        id: &RequestId,
        message: ClientMessage,
    ) -> Result<ServerMessage> {
        self.send(message).await?;
        loop {
            let message = self.next().await?;
            if matches!(message, ServerMessage::Fault(_))
                || message.to() == Some(id)
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

/// The most bytes of an image one part carries.
const PART: usize = 512 * 1024;

/// Sends the image file at `path` over a connection of its own, so
/// the stream of a chat is left alone, and gives the id it is kept
/// under.
pub async fn upload_image(
    address: &ServerAddress,
    path: &Path,
) -> Result<ImageId> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Cannot read {}", path.display()))?;
    let (mut client, _) = connect(address).await?;
    let id = RequestId::new();
    let begun = client
        .request(
            &id,
            ClientMessage::PutImage(PutImage {
                id: id.clone(),
                size: bytes.len() as u64,
            }),
        )
        .await?;
    let upload = match begun {
        ServerMessage::Reply(Reply {
            kind: ReplyKind::Notice,
            upload: Some(upload),
            ..
        }) => upload,
        other => bail!("{}", refusal(&other)),
    };
    let mut image = None;
    let mut offset = 0;
    for part in bytes.chunks(PART) {
        let id = RequestId::new();
        let answer = client
            .request(
                &id,
                ClientMessage::PutImagePart(PutImagePart {
                    data: base64::engine::general_purpose::STANDARD
                        .encode(part),
                    id: id.clone(),
                    offset,
                    upload: upload.clone(),
                }),
            )
            .await?;
        match answer {
            ServerMessage::Reply(Reply {
                kind: ReplyKind::Notice,
                image: kept,
                ..
            }) => image = kept.or(image),
            other => bail!("{}", refusal(&other)),
        }
        offset += part.len() as u64;
    }
    client.close().await?;
    image.context("The server kept no image.")
}

/// The words of a refusal or a fault, or of an answer that is
/// neither.
fn refusal(message: &ServerMessage) -> String {
    match message {
        ServerMessage::Reply(reply) => reply.text.clone(),
        ServerMessage::Fault(fault) => fault.text.clone(),
        _ => "The server answered with something else.".into(),
    }
}
