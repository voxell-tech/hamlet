//! `credentials.json` in the realm's folder: the providers'
//! credentials an agent asks for and keeps, and the realm's secrets
//! as the workspace reads them.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::{Agent, Workspace};
use hamlet_messages::realm::documents::RemoteWorkspace;
use hamlet_messages::realm::models::{
    Credential, CredentialAnswer, CredentialRefusal, KeepCredential,
    Kept, OauthLogin,
};
use hamlet_messages::realm::{SecretList, Secrets};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::reach::SecretsVersion;
use hamlet_os::fs::{Directory, LastLink, Mode, Name, Open, Replace};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::act::Look;
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
    /// The workspace on another machine the chief chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<RemoteWorkspace>,
    /// The keys the realm does not read, kept as they are.
    #[serde(flatten)]
    other: Map<String, Value>,
}

fn first() -> SecretsVersion {
    VERSION
}

impl Stored {
    fn secrets(&self) -> impl Iterator<Item = &str> {
        self.other
            .get("connection")
            .and_then(|connection| connection["token"].as_str())
            .into_iter()
            .chain(self.workspace.iter().map(|w| w.token.as_str()))
            .chain(self.models.values().flat_map(secrets))
    }
}

fn secrets(credential: &Value) -> impl Iterator<Item = &str> {
    SECRET_FIELDS
        .iter()
        .filter_map(|field| credential[field].as_str())
}

/// The file, held in memory from the realm's start.
pub(super) struct Held {
    folder: PathBuf,
    held: Mutex<InMemory>,
}

struct InMemory {
    stored: Stored,
    /// The values replaced since the start, which the workspace
    /// still takes out.
    retired: Vec<String>,
}

impl Held {
    /// Reads the file, and writes it back at mode 0600 where there is
    /// one.
    pub fn load(folder: &Path) -> Result<Self> {
        let stored = match std::fs::read(folder.join(FILE)) {
            Ok(bytes) => {
                let stored = serde_json::from_slice(&bytes)
                    .with_context(|| format!("reading {FILE}"))?;
                write_whole(folder, &stored)?;
                stored
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                Stored {
                    version: VERSION,
                    models: Map::new(),
                    workspace: None,
                    other: Map::new(),
                }
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            folder: folder.to_owned(),
            held: Mutex::new(InMemory {
                stored,
                retired: Vec::new(),
            }),
        })
    }

    fn held(&self) -> MutexGuard<'_, InMemory> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn version(&self) -> SecretsVersion {
        self.held().stored.version
    }

    fn keep(&self, keep: &KeepCredential) -> Result<()> {
        let value = saved(&keep.value)?;
        self.change(|stored| {
            let models = &mut stored.models;
            let value = match models.get(&keep.provider) {
                Some(before) => unchanged_expiry(before, value),
                None => value,
            };
            let before = models.insert(keep.provider.clone(), value);
            let value = &models[&keep.provider];
            (before.as_ref() != Some(value)).then(|| {
                before
                    .iter()
                    .flat_map(secrets)
                    .map(str::to_owned)
                    .collect()
            })
        })
    }

    /// Keeps `workspace` as the chosen one; `None` takes it away.
    pub fn choose_workspace(
        &self,
        workspace: Option<RemoteWorkspace>,
    ) -> Result<()> {
        self.change(|stored| {
            let before =
                std::mem::replace(&mut stored.workspace, workspace);
            (before != stored.workspace).then(|| {
                before
                    .map(|before| before.token)
                    .into_iter()
                    .collect()
            })
        })
    }

    pub fn forget(&self, providers: &[String]) -> Result<()> {
        self.change(|stored| {
            let dropped = providers
                .iter()
                .filter_map(|provider| stored.models.remove(provider))
                .collect::<Vec<_>>();
            (!dropped.is_empty()).then(|| {
                dropped
                    .iter()
                    .flat_map(secrets)
                    .map(str::to_owned)
                    .collect()
            })
        })
    }

    /// Writes the file whole with what `change` makes of it, then
    /// holds it. `change` answers the values it replaced, or `None`
    /// when it changed nothing, which keeps the version. The caller
    /// makes changes one at a time.
    fn change(
        &self,
        change: impl FnOnce(&mut Stored) -> Option<Vec<String>>,
    ) -> Result<()> {
        let mut held = self.held();
        let mut stored = held.stored.clone();
        let retired = change(&mut stored);
        if retired.is_some() {
            stored.version = SecretsVersion(stored.version.0 + 1);
        }
        write_whole(&self.folder, &stored)?;
        held.stored = stored;
        held.retired.extend(retired.into_iter().flatten());
        Ok(())
    }
}

/// `kept` with `before`'s expiry when they name the same second, so a
/// login read and kept back is no change: today's server keeps the
/// expiry to the millisecond.
fn unchanged_expiry(before: &Value, mut kept: Value) -> Value {
    let second = |value: &Value| {
        value["expires"].as_f64().map(|ms| (ms / 1000.0) as u32)
    };
    if before["type"] == "oauth"
        && kept["type"] == "oauth"
        && second(before) == second(&kept)
    {
        kept["expires"] = before["expires"].clone();
    }
    kept
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

/// Writes `stored` to a new file at mode 0600 in `folder`, then
/// renames it over the file.
fn write_whole(folder: &Path, stored: &Stored) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(stored)?;
    let folder = Directory::open(folder, LastLink::Follow)?;
    let temporary =
        Name::new(&format!("{FILE}.{}.tmp", uuid::Uuid::new_v4()))?;
    let written = (|| {
        let mut file = folder
            .file(&temporary, Open::New(Mode::PRIVATE))?
            .into_file();
        file.write_all(&bytes)?;
        file.sync_all()?;
        folder.rename(
            &temporary,
            &folder,
            &Name::new(FILE)?,
            Replace::Always,
        )
    })();
    if written.is_err() {
        let _ =
            folder.remove(&temporary, hamlet_os::fs::Removing::File);
    }
    Ok(written?)
}

impl Look<Workspace> for Secrets {
    type Output = SecretList;

    fn look(&self, _by: &Workspace, realm: &Realm) -> SecretList {
        let held = realm.secrets.held();
        let mut values = held
            .stored
            .secrets()
            .chain(held.retired.iter().map(String::as_str))
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        values.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        values.dedup();
        SecretList {
            version: held.stored.version,
            values,
        }
    }
}

impl Look<Agent> for Credential {
    type Output = CredentialAnswer;

    fn look(&self, _by: &Agent, realm: &Realm) -> CredentialAnswer {
        realm
            .secrets
            .held()
            .stored
            .models
            .get(&self.provider)
            .and_then(given)
            .unwrap_or_else(|| CredentialAnswer::Refused {
                why: CredentialRefusal::NotSignedIn {
                    provider: self.provider.clone(),
                },
            })
    }
}

impl Handler<KeepCredential> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        keep: KeepCredential,
    ) -> Result<()> {
        let Agent(_) = sender(&ctx, &keep)?;
        let tx = change::Transaction::begin(&ctx).await?;
        let raises = raises(&tx.rows()).await?;
        self.secrets.keep(&keep)?;
        commit(&self, &ctx, tx, raises).await
    }
}

/// The raises of every living agent's reach, which then asks for the
/// secrets of the version kept.
pub(super) async fn raises(
    rows: &change::Rows<'_>,
) -> Result<Vec<change::Change>> {
    Ok(documents::living(rows)
        .await?
        .into_iter()
        .map(|agent| versions::Change::WantReach(agent).into())
        .collect())
}

/// Commits `changes` in `tx`, and makes the pushes they raise.
pub(super) async fn commit(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    tx: change::Transaction,
    changes: Vec<change::Change>,
) -> Result<()> {
    let write = change::Write::<()> {
        changes,
        entries: Vec::new(),
        marker: None,
    };
    let pushes = tx.commit(realm, write).await?;
    ctx.register_task(calls::after_answer(
        realm.clone(),
        ctx.clone(),
        Vec::new(),
        pushes,
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::HamletId;
    use hamlet_messages::workspace::AgentNumber;

    use super::super::change::Rows;
    use super::super::testing::Started;
    use super::super::{reach, versions};
    use super::*;

    const HIRE: AgentNumber = AgentNumber(2);

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_reach_asks_for_the_secrets_of_its_version() {
        let started = Started::new().await;
        started.found(&HamletId("h".to_owned()), HIRE).await;
        started
            .commit(versions::Change::WantReach(HIRE))
            .await
            .unwrap();
        for n in 0..200 {
            let (before, _) =
                versions::reach(&started.rows(), HIRE).await.unwrap();
            let (realm, ctx) =
                (started.realm.clone(), started.ctx.clone());
            let read = tokio::spawn(async move {
                let rows = Rows::outside(&ctx)?;
                reach::of(&rows, HIRE, &realm.secrets).await
            });
            let tx = change::Transaction::begin(&started.ctx)
                .await
                .unwrap();
            let raises = raises(&tx.rows()).await.unwrap();
            let keep = KeepCredential {
                provider: "p".to_owned(),
                value: Kept::Key {
                    key: format!("k{n}"),
                },
            };
            started.realm.secrets.keep(&keep).unwrap();
            commit(&started.realm, &started.ctx, tx, raises)
                .await
                .unwrap();
            let set = read.await.unwrap().unwrap();
            let secrets = set.reach.footing().unwrap().secrets;
            assert!(
                set.version == before
                    || secrets == started.realm.secrets.version(),
                "{set:?} under the keep's version asks for the old \
                 secrets"
            );
        }
        started.stop().await;
    }
}
