//! Whole-file credentials and scrubbing at the protocol boundary
//! (S7).

use std::collections::HashMap;
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

/// The home's credential file and its in-memory secret history.
pub struct Credentials {
    file: PathBuf,
    stored: Stored,
    api_key: Option<String>,
    workspace_token: Option<String>,
    transient: Vec<String>,
    revisions: HashMap<String, u64>,
}

impl Credentials {
    /// Opens the credential owner for a home held by this process.
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
            revisions: HashMap::new(),
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
        credentials.write()?;
        Ok(credentials)
    }

    pub(crate) fn accepts(&self, token: &str) -> bool {
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

    pub(crate) fn workspace_url(&self) -> Option<&str> {
        self.workspace().map(|w| w.url.as_str())
    }

    pub(crate) fn workspace(&self) -> Option<&Workspace> {
        self.stored.workspace.as_ref()
    }

    pub(crate) fn installation_id(&self) -> &str {
        &self.stored.installation_id
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

    pub(crate) fn configured(&self, provider: &str) -> bool {
        self.stored.models.contains_key(provider)
    }

    pub(crate) fn revision(&self, provider: &str) -> u64 {
        self.revisions.get(provider).copied().unwrap_or_default()
    }

    pub(crate) fn remember_model(&mut self, value: &Value) {
        for key in ["key", "access", "refresh"] {
            if let Some(secret) = value[key].as_str() {
                self.remember(secret);
            }
        }
    }

    pub(crate) fn save_model(
        &mut self,
        provider: &str,
        credential: Option<Value>,
    ) -> Result<()> {
        let previous = self.stored.models.get(provider).cloned();
        for value in previous.iter().chain(credential.iter()) {
            self.remember_model(value);
        }
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
        *self.revisions.entry(provider.into()).or_default() += 1;
        Ok(())
    }

    pub(crate) fn choose_workspace(
        &mut self,
        workspace: Option<Workspace>,
    ) -> Result<()> {
        if let Some(target) = &workspace {
            self.remember(&target.token);
        }
        let previous =
            std::mem::replace(&mut self.stored.workspace, workspace);
        if let Some(active) = &previous {
            self.remember(&active.token);
        }
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
        let secrets = self.secrets();
        let secrets =
            secrets.iter().map(String::as_str).collect::<Vec<_>>();
        scrub_value(value, &secrets);
    }

    /// `text` with every secret removed.
    pub(crate) fn scrub_text(&self, text: &str) -> String {
        let mut value = Value::String(text.into());
        self.scrub(&mut value);
        match value {
            Value::String(text) => text,
            _ => String::new(),
        }
    }
}

pub(crate) fn pack(
    mut value: Value,
) -> Result<hamlet_protocol::runtime::Credential> {
    if value["type"] == "oauth" {
        let mut extra =
            value.as_object().context("OAuth credential")?.clone();
        for key in ["type", "access", "refresh", "expires"] {
            extra.remove(key);
        }
        let fields =
            value.as_object_mut().context("OAuth credential")?;
        fields.retain(|key, _| {
            ["type", "access", "refresh", "expires"]
                .contains(&key.as_str())
        });
        fields.insert("extra".into(), Value::Object(extra));
    }
    Ok(serde_json::from_value(value)?)
}

pub(crate) fn unpack(
    credential: hamlet_protocol::runtime::Credential,
) -> Result<Value> {
    let mut value = serde_json::to_value(credential)?;
    if value["type"] == "oauth" {
        let fields =
            value.as_object_mut().context("OAuth credential")?;
        if let Some(Value::Object(extra)) = fields.remove("extra") {
            for (key, item) in extra {
                fields.entry(key).or_insert(item);
            }
        }
    }
    Ok(value)
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
            values.iter_mut().for_each(|v| scrub_value(v, secrets));
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

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;
    use serde_json::json;

    use super::*;

    #[test]
    fn active_and_retired_credentials_stay_scrubbed_after_replacement_and_logout()
     {
        let dir = Scratch::new("credential-retention");
        let mut credentials = Credentials::open(&dir).unwrap();
        credentials.workspace_token =
            Some("environment-workspace-token".into());
        credentials
            .choose_workspace(Some(Workspace {
                url: "https://first.invalid".into(),
                token: "startup-workspace-token".into(),
            }))
            .unwrap();
        drop(credentials);
        let mut credentials = Credentials::open(&dir).unwrap();
        credentials.workspace_token =
            Some("environment-workspace-token".into());
        credentials
            .choose_workspace(Some(Workspace {
                url: "https://second.invalid".into(),
                token: "target-workspace-token".into(),
            }))
            .unwrap();
        credentials.save_model("chatgpt", Some(json!({"type":"oauth","access":"before-access-token","refresh":"before-refresh-token","expires":0}))).unwrap();
        credentials.save_model("chatgpt", Some(json!({"type":"oauth","access":"after-access-token","refresh":"after-refresh-token","expires":9}))).unwrap();
        let mut text = json!(
            "startup-workspace-token target-workspace-token \
             environment-workspace-token before-access-token \
             before-refresh-token after-access-token \
             after-refresh-token untouched"
        );
        credentials.scrub(&mut text);
        assert_eq!(
            text,
            "[REDACTED] [REDACTED] [REDACTED] [REDACTED] [REDACTED] \
             [REDACTED] [REDACTED] untouched"
        );
        credentials.choose_workspace(None).unwrap();
        credentials.save_model("chatgpt", None).unwrap();
        let mut retired = json!({"before-refresh-token":"startup-workspace-token target-workspace-token after-access-token environment-workspace-token"});
        credentials.scrub(&mut retired);
        assert_eq!(
            retired,
            json!({"[REDACTED]":"[REDACTED] [REDACTED] [REDACTED] [REDACTED]"})
        );
        let disk =
            fs::read_to_string(dir.join("credentials.json")).unwrap();
        assert!(!disk.contains("workspace-token"));
        assert!(!disk.contains("access-token"));
    }
}
