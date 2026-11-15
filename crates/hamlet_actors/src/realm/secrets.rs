//! `credentials.json` in the realm's folder: the providers'
//! credentials an agent asks for and keeps, and the realm's secrets
//! as the workspace reads them.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{Ctx, Handler};
use hamlet_host::HostRef;
use hamlet_messages::address::{Agent, Workspace};
use hamlet_messages::realm::models::{
    Credential, CredentialAnswer, CredentialRefusal, KeepCredential,
    Kept, OauthLogin,
};
use hamlet_messages::realm::{SecretList, Secrets};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::reach::SecretsVersion;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::act::Look;
use super::world::World;
use super::{Realm, calls, change, documents, versions};

/// The version of a file that holds no counter, and of no file.
pub(super) const VERSION: SecretsVersion = SecretsVersion(1);

const FILE: &str = "credentials.json";

/// An OAuth login's fields that the message names; the file keeps
/// the provider's own beside them.
const OAUTH_FIELDS: [&str; 4] =
    ["type", "access", "refresh", "expires"];

/// A credential's fields that the workspace takes out.
const SECRET_FIELDS: [&str; 3] = ["key", "access", "refresh"];

/// `credentials.json` as today's server writes it, with the counter
/// of its versions.
#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    #[serde(default = "first")]
    version: SecretsVersion,
    /// Each provider's credential, by the provider's name.
    #[serde(default)]
    models: Map<String, Value>,
    /// The keys the realm does not read, kept as they are.
    #[serde(flatten)]
    other: Map<String, Value>,
}

fn first() -> SecretsVersion {
    VERSION
}

impl Stored {
    fn secrets(&self) -> impl Iterator<Item = &str> {
        ["connection", "workspace"]
            .into_iter()
            .filter_map(|key| self.other.get(key)?["token"].as_str())
            .chain(self.models.values().flat_map(secrets))
    }
}

fn secrets(credential: &Value) -> impl Iterator<Item = &str> {
    SECRET_FIELDS
        .iter()
        .filter_map(|field| credential[field].as_str())
}

/// The file, held in memory from the realm's start, with the values
/// replaced since then, which the workspace still takes out.
pub(super) struct Held {
    path: PathBuf,
    stored: RwLock<Arc<Stored>>,
    retired: Mutex<Vec<String>>,
}

impl Held {
    pub fn load(folder: &Path) -> Result<Self> {
        let path = folder.join(FILE);
        let stored = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("reading {FILE}"))?,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                Stored {
                    version: VERSION,
                    models: Map::new(),
                    other: Map::new(),
                }
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            stored: RwLock::new(Arc::new(stored)),
            retired: Mutex::new(Vec::new()),
        })
    }

    fn current(&self) -> Arc<Stored> {
        self.stored
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[expect(
        dead_code,
        reason = "the reach takes it once the realm holds the file"
    )]
    pub fn version(&self) -> SecretsVersion {
        self.current().version
    }

    #[expect(
        dead_code,
        reason = "`credential` reads it once the realm holds the \
                  file"
    )]
    fn answer(&self, provider: &str) -> CredentialAnswer {
        self.current()
            .models
            .get(provider)
            .and_then(given)
            .unwrap_or_else(|| CredentialAnswer::Refused {
                why: CredentialRefusal::NotSignedIn {
                    provider: provider.to_owned(),
                },
            })
    }

    /// Longest first, so a value inside another is taken out after
    /// it.
    #[expect(
        dead_code,
        reason = "`secrets` reads it once the realm holds the file"
    )]
    fn list(&self) -> SecretList {
        let stored = self.current();
        let retired = self
            .retired
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut values = stored
            .secrets()
            .chain(retired.iter().map(String::as_str))
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        values.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        values.dedup();
        SecretList {
            version: stored.version,
            values,
        }
    }

    /// Writes the file whole with `keep` in it, then holds it. The
    /// caller makes keeps one at a time.
    fn keep(&self, keep: &KeepCredential) -> Result<()> {
        let mut stored = Stored::clone(&self.current());
        let value = saved(&keep.value)?;
        let before = stored
            .models
            .insert(keep.provider.clone(), value.clone());
        if before.as_ref() != Some(&value) {
            stored.version = SecretsVersion(stored.version.0 + 1);
            self.retired
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(
                    before
                        .iter()
                        .flat_map(secrets)
                        .map(str::to_owned),
                );
        }
        write_whole(
            &self.path,
            &serde_json::to_vec_pretty(&stored)?,
        )?;
        *self
            .stored
            .write()
            .unwrap_or_else(PoisonError::into_inner) =
            Arc::new(stored);
        Ok(())
    }
}

/// The credential as the agent reads it; `None` for one it cannot.
fn given(value: &Value) -> Option<CredentialAnswer> {
    match value["type"].as_str()? {
        "api_key" => Some(CredentialAnswer::Key {
            key: value["key"].as_str()?.to_owned(),
        }),
        "oauth" => {
            let mut extra = value.as_object()?.clone();
            extra.retain(|field, _| {
                !OAUTH_FIELDS.contains(&field.as_str())
            });
            let expires_ms = value["expires"].as_f64()?;
            Some(CredentialAnswer::Oauth {
                login: OauthLogin {
                    access: value["access"].as_str()?.to_owned(),
                    refresh: value["refresh"].as_str()?.to_owned(),
                    // `as` saturates, so a login that never expires
                    // reads as the last second a `u32` holds.
                    expires: (expires_ms / 1000.0) as u32,
                    extra: (!extra.is_empty())
                        .then(|| Value::Object(extra).to_string()),
                },
            })
        }
        _ => None,
    }
}

/// The credential as the file keeps it: an OAuth login's own fields
/// beside the others, its expiry in milliseconds.
fn saved(kept: &Kept) -> Result<Value> {
    Ok(match kept {
        Kept::Key { key } => json!({ "type": "api_key", "key": key }),
        Kept::Oauth { login } => {
            let mut fields = match &login.extra {
                Some(extra) => match serde_json::from_str(extra)? {
                    Value::Object(fields) => fields,
                    _ => bail!("an OAuth login's extra is no object"),
                },
                None => Map::new(),
            };
            fields.insert("type".into(), "oauth".into());
            fields
                .insert("access".into(), login.access.clone().into());
            fields.insert(
                "refresh".into(),
                login.refresh.clone().into(),
            );
            fields.insert(
                "expires".into(),
                (u64::from(login.expires) * 1000).into(),
            );
            Value::Object(fields)
        }
    })
}

/// Writes `bytes` to a new file with mode 0600 beside `file`, then
/// renames it over `file`.
fn write_whole(file: &Path, bytes: &[u8]) -> Result<()> {
    let temporary =
        file.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let written = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut output = options.open(&temporary)?;
        output.write_all(bytes)?;
        output.sync_all()?;
        fs::rename(&temporary, file)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    Ok(written?)
}

impl Look<Workspace> for Secrets {
    type Output = SecretList;

    fn look(&self, _by: &Workspace, _world: &World) -> SecretList {
        SecretList {
            version: VERSION,
            values: Vec::new(),
        }
    }
}

impl Look<Agent> for Credential {
    type Output = CredentialAnswer;

    fn look(&self, _by: &Agent, _world: &World) -> CredentialAnswer {
        CredentialAnswer::Refused {
            why: CredentialRefusal::NotSignedIn {
                provider: self.provider.clone(),
            },
        }
    }
}

/// Writes the file before it commits, so it is the one handler
/// outside `Act`. The transaction, begun first, makes keeps one at a
/// time.
impl Handler<KeepCredential> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        keep: KeepCredential,
    ) -> Result<()> {
        let Agent(_) = sender(&ctx, &keep)?;
        let tx = change::Transaction::begin(&ctx).await?;
        let held = Held::load(HostRef::of(ctx.inner())?.folder())?;
        let living = documents::living(&tx.rows()).await?;
        held.keep(&keep)?;
        let raises = living
            .into_iter()
            .map(|agent| versions::Change::WantReach(agent).into())
            .collect();
        let pushes = tx
            .commit(
                &self,
                change::Write::<()> {
                    changes: raises,
                    entries: Vec::new(),
                    marker: None,
                },
            )
            .await?;
        ctx.register_task(calls::after_answer(
            self.clone(),
            ctx.clone(),
            Vec::new(),
            pushes,
        ));
        Ok(())
    }
}
