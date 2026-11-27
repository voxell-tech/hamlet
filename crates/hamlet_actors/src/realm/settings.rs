//! The realm's settings: the `settings` table and `EditSettings`.

use std::collections::BTreeSet;
use std::sync::{Arc, LazyLock};

use anyhow::Result;
use hamlet_actor::{Action, BindParam, Ctx, Handler};
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
use super::{Realm, marker, secrets, world};
use crate::cbor;

const CHARACTER: &str = "You are a scholar new to this realm. You \
                         know nothing of your own yet: use the \
                         library's skills, learn from the work you \
                         are given, and keep what is worth keeping \
                         in your memory so that it lasts.";

pub(super) const CHARACTER_MOST: usize = 4000;

static OVERRIDES: LazyLock<Overrides> =
    LazyLock::new(Overrides::from_env);

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
            endpoints: self.endpoints.clone(),
        }
    }

    /// The parts of the copies that differ from `before`'s.
    fn moved(&self, before: &Self) -> BTreeSet<Part> {
        [
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

/// A model server's address, checked.
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
    let moved = settings.moved(&before);
    let mut seen = BTreeSet::new();
    if settings != before {
        seen.insert(world::Part::State);
    }
    if !reaches.is_empty() {
        seen.insert(world::Part::Reaches);
    }
    if moved.contains(&Part::Model) {
        seen.insert(world::Part::Agents);
    }
    let copies = moved
        .into_iter()
        .map(|part| Copied {
            part,
            readers: Readers::Living,
        })
        .collect();
    Ok(Affected {
        reaches,
        copies,
        seen,
        ..Affected::default()
    })
}

/// The overrides the process puts over the kept settings, read once
/// from `HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_REFLECT_EVERY` and
/// `HAMLET_NET`. It is never kept.
#[derive(Debug, Default)]
struct Overrides {
    model: Option<String>,
    base_url: Option<String>,
    reflect_every: Option<u32>,
    allow_net: Option<bool>,
}

impl Overrides {
    fn from_env() -> Self {
        let set =
            |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        Self {
            model: set("HAMLET_MODEL"),
            base_url: set("HAMLET_BASE_URL"),
            reflect_every: set("HAMLET_REFLECT_EVERY")
                .and_then(|v| v.parse().ok()),
            allow_net: set("HAMLET_NET")
                .map(|v| v == "1" || v == "true"),
        }
    }

    /// `settings` as this process runs them: the model, and the
    /// endpoint its `endpoint::` names pointed at the base URL.
    fn apply(&self, mut settings: Settings) -> Settings {
        if let Some(model) = &self.model {
            settings.model = Some(model.clone());
        }
        if let Some(every) = self.reflect_every {
            settings.reflect_every = every;
        }
        if let Some(net) = self.allow_net {
            settings.allow_net = net;
        }
        let endpoint = settings
            .model
            .as_deref()
            .and_then(|model| model.split_once("::"))
            .map(|(endpoint, _)| endpoint.to_owned());
        if let (Some(url), Some(endpoint)) =
            (&self.base_url, endpoint)
        {
            match settings
                .endpoints
                .iter_mut()
                .find(|kept| kept.name == endpoint)
            {
                Some(kept) => kept.base_url.clone_from(url),
                None => settings.endpoints.push(Endpoint {
                    name: endpoint,
                    base_url: url.clone(),
                }),
            }
        }
        settings
    }
}

/// The settings this process runs: the kept ones under the
/// environment's overrides.
pub(super) async fn read(rows: &Rows<'_>) -> Result<Settings> {
    Ok(OVERRIDES.apply(kept(rows).await?))
}

/// The settings as the rows hold them: a chief's, else the realm's
/// first.
pub(super) async fn kept(rows: &Rows<'_>) -> Result<Settings> {
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
        let key = marker::Key::of(
            &Client,
            marker::Id::Request(asked.id),
            <Asked<EditSettings> as Action>::NAME,
            &asked.act,
        )?;
        let tx = change::Transaction::begin(&ctx).await?;
        match marker::find(&tx.rows(), &key).await? {
            Some(marker::Found::Same { done, .. }) => {
                return Ok(done);
            }
            Some(marker::Found::Other) => return Err(key.reused()),
            None => {}
        }
        let mut settings = kept(&tx.rows()).await?;
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
        let done = Done::Done;
        let write = change::Write {
            changes,
            entries: Vec::new(),
            marker: Some((&key, &done)),
        };
        secrets::commit(&self, &ctx, tx, write).await?;
        Ok(done)
    }
}
