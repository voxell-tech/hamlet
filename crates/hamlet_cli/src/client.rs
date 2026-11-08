//! Authenticated protocol connections, without the window's ECS.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use futures::{SinkExt, StreamExt};
pub use hamlet_protocol::client::discovery::ServerAddress;
use hamlet_protocol::client::{
    AttachedFile, ClientMessage, FileId, Hello, ImageId, PutFile,
    PutFilePart, PutImage, PutImagePart, Reply, ReplyKind, RequestId,
    ServerMessage, State, VERSION,
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

/// A file to send, and the name it is sent under.
pub struct Planned {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

/// The files a path holds, to send as one attachment (C33).
pub struct Plan {
    pub files: Vec<Planned>,
    pub bytes: u64,
    /// Links inside a folder and the like, which are left out.
    pub skipped: usize,
}

/// The files at `path`: itself, or what the folder holds, under
/// names that start with its own.
pub fn plan(path: &Path) -> Result<Plan> {
    let last = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .context("A name to send is in UTF-8.")
    };
    let name = last(path)?;
    let real = std::fs::canonicalize(path)
        .with_context(|| format!("Cannot read {}", path.display()))?;
    let mut plan = Plan {
        files: Vec::new(),
        bytes: 0,
        skipped: 0,
    };
    let metadata = std::fs::metadata(&real)?;
    if metadata.is_dir() {
        walk(&real, &name, &mut plan)?;
    } else if metadata.is_file() {
        plan.add(name.clone(), real, metadata.len())?;
    } else {
        bail!("`{name}` is neither a file nor a folder.");
    }
    if plan.files.is_empty() {
        bail!("There is nothing to send in `{name}`.");
    }
    Ok(plan)
}

impl Plan {
    fn add(
        &mut self,
        name: String,
        path: PathBuf,
        size: u64,
    ) -> Result<()> {
        if size > AttachedFile::MOST_FILE {
            bail!("`{name}` is more than 256 MiB.");
        }
        self.bytes += size;
        if self.bytes > AttachedFile::MOST_BYTES {
            bail!("That is more than 1 GiB to send at once.");
        }
        if self.files.len() >= AttachedFile::MOST_FILES {
            bail!("That is more than 10000 files to send at once.");
        }
        self.files.push(Planned { name, path, size });
        Ok(())
    }
}

fn walk(folder: &Path, name: &str, plan: &mut Plan) -> Result<()> {
    let mut entries = std::fs::read_dir(folder)
        .with_context(|| format!("Cannot read {}", folder.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    for entry in entries {
        let Some(last) =
            entry.file_name().and_then(|name| name.to_str())
        else {
            plan.skipped += 1;
            continue;
        };
        let name = format!("{name}/{last}");
        let metadata = std::fs::symlink_metadata(&entry)?;
        if metadata.is_dir() {
            walk(&entry, &name, plan)?;
        } else if metadata.is_file() {
            plan.add(name, entry, metadata.len())?;
        } else {
            plan.skipped += 1;
        }
    }
    Ok(())
}

/// Sends the files of `plan` over a connection of its own, and gives
/// how the server keeps them.
pub async fn upload_files(
    address: &ServerAddress,
    plan: &Plan,
) -> Result<Vec<AttachedFile>> {
    let (mut client, _) = connect(address).await?;
    let mut kept = Vec::new();
    for file in &plan.files {
        let bytes = std::fs::read(&file.path).with_context(|| {
            format!("Cannot read {}", file.path.display())
        })?;
        if bytes.len() as u64 != file.size {
            bail!("`{}` changed while it was sent.", file.name);
        }
        let id = put_file(&mut client, &bytes).await?;
        kept.push(AttachedFile {
            id,
            name: file.name.clone(),
            size: file.size,
        });
    }
    client.close().await?;
    Ok(kept)
}

/// Sends `bytes` as a file, and gives the id it is kept under.
async fn put_file(
    client: &mut Client,
    bytes: &[u8],
) -> Result<FileId> {
    let id = RequestId::new();
    let begun = client
        .request(
            &id,
            ClientMessage::PutFile(PutFile {
                id: id.clone(),
                size: bytes.len() as u64,
            }),
        )
        .await?;
    let upload = match begun {
        ServerMessage::Reply(Reply {
            kind: ReplyKind::Notice,
            file: Some(file),
            ..
        }) => return Ok(file),
        ServerMessage::Reply(Reply {
            kind: ReplyKind::Notice,
            upload: Some(upload),
            ..
        }) => upload,
        other => bail!("{}", refusal(&other)),
    };
    let mut kept = None;
    let mut offset = 0;
    for part in bytes.chunks(PART) {
        let id = RequestId::new();
        let answer = client
            .request(
                &id,
                ClientMessage::PutFilePart(PutFilePart {
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
                file,
                ..
            }) => kept = file.or(kept),
            other => bail!("{}", refusal(&other)),
        }
        offset += part.len() as u64;
    }
    kept.context("The server kept no file.")
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
