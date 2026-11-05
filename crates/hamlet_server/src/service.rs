//! The served home and the owners it composes.

use anyhow::{Context, Result, ensure};
use diesel::prelude::*;
use diesel::upsert::excluded;
use hamlet_protocol::client::{
    Address, Edit, Read, RequestId, ServerMessage, State,
};
use serde_json::Value;

use crate::credentials::Credentials;
use crate::diagnostic::{Secrets, tell};
use crate::documents::Work;
use crate::limits::LimitsSchema;
use crate::papers::{Capability, Houses};
use crate::protocol::{Cause, Refusable, Refusal, reply};
use crate::record::{self, FoundRecord};
use crate::schema::{chief_document, record_hamlet};
use crate::scholars::Scholars;
use crate::settings::{Overrides, Settings};
use crate::{Home, OperationId, Outcome, Reply, RuntimeInstance};

/// The served home and its chief-facing requests.
pub struct Service {
    pub home: Home,
    pub(crate) chief: String,
    pub(crate) credentials: Credentials,
    pub(crate) limits: LimitsSchema,
    pub(crate) houses: Houses,
    pub(crate) scholars: Scholars,
    pub(crate) overrides: Overrides,
    pub(crate) models: crate::models::Models,
    pub(crate) chats: crate::chats::Chats,
    pub(crate) gifts: crate::gifts::Uploads,
    pub(crate) moving: crate::papers::moving::Moving,
    pub(crate) house_work: crate::society::work::Work,
    pub(crate) merges: crate::society::merging::lane::Merges,
}

impl Service {
    /// Opens the chief's documents and the first hamlet, retaining
    /// the recorded chief on restart.
    pub fn new(mut home: Home, overrides: Overrides) -> Result<Self> {
        let root = home.layout().root().to_owned();
        let credentials = Credentials::open(&root)?;
        overrides
            .apply(crate::settings::stored(home.connection())?)?;
        let workspace = overrides
            .workspace
            .as_ref()
            .or(credentials.workspace())
            .cloned();
        if let Some(connection) = &workspace {
            crate::settings::check_workspace(connection)
                .map_err(anyhow::Error::msg)?;
            #[cfg(windows)]
            ensure!(
                overrides.isolation
                    != crate::sandbox::Isolation::Insecure,
                "--insecure runs commands on this machine, but they \
                 run in the remote workspace. Start the server \
                 without it."
            );
        }
        let founded = home.store().record("home")?;
        let chief = match &founded {
            Some(record) => record.chiefs.first().cloned().context(
                "The home hamlet's record names no chief.",
            )?,
            None => ["HAMLET_USER", "USER", "USERNAME"]
                .into_iter()
                .find_map(|key| {
                    std::env::var(key)
                        .ok()
                        .filter(|name| !name.is_empty())
                })
                .context(
                    "A new home needs its chief's name: set \
                     HAMLET_USER or USER.",
                )?,
        };
        if founded.is_none() {
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
        let scholars = Scholars::new(&root);
        scholars.plant(home.connection())?;
        let houses =
            Houses::open(&root, workspace, overrides.isolation)?;
        let mut realm = crate::realm::read(home.connection())?;
        for place in realm.hamlets.iter().filter(|place| place.erased)
        {
            if houses.allows(Capability::EraseHamlet).is_ok() {
                houses.erase_hamlet(&place.id)?;
            }
        }
        if let Some(pending) = &realm.pending {
            ensure!(
                houses.allows(Capability::MoveHouse).is_ok(),
                "An unfinished local house move needs the original \
                 local workspace. Remote houses were not changed."
            );
            for binding in crate::chats::records::conversations_of(
                home.connection(),
                &pending.to,
            )? {
                home.store().begin_cancel(binding)?;
            }
            houses.move_house(&pending.from, &pending.to)?;
            realm.pending = None;
            crate::realm::write(home.connection(), &realm)?;
        }
        let live = crate::realm::live(home.connection())?;
        for record in records(home.connection())? {
            if !live.contains(&record.id) {
                continue;
            }
            for agent in record.agents {
                houses.build(&record.id, agent.number)?;
            }
        }
        for directory in ["library", "tools", "librarian"] {
            std::fs::create_dir_all(root.join(directory))?;
        }
        crate::prompt::stock(&root.join("library"), &credentials);
        Ok(Self {
            home,
            chief,
            credentials,
            houses,
            scholars,
            limits: LimitsSchema::new()?,
            models: crate::models::Models::open(
                &root,
                overrides.faux,
            )?,
            overrides,
            chats: crate::chats::Chats::new(),
            gifts: crate::gifts::Uploads::default(),
            moving: crate::papers::moving::Moving::default(),
            house_work: crate::society::work::Work::default(),
            merges: crate::society::merging::lane::Merges::default(),
        })
    }

    /// The home's open database. A write that needs a transaction
    /// takes `home.store()`.
    pub(crate) fn db(&mut self) -> &mut SqliteConnection {
        self.home.connection()
    }

    /// The state visible to a newly authenticated client.
    pub fn state(
        &mut self,
        epoch: i64,
        revision: i64,
    ) -> Result<State> {
        crate::state::state(self, epoch, revision)
    }

    /// Answers the chief's edit of a document, the request `id`.
    pub async fn edit_document(
        &mut self,
        id: &RequestId,
        edit: Edit,
    ) -> Result<ServerMessage> {
        crate::fault::panic_at("edit_document");
        Ok(reply(id, self.edit(edit).await?))
    }

    pub(crate) fn access(
        &mut self,
        hamlet: &str,
        operation: &str,
    ) -> Result<Refusable<record::Hamlet>> {
        if let Some(place) = crate::realm::read(self.db())?
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
            return Ok(Err(Refusal::Cannot {
                operation: operation.into(),
                cause: Cause::NoHamlet(hamlet.into()),
            }));
        };
        if !record.chiefs.is_empty()
            && !record.chiefs.contains(&self.chief)
        {
            return Ok(Err(Refusal::Cannot {
                operation: operation.into(),
                cause: Cause::NotChief,
            }));
        }
        Ok(Ok(record))
    }

    pub(crate) fn agent(
        &mut self,
        address: &Address,
        operation: &str,
    ) -> Result<Refusable<record::Agent>> {
        let record = match self.access(&address.hamlet, operation)? {
            Ok(record) => record,
            Err(error) => return Ok(Err(error)),
        };
        let found = record.agents.iter().find(|a| {
            if address.agent == hamlet_protocol::MAYOR {
                a.number == record.mayor
            } else {
                a.name == address.agent
            }
        });
        Ok(match found {
            Some(agent) => Ok(agent.clone()),
            None => Err(record
                .archive
                .iter()
                .rev()
                .find(|a| {
                    a.back.is_none() && a.agent.name == address.agent
                })
                .map_or_else(
                    || Refusal::NoAgent(address.agent.clone()),
                    |a| Refusal::Invalid(record.farewell(a)),
                )),
        })
    }

    /// The home's settings, with the process's overrides.
    pub(crate) fn settings(&mut self) -> Result<Settings> {
        let stored = crate::settings::stored(self.db())?;
        self.overrides.apply(stored)
    }

    /// The tools of `agent` working at `place`, reaching the network
    /// only with `net`.
    pub(crate) fn tools(
        &self,
        agent: &record::AgentRef,
        place: crate::tools::Place,
        net: bool,
    ) -> Result<crate::tools::ToolSession> {
        self.houses.tools(agent, place, net)
    }

    /// Makes the live agents' houses where they are missing. A
    /// failure is told and does not stop the server.
    pub(crate) async fn initialize_houses(&mut self) -> Result<()> {
        let live = crate::realm::live(self.db())?;
        for record in records(self.db())? {
            if !live.contains(&record.id) {
                continue;
            }
            for agent in record.agents {
                if let Err(error) = self
                    .houses
                    .initialize(&record.id, agent.number)
                    .await
                {
                    tell(
                        Secrets::Of(&self.credentials),
                        format_args!(
                            "could not initialize a house: {error}"
                        ),
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn limits_of(
        &mut self,
        hamlet: &str,
    ) -> Result<crate::limits::Limits> {
        crate::limits::of(self.db(), hamlet)
    }

    /// What the hamlet works on, and how its agents ask.
    pub(crate) fn work(&mut self, hamlet: &str) -> Result<Work> {
        Ok(document(self.db(), "work", hamlet)?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default())
    }

    /// Answers the chief's read of a document.
    pub async fn read(
        &mut self,
        read: Read,
    ) -> Result<ServerMessage> {
        crate::documents::read(self, read).await
    }
}

pub(crate) fn records(
    db: &mut SqliteConnection,
) -> Result<Vec<record::Hamlet>> {
    record_hamlet::table
        .select(record_hamlet::document)
        .order(record_hamlet::id)
        .load::<String>(db)?
        .iter()
        .map(|document| Ok(serde_json::from_str(document)?))
        .collect()
}

pub(crate) fn document(
    db: &mut SqliteConnection,
    kind: &str,
    key: &str,
) -> Result<Option<Value>> {
    chief_document::table
        .find((kind, key))
        .select(chief_document::value)
        .first::<String>(db)
        .optional()?
        .map(|v| {
            serde_json::from_str(&v)
                .context("reading a chief document")
        })
        .transpose()
}

pub(crate) fn save(
    db: &mut SqliteConnection,
    kind: &str,
    key: &str,
    value: &Value,
) -> Result<()> {
    diesel::insert_into(chief_document::table)
        .values((
            chief_document::kind.eq(kind),
            chief_document::key.eq(key),
            chief_document::value.eq(serde_json::to_string(value)?),
        ))
        .on_conflict((chief_document::kind, chief_document::key))
        .do_update()
        .set(
            chief_document::value.eq(excluded(chief_document::value)),
        )
        .execute(db)?;
    Ok(())
}
