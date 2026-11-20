//! A chief's sign-in to an MCP server reached by a url, with OAuth:
//! the server registers itself as a client of the server's
//! authorization server, gives the chief the address to sign in at,
//! and takes the code back on this machine's loopback, or from the
//! address the chief's browser ended on. The tokens are kept in
//! `mcp/.signed-in/<name>.json`, out of every agent's reach and every
//! program server's, and refreshed there.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy_ecs::message::Message;
use rmcp::transport::auth::{
    AuthError, AuthorizationManager, AuthorizationRequest,
    CredentialStore, OAuthState, StoredCredentials,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::app::input::InputSender;
use crate::realm::layout::Layout;

/// How long a sign-in waits for the chief.
const PATIENCE: Duration = Duration::from_secs(10 * 60);

/// The most characters of why a sign-in failed that are kept.
const WORDS: usize = 300;

/// The longest request the loopback reads.
const REQUEST: usize = 16 * 1024;

/// The folder that keeps the tokens of the server `name`.
pub(super) fn folder(layout: &Layout, name: &str) -> PathBuf {
    layout.mcp().join(".signed-in").join(name)
}

/// The file that keeps the tokens of the server `name` at `url`: a
/// server whose url changed signs in again, so no token reaches
/// another resource.
pub(super) fn tokens(
    layout: &Layout,
    name: &str,
    url: &str,
) -> PathBuf {
    let hash = Sha256::digest(url.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    folder(layout, name).join(format!("{hash}.json"))
}

/// The tokens in `file`, which a result must not show.
pub(super) async fn secrets(file: &Path) -> Vec<String> {
    let Ok(Some(stored)) = Tokens::at(file).load().await else {
        return Vec::new();
    };
    // As stored, a token is a string under its name.
    let Ok(stored) = serde_json::to_value(stored) else {
        return Vec::new();
    };
    ["access_token", "refresh_token"]
        .iter()
        .filter_map(|key| stored["token_response"][key].as_str())
        .map(String::from)
        .collect()
}

/// A sign-in under way, which ends when this drops.
pub(super) struct Signing {
    /// The address the chief signs in at, once the server gave it.
    pub address: Option<String>,
    /// Where the address the chief's browser ended on goes.
    pasted: mpsc::Sender<String>,
    task: AbortHandle,
}

impl Signing {
    /// Hands the sign-in the address the chief's browser ended on.
    pub fn paste(&self, address: String) {
        let _ = self.pasted.try_send(address);
    }
}

impl Drop for Signing {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// What a sign-in says of itself.
#[derive(Message)]
pub(crate) struct SignInStep {
    pub server: String,
    pub step: Step,
}

pub(crate) enum Step {
    /// The address to sign in at.
    Address(String),
    Done,
    Failed(String),
}

/// Why a sign-in ended without tokens.
#[derive(Debug, thiserror::Error)]
enum Unsigned {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("the loopback could not open: {0}")]
    Loopback(std::io::Error),
    #[error("no sign-in came within {} minutes", PATIENCE.as_secs() / 60)]
    Late,
    #[error("the server is stopping")]
    Stopping,
}

/// Begins the chief's sign-in to the server `server` at `url`, its
/// tokens kept in `file`; each step goes to `inputs`.
pub(super) fn begin(
    server: String,
    url: String,
    file: PathBuf,
    inputs: InputSender,
) -> Signing {
    let (pasted, from_chief) = mpsc::channel(1);
    let task = tokio::spawn(async move {
        let step =
            match sign_in(&server, &url, &file, from_chief, &inputs)
                .await
            {
                Ok(()) => Step::Done,
                Err(unsigned) => {
                    Step::Failed(first_line(&unsigned.to_string()))
                }
            };
        let _ = inputs.send(SignInStep { server, step });
    });
    Signing {
        address: None,
        pasted,
        task: task.abort_handle(),
    }
}

async fn sign_in(
    server: &str,
    url: &str,
    file: &Path,
    mut pasted: mpsc::Receiver<String>,
    inputs: &InputSender,
) -> Result<(), Unsigned> {
    // TLS runs on ring, installed once for the process.
    let _ =
        rustls::crypto::ring::default_provider().install_default();
    let loopback = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(Unsigned::Loopback)?;
    let port =
        loopback.local_addr().map_err(Unsigned::Loopback)?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let mut manager = AuthorizationManager::new(url).await?;
    manager.set_credential_store(Tokens::at(file));
    let mut state = OAuthState::Unauthorized(manager);
    state
        .start_authorization(
            AuthorizationRequest::new(&redirect)
                .with_client_name("Hamlet")
                .with_initial_scope_selection(),
        )
        .await?;
    let address = state.get_authorization_url().await?;
    inputs
        .send(SignInStep {
            server: server.to_owned(),
            step: Step::Address(address),
        })
        .map_err(|_| Unsigned::Stopping)?;
    let came = tokio::time::timeout(PATIENCE, async {
        tokio::select! {
            Some(address) = pasted.recv() => Some(address),
            Some(path) = called_back(&loopback) => {
                Some(format!("http://127.0.0.1:{port}{path}"))
            }
        }
    })
    .await
    .map_err(|_| Unsigned::Late)?
    .ok_or(Unsigned::Stopping)?;
    state.handle_callback_url(&came).await?;
    Ok(())
}

/// The path the chief's browser asked the loopback for, answered with
/// a page that says to go back to Hamlet; none once it cannot listen.
async fn called_back(loopback: &TcpListener) -> Option<String> {
    loop {
        let (mut stream, _) = loopback.accept().await.ok()?;
        let mut head = Vec::new();
        let mut byte = [0; 1];
        while !head.ends_with(b"\r\n\r\n") && head.len() < REQUEST {
            if stream.read(&mut byte).await.ok()? == 0 {
                break;
            }
            head.push(byte[0]);
        }
        let text = String::from_utf8_lossy(&head);
        let path = text
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|rest| rest.split(' ').next())
            .filter(|path| path.starts_with("/callback?"))
            .map(String::from);
        let page =
            "Signed in. Go back to Hamlet; this tab can close.";
        let _ = stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; \
                     charset=utf-8\r\nContent-Length: \
                     {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                )
                .as_bytes(),
            )
            .await;
        if path.is_some() {
            return path;
        }
    }
}

/// The first line of `words`, cut to `WORDS` characters: a server's
/// error page says little more, and at length.
fn first_line(words: &str) -> String {
    let line = words.lines().next().unwrap_or_default().trim_end();
    let line = line.trim_end_matches(':');
    match line.char_indices().nth(WORDS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    }
}

/// The tokens of one server, in their file.
pub(super) struct Tokens {
    file: PathBuf,
}

impl Tokens {
    pub fn at(file: &Path) -> Self {
        Self {
            file: file.to_owned(),
        }
    }
}

#[async_trait::async_trait]
impl CredentialStore for Tokens {
    async fn load(
        &self,
    ) -> Result<Option<StoredCredentials>, AuthError> {
        match tokio::fs::read(&self.file).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| stored(&error)),
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(stored(&error)),
        }
    }

    async fn save(
        &self,
        credentials: StoredCredentials,
    ) -> Result<(), AuthError> {
        let bytes = serde_json::to_vec(&credentials)
            .map_err(|error| stored(&error))?;
        let file = self.file.clone();
        tokio::task::spawn_blocking(move || {
            write_private(&file, &bytes)
        })
        .await
        .map_err(|error| stored(&error))?
        .map_err(|error| stored(&error))
    }

    async fn clear(&self) -> Result<(), AuthError> {
        match tokio::fs::remove_file(&self.file).await {
            Err(error)
                if error.kind() != std::io::ErrorKind::NotFound =>
            {
                Err(stored(&error))
            }
            _ => Ok(()),
        }
    }
}

fn stored(error: &impl std::fmt::Display) -> AuthError {
    AuthError::CredentialStoreError(error.to_string())
}

/// Writes `bytes` to `file` at once, readable by this account alone.
fn write_private(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let folder = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(folder)?;
    let partial = file.with_extension("partial");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&partial)?.write_all(bytes)?;
    std::fs::rename(&partial, file)
}
