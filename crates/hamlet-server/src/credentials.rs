//! Whole-file credentials and scrubbing at the protocol boundary
//! (S7).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
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
    workspace: Option<Workspace>,
}

#[derive(Serialize, Deserialize)]
struct Connection {
    token: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Workspace {
    pub url: String,
    pub token: String,
}

pub(crate) struct Credentials {
    file: PathBuf,
    stored: Stored,
    api_key: Option<String>,
}

impl Credentials {
    pub fn open(root: &Path) -> Result<Self> {
        let file = root.join("credentials.json");
        let stored = match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("reading credentials.json")?,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                let mut random = [0; 32];
                SystemRandom::new().fill(&mut random).map_err(
                    |_| {
                        anyhow::anyhow!(
                            "cannot make a connection token"
                        )
                    },
                )?;
                Stored {
                    installation_id: uuid::Uuid::new_v4().to_string(),
                    connection: Connection {
                        token: random
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect(),
                    },
                    models: Default::default(),
                    workspace: None,
                }
            }
            Err(error) => return Err(error.into()),
        };
        let credentials = Self {
            file,
            stored,
            api_key: std::env::var("HAMLET_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
        };
        ensure!(
            !credentials.stored.connection.token.is_empty(),
            "credentials.json has an empty token"
        );
        credentials.write()?;
        Ok(credentials)
    }

    pub fn accepts(&self, token: &str) -> bool {
        // Both hashes are fixed length, independent of the token's
        // spelling.
        let a = Sha256::digest(token.as_bytes());
        let b =
            Sha256::digest(self.stored.connection.token.as_bytes());
        #[allow(deprecated)]
        ring::constant_time::verify_slices_are_equal(&a, &b).is_ok()
    }

    pub fn workspace_url(&self) -> Option<&str> {
        self.stored.workspace.as_ref().map(|w| w.url.as_str())
    }

    pub fn model(
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

    pub fn configured(&self, provider: &str) -> bool {
        self.stored.models.contains_key(provider)
    }

    pub fn save_model(
        &mut self,
        provider: &str,
        credential: Option<Value>,
    ) -> Result<()> {
        let previous = self.stored.models.get(provider).cloned();
        match credential {
            Some(value) => {
                self.stored.models.insert(provider.into(), value);
            }
            None => {
                self.stored.models.remove(provider);
            }
        }
        if let Err(error) = self.write() {
            match previous {
                Some(value) => {
                    self.stored.models.insert(provider.into(), value);
                }
                None => {
                    self.stored.models.remove(provider);
                }
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn choose_workspace(
        &mut self,
        workspace: Option<Workspace>,
    ) -> Result<()> {
        let previous =
            std::mem::replace(&mut self.stored.workspace, workspace);
        if let Err(error) = self.write() {
            self.stored.workspace = previous;
            return Err(error);
        }
        Ok(())
    }

    fn write(&self) -> Result<()> {
        write_whole(
            &self.file,
            &serde_json::to_vec_pretty(&self.stored)?,
        )
    }

    pub fn scrub(&self, value: &mut Value) {
        let mut secrets = vec![self.stored.connection.token.as_str()];
        secrets.extend(self.api_key.as_deref());
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
        scrub_value(value, &secrets);
    }
}

fn scrub_value(value: &mut Value, secrets: &[&str]) {
    match value {
        Value::String(text) => {
            let mut clean = String::new();
            let mut rest = text.as_str();
            while !rest.is_empty() {
                if let Some(secret) =
                    secrets.iter().find(|s| rest.starts_with(**s))
                {
                    clean.push_str("[REDACTED]");
                    rest = &rest[secret.len()..];
                } else {
                    let c =
                        rest.chars().next().expect("nonempty text");
                    clean.push(c);
                    rest = &rest[c.len_utf8()..];
                }
            }
            *text = clean;
        }
        Value::Array(values) => {
            values.iter_mut().for_each(|v| scrub_value(v, secrets))
        }
        Value::Object(values) => {
            let old = std::mem::take(values);
            for (key, mut value) in old {
                let mut key = Value::String(key);
                scrub_value(&mut key, secrets);
                scrub_value(&mut value, secrets);
                values.insert(
                    key.as_str().expect("string key").into(),
                    value,
                );
            }
        }
        _ => {}
    }
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
