//! The realm's settings: the `settings` table and `EditSettings`.

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{BindParam, Ctx, Handler};
use hamlet_messages::agent::copies::AgentSettings;
use hamlet_messages::realm::documents::{
    EditSettings, Endpoint, Newcomers, RemoteWorkspace, Setting,
    SettingRefusal,
};
use hamlet_messages::realm::{Asked, Done, Refusable};
use hamlet_messages::sender::{Client, sender};
use hamlet_protocol::Effort;
use serde::{Deserialize, Serialize};

use super::change::{self, Affected, Rows, Writer};
use super::documents::{self, Copied, Part, Readers};
use super::{Realm, secrets, world};
use crate::cbor;

const CHARACTER: &str = "You are a scholar new to this realm. You \
                         know nothing of your own yet: use the \
                         library's skills, learn from the work you \
                         are given, and keep what is worth keeping \
                         in your memory so that it lasts.";

const CHARACTER_MOST: usize = 4000;

/// The realm's settings, as a chief last set them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Settings {
    /// `provider::model`; absent for the catalogue's first.
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub reflect_every: u32,
    pub allow_net: bool,
    pub newcomers: Newcomers,
    /// The character a blank agent starts from.
    pub character: String,
    pub endpoints: Vec<Endpoint>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: None,
            effort: None,
            reflect_every: 10,
            allow_net: false,
            newcomers: Newcomers::Chief,
            character: CHARACTER.to_owned(),
            endpoints: Vec::new(),
        }
    }
}

impl Settings {
    /// The part of the copies an agent's turns read.
    pub fn for_agents(&self) -> AgentSettings {
        AgentSettings {
            reflect_every: self.reflect_every,
            allow_net: self.allow_net,
        }
    }

    /// The parts of the copies that differ from `before`'s.
    fn moved(&self, before: &Self) -> BTreeSet<Part> {
        [
            (Part::Character, self.character != before.character),
            (
                Part::Model,
                (&self.model, self.effort)
                    != (&before.model, before.effort),
            ),
            (
                Part::Settings,
                self.for_agents() != before.for_agents(),
            ),
        ]
        .into_iter()
        .filter_map(|(part, moved)| moved.then_some(part))
        .collect()
    }

    /// Sets `setting` but the workspace, which it answers checked.
    fn set(
        &mut self,
        setting: Setting,
    ) -> Result<Option<Option<RemoteWorkspace>>, SettingRefusal> {
        match setting {
            Setting::Model { model } => self.model = Some(model),
            Setting::Effort { effort } => self.effort = effort,
            Setting::ReflectEvery { turns } => {
                self.reflect_every = turns;
            }
            Setting::AllowNet { allow } => self.allow_net = allow,
            Setting::Newcomers { newcomers } => {
                self.newcomers = newcomers;
            }
            Setting::Character { text } => {
                if text.trim().is_empty() {
                    return Err(SettingRefusal::BlankCharacter);
                }
                if text.chars().count() > CHARACTER_MOST {
                    return Err(SettingRefusal::LongCharacter);
                }
                self.character = text;
            }
            Setting::Endpoints { endpoints } => {
                let endpoints = endpoints
                    .into_iter()
                    .map(|endpoint| Endpoint {
                        name: endpoint.name.trim().to_owned(),
                        base_url: endpoint.base_url.trim().to_owned(),
                    })
                    .collect::<Vec<_>>();
                endpoints_fault(&endpoints)?;
                self.endpoints = endpoints;
            }
            Setting::Workspace { workspace } => {
                let workspace =
                    workspace.map(|workspace| RemoteWorkspace {
                        url: workspace.url.trim().to_owned(),
                        ..workspace
                    });
                if let Some(workspace) = &workspace {
                    address_fault(&workspace.url)?;
                }
                return Ok(Some(workspace));
            }
        }
        Ok(None)
    }
}

fn endpoints_fault(
    endpoints: &[Endpoint],
) -> Result<(), SettingRefusal> {
    for (at, endpoint) in endpoints.iter().enumerate() {
        if endpoint.name.is_empty() || endpoint.name.contains("::") {
            return Err(SettingRefusal::EndpointName);
        }
        if endpoints[..at].iter().any(|e| e.name == endpoint.name) {
            return Err(SettingRefusal::EndpointTwice {
                name: endpoint.name.clone(),
            });
        }
        address_fault(&endpoint.base_url)?;
    }
    Ok(())
}

/// A model server's address, checked as today's
/// (`hamlet_server/src/clients/documents.rs:748-765`).
fn address_fault(address: &str) -> Result<(), SettingRefusal> {
    let Ok(url) = url::Url::parse(address) else {
        return Err(SettingRefusal::NotAnAddress {
            address: address.to_owned(),
        });
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Err(SettingRefusal::NotHttp {
            address: address.to_owned(),
        });
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(SettingRefusal::AddressHoldsKey);
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Keep(Settings),
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let Change::Keep(settings) = change;
    let before = read(&writer.rows()).await?;
    writer
        .execute(
            "INSERT INTO settings (id, value) VALUES (1, ?1) ON \
             CONFLICT (id) DO UPDATE SET value = excluded.value",
            vec![BindParam::Blob(cbor(&settings)?)],
        )
        .await?;
    let reaches = if settings.allow_net == before.allow_net {
        BTreeSet::new()
    } else {
        documents::living(&writer.rows())
            .await?
            .into_iter()
            .collect()
    };
    let copies = settings
        .moved(&before)
        .into_iter()
        .map(|part| Copied {
            part,
            readers: Readers::Living,
        })
        .collect();
    let seen = if reaches.is_empty() {
        BTreeSet::new()
    } else {
        BTreeSet::from([world::Part::Reaches])
    };
    Ok(Affected {
        reaches,
        copies,
        seen,
        ..Affected::default()
    })
}

/// The settings as the rows hold them: a chief's, else the realm's
/// first.
pub(super) async fn read(rows: &Rows<'_>) -> Result<Settings> {
    let kept = rows
        .value::<Vec<u8>>(
            "SELECT value FROM settings WHERE id = 1",
            Vec::new(),
        )
        .await?;
    Ok(match kept {
        Some(bytes) => ciborium::from_reader(bytes.as_slice())?,
        None => Settings::default(),
    })
}

impl Handler<Asked<EditSettings>> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Asked<EditSettings>,
    ) -> Result<Done<SettingRefusal>> {
        let Client = sender(&ctx, &asked)?;
        let tx = change::Transaction::begin(&ctx).await?;
        let mut settings = read(&tx.rows()).await?;
        let before = settings.endpoints.clone();
        let mut chosen = None;
        for setting in asked.act.set {
            match settings.set(setting) {
                Ok(workspace) => chosen = workspace.or(chosen),
                Err(refusal) => return Ok(Done::refused(refusal)),
            }
        }
        let removed = before
            .into_iter()
            .filter(|old| {
                !settings.endpoints.iter().any(|e| e.name == old.name)
            })
            .map(|old| old.name)
            .collect::<Vec<_>>();
        let mut changes = vec![Change::Keep(settings).into()];
        if chosen.is_some() || !removed.is_empty() {
            changes.extend(secrets::raises(&tx.rows()).await?);
        }
        if let Some(workspace) = chosen {
            self.secrets.choose_workspace(workspace)?;
        }
        if !removed.is_empty() {
            self.secrets.forget(&removed)?;
        }
        secrets::commit(&self, &ctx, tx, changes).await?;
        Ok(Done::Done)
    }
}
