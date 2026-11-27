//! Whole-file credentials and scrubbing at the protocol boundary
//! (S7).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use hamlet_sandbox::scrub;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    installation_id: String,
    connection: Connection,
    models: serde_json::Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<WorkspaceAddress>,
}

#[derive(Serialize, Deserialize)]
struct Connection {
    token: String,
}

/// A remote workspace's url and token.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct WorkspaceAddress {
    pub url: String,
    pub token: String,
}

/// The realm's credential file and its in-memory secret history.
pub struct Credentials {
    file: PathBuf,
    stored: Stored,
    api_key: Option<String>,
    workspace_token: Option<String>,
    transient: Vec<String>,
}

impl Credentials {
    /// Opens the credentials of a realm whose actors own the file,
    /// writing it only to make it.
    pub(crate) fn open(root: &Path) -> Result<Self> {
        let file = root.join("credentials.json");
        match read(&file)? {
            Some(stored) => Self::holding(file, stored),
            None => {
                let credentials = Self::holding(file, made()?)?;
                credentials.write()?;
                Ok(credentials)
            }
        }
    }

    fn holding(file: PathBuf, stored: Stored) -> Result<Self> {
        let credentials = Self {
            file,
            transient: stored
                .workspace
                .iter()
                .map(|w| w.token.clone())
                .collect(),
            stored,
            api_key: std::env::var("HAMLET_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            workspace_token: std::env::var("HAMLET_WORKSPACE_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
        };
        ensure!(
            !credentials.stored.connection.token.is_empty(),
            "credentials.json has an empty token"
        );
        Ok(credentials)
    }

    pub(crate) fn is_connection_token(&self, token: &str) -> bool {
        // Both hashes are fixed length, independent of the token's
        // spelling.
        let a = Sha256::digest(token.as_bytes());
        let b =
            Sha256::digest(self.stored.connection.token.as_bytes());
        #[expect(
            deprecated,
            reason = "ring's is the only constant-time compare here"
        )]
        ring::constant_time::verify_slices_are_equal(&a, &b).is_ok()
    }

    pub(crate) fn workspace(&self) -> Option<&WorkspaceAddress> {
        self.stored.workspace.as_ref()
    }

    pub(crate) fn remember(&mut self, secret: &str) {
        if !secret.is_empty()
            && !self.transient.iter().any(|s| s == secret)
        {
            self.transient.push(secret.into());
        }
    }

    pub(crate) fn model(
        &self,
        provider: &str,
        serving: &str,
    ) -> Option<Value> {
        if provider == serving
            && let Some(key) = &self.api_key
        {
            return Some(
                serde_json::json!({"type":"api_key", "key":key}),
            );
        }
        self.stored.models.get(provider).cloned()
    }

    fn write(&self) -> Result<()> {
        write_whole(
            &self.file,
            &serde_json::to_vec_pretty(&self.stored)?,
        )
    }

    /// The secrets kept here, longest first, which nothing shown or
    /// kept may hold.
    pub fn secrets(&self) -> Vec<String> {
        let mut secrets = vec![self.stored.connection.token.as_str()];
        secrets.extend(self.api_key.as_deref());
        secrets.extend(self.workspace_token.as_deref());
        secrets.extend(self.transient.iter().map(String::as_str));
        secrets.extend(
            self.stored.workspace.as_ref().map(|w| w.token.as_str()),
        );
        for model in self.stored.models.values() {
            for key in ["key", "access", "refresh"] {
                secrets.extend(model[key].as_str());
            }
        }
        secrets.retain(|s| !s.is_empty());
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets.into_iter().map(String::from).collect()
    }

    pub(crate) fn scrub(&self, value: &mut Value) {
        scrub::scrub_value(&self.secrets(), value);
    }

    /// `text` with every secret removed.
    pub(crate) fn scrub_text(&self, text: &str) -> String {
        scrub::scrub_text(&self.secrets(), text)
    }
}

fn read(file: &Path) -> Result<Option<Stored>> {
    match fs::read(file) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes)
                .context("reading credentials.json")?,
        )),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

/// A new installation's file, with a new connection token.
fn made() -> Result<Stored> {
    let mut random = [0; 32];
    SystemRandom::new().fill(&mut random).map_err(|_| {
        anyhow::anyhow!("cannot make a connection token")
    })?;
    Ok(Stored {
        installation_id: uuid::Uuid::new_v4().to_string(),
        connection: Connection {
            token: random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        },
        models: Default::default(),
        workspace: None,
    })
}

pub(crate) fn write_whole(file: &Path, bytes: &[u8]) -> Result<()> {
    let temporary =
        file.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&temporary)?;
        output.write_all(bytes)?;
        output.sync_all()?;
        fs::rename(&temporary, file)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
