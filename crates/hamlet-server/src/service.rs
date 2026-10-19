//! Protocol requests composed from the home's owners.

use anyhow::{Context, Result, ensure};
use hamlet_protocol::v0::{
    Address, ClientMessage, Read, ServerMessage, State,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::credentials::Credentials;
use crate::limits::Rules;
use crate::papers::Houses;
use crate::protocol::{Refusal, fault, reply};
use crate::record::{self, FoundRecord};
use crate::settings::Overrides;
use crate::{Home, OperationId, Outcome, Reply, RuntimeInstance};

/// The served home and its chief-facing requests.
pub struct Service {
    pub home: Home,
    pub(crate) chief: String,
    pub(crate) credentials: Credentials,
    pub(crate) limits: Rules,
    pub(crate) houses: Houses,
    pub(crate) overrides: Overrides,
    pub(crate) models: crate::models::Models,
    pub(crate) chats: crate::chats::Chats,
    pub(crate) gifts: crate::gifts::Uploads,
    pub(crate) moving: crate::papers::moving::Moving,
    pub(crate) house_work: crate::society::work::Work,
    pub(crate) merges: crate::society::merging::remote::Merges,
}

impl Service {
    /// Opens the chief's documents and the first hamlet, retaining
    /// the recorded chief on restart.
    pub fn new(mut home: Home, faux: bool) -> Result<Self> {
        let root = home.layout().root().to_owned();
        let credentials = Credentials::open(&root)?;
        let overrides = Overrides::from_env(faux)?;
        let workspace = overrides
            .workspace
            .as_ref()
            .or(credentials.workspace())
            .cloned();
        if let Some(connection) = &workspace {
            crate::settings::check_workspace(connection)
                .map_err(anyhow::Error::msg)?;
        }
        let chief = home
            .store()
            .record("home")?
            .and_then(|r| r.chiefs.first().cloned())
            .unwrap_or_else(|| {
                ["HAMLET_USER", "USER"]
                    .into_iter()
                    .find_map(|key| {
                        std::env::var(key)
                            .ok()
                            .filter(|name| !name.is_empty())
                    })
                    .unwrap_or_else(|| "user".into())
            });
        if home.store().record("home")?.is_none() {
            let result = home.store().run(
                &OperationId {
                    runtime: RuntimeInstance::new("server")?,
                    key: "found-home".into(),
                },
                &FoundRecord {
                    hamlet: "home".into(),
                    name: "founder".into(),
                    purpose: String::new(),
                    chief: chief.clone(),
                    at: 0,
                },
            )?;
            ensure!(
                matches!(result, Reply::Outcome(Outcome::Done(_))),
                "the first hamlet could not be founded"
            );
        }
        let mut houses = Houses::open(&root)?;
        houses.workspace = workspace;
        let mut realm =
            crate::realm::read(home.store().connection())?;
        for place in realm.hamlets.iter().filter(|place| place.erased)
        {
            if houses.workspace.is_none() {
                houses.erase_hamlet(&place.id)?;
            }
        }
        if let Some(pending) = &realm.pending {
            ensure!(
                houses.workspace.is_none(),
                "An unfinished local house move needs the original \
                 local backend. Remote houses were not changed."
            );
            for binding in crate::chats::records::conversations_of(
                home.store().connection(),
                &pending.to,
            )? {
                home.store().begin_cancel(binding)?;
            }
            houses.move_house(&pending.from, &pending.to)?;
            realm.pending = None;
            crate::realm::write(home.store().connection(), &realm)?;
        }
        let live = crate::realm::live(home.store().connection())?;
        for record in records(home.store().connection())? {
            if !live.contains(&record.id) {
                continue;
            }
            for agent in record.agents {
                if houses.workspace.is_none() {
                    houses.build(&record.id, agent.number)?;
                }
            }
        }
        for directory in ["library", "tools", "librarian"] {
            std::fs::create_dir_all(root.join(directory))?;
        }
        crate::prompt::stock(&root.join("library"));
        Ok(Self {
            home,
            chief,
            credentials,
            houses,
            limits: Rules::new()?,
            overrides,
            models: crate::models::Models::open(&root, faux)?,
            chats: crate::chats::Chats::new(
                crate::sandbox::Sandbox::platform(),
            ),
            gifts: crate::gifts::Uploads::default(),
            moving: crate::papers::moving::Moving::default(),
            house_work: crate::society::work::Work::default(),
            merges: crate::society::merging::remote::Merges::default(
            ),
        })
    }

    /// The state visible to a newly authenticated client.
    pub fn state(
        &mut self,
        epoch: i64,
        revision: i64,
    ) -> Result<State> {
        crate::state::state(self, epoch, revision)
    }

    /// Handles one authenticated request; an unsupported owner
    /// returns a typed refusal.
    pub async fn handle(
        &mut self,
        message: ClientMessage,
    ) -> Result<Option<ServerMessage>> {
        let raw = serde_json::to_value(&message)?;
        let id = raw["id"].as_str().unwrap_or_default();
        let unsupported =
            |part| reply(id, Err(Refusal::Unsupported(part)));
        Ok(Some(match message {
            ClientMessage::Hello(_) => {
                fault(None, "This connection said hello already.")
            }
            ClientMessage::Close(_) => return Ok(None),
            ClientMessage::Edit(edit) => {
                reply(id, self.edit(edit).await?)
            }
            ClientMessage::Read(read) => self.read(read).await?,
            // The runtime hands these to the chats first.
            ClientMessage::Say(_)
            | ClientMessage::Open(_)
            | ClientMessage::New(_)
            | ClientMessage::Resume(_)
            | ClientMessage::Retry(_)
            | ClientMessage::Rewind(_)
            | ClientMessage::Stop(_)
            | ClientMessage::Answer(_) => {
                unsupported("conversations")
            }
            ClientMessage::Give(_) | ClientMessage::GivePart(_) => {
                unsupported("gifts")
            }
            ClientMessage::Login(_) => unsupported("sign-ins"),
            ClientMessage::Check(_) => unsupported("model checks"),
            ClientMessage::MoveHouses { .. } => {
                match self.start_move(0, id)? {
                    Ok(()) => return Ok(None),
                    Err(error) => reply(id, Err(error)),
                }
            }
        }))
    }

    pub(crate) fn access(
        &mut self,
        hamlet: &str,
        operation: &str,
    ) -> Result<std::result::Result<record::Hamlet, Refusal>> {
        if let Some(place) =
            crate::realm::read(self.home.store().connection())?
                .hamlets
                .iter()
                .find(|h| h.id == hamlet)
            && (place.removed.is_some() || place.erased)
        {
            return Ok(Err(Refusal::Invalid(format!(
                "The hamlet `{hamlet}` was removed; ask a chief to \
                 restore it."
            ))));
        }
        let Some(record) = self.home.store().record(hamlet)? else {
            return Ok(Err(Refusal::Invalid(format!(
                "Cannot {operation}: no hamlet named `{hamlet}`. \
                 Choose a living hamlet."
            ))));
        };
        if !record.chiefs.is_empty()
            && !record.chiefs.contains(&self.chief)
        {
            return Ok(Err(Refusal::Invalid(format!(
                "Cannot {operation}: Only a chief of that hamlet \
                 may reach its agents. Ask one of its chiefs."
            ))));
        }
        Ok(Ok(record))
    }

    pub(crate) fn agent(
        &mut self,
        address: &Address,
        operation: &str,
    ) -> Result<std::result::Result<record::Agent, Refusal>> {
        let record = match self.access(&address.hamlet, operation)? {
            Ok(record) => record,
            Err(error) => return Ok(Err(error)),
        };
        let found = record.agents.iter().find(|a| {
            if address.agent == "mayor" {
                a.number == record.mayor
            } else {
                a.name == address.agent
            }
        });
        Ok(match found {
            Some(agent) => Ok(agent.clone()),
            None => Err(Refusal::Invalid(
                record
                    .archive
                    .iter()
                    .rev()
                    .find(|a| {
                        a.back.is_none()
                            && a.agent.name == address.agent
                    })
                    .map(|a| record.farewell(a))
                    .unwrap_or_else(|| {
                        format!("no agent named `{}`", address.agent)
                    }),
            )),
        })
    }

    pub(crate) fn settings(&mut self) -> Result<Value> {
        let stored = document(
            self.home.store().connection(),
            "settings",
            "home",
        )?
        .unwrap_or_else(crate::settings::defaults);
        Ok(self.overrides.apply(stored))
    }

    pub(crate) fn remote_tools(
        &mut self,
        agent: &record::AgentRef,
        network: bool,
    ) -> Result<Option<crate::workspace::Remote>> {
        let project = !self.work(&agent.hamlet)?["folder"].is_null();
        self.houses
            .workspace
            .as_ref()
            .map(|connection| {
                crate::workspace::Remote::new(
                    connection.clone(),
                    agent.clone(),
                    project,
                    network,
                    false,
                )
                .map(|remote| {
                    remote.with_library(
                        self.home.layout().root().join("library"),
                    )
                })
            })
            .transpose()
    }

    pub(crate) async fn initialize_houses(&mut self) -> Result<()> {
        if self.houses.workspace.is_none() {
            return Ok(());
        }
        let live =
            crate::realm::live(self.home.store().connection())?;
        for record in records(self.home.store().connection())? {
            if !live.contains(&record.id) {
                continue;
            }
            for agent in record.agents {
                if let Err(error) = self
                    .houses
                    .initialize(&record.id, agent.number)
                    .await
                {
                    let mut words = Value::String(error.to_string());
                    self.credentials.scrub(&mut words);
                    eprintln!(
                        "hamlet: could not initialize remote house: \
                         {}",
                        words.as_str().unwrap_or_default()
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn limits_of(
        &mut self,
        hamlet: &str,
    ) -> Result<Value> {
        Ok(document(
            self.home.store().connection(),
            "limits",
            hamlet,
        )?
        .unwrap_or_else(|| self.limits.defaults()))
    }

    pub(crate) fn work(&mut self, hamlet: &str) -> Result<Value> {
        Ok(document(self.home.store().connection(), "work", hamlet)?.unwrap_or_else(|| json!({"folder":null, "mode":"manual", "before":[]})))
    }

    pub(crate) async fn read(
        &mut self,
        read: Read,
    ) -> Result<ServerMessage> {
        crate::documents::read(self, read).await
    }
}

pub(crate) fn records(
    db: &Connection,
) -> Result<Vec<record::Hamlet>> {
    let mut statement =
        db.prepare("SELECT document FROM record_hamlet ORDER BY id")?;
    statement
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str(&r?)?))
        .collect()
}

pub(crate) fn document(
    db: &Connection,
    kind: &str,
    key: &str,
) -> Result<Option<Value>> {
    db.query_row(
        "SELECT value FROM chief_document WHERE kind = ?1 AND key = \
         ?2",
        params![kind, key],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(|v| {
        serde_json::from_str(&v).context("reading a chief document")
    })
    .transpose()
}

pub(crate) fn save(
    db: &Connection,
    kind: &str,
    key: &str,
    value: &Value,
) -> Result<()> {
    db.execute(
        "INSERT INTO chief_document(kind, key, value) VALUES (?1, \
         ?2, ?3) ON CONFLICT(kind, key) DO UPDATE SET value = \
         excluded.value",
        params![kind, key, serde_json::to_string(value)?],
    )?;
    Ok(())
}
