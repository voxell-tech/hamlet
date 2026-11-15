//! The realm's settings: the `settings` table and `EditSettings`.

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::agent::copies::AgentSettings;
use hamlet_messages::realm::documents::{
    EditSettings, Endpoint, Newcomers, RemoteWorkspace, Setting,
    SettingRefusal,
};
use hamlet_messages::realm::{Done, Refusable};
use hamlet_messages::sender::Client;
use hamlet_protocol::Effort;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::calls::Made;
use super::change::{Affected, Rows, Writer};
use super::documents::{self, Copied, Part, Readers};
use super::world;
use crate::cbor;

// Copied from the server's `settings.rs`.
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
    pub workspace: Option<RemoteWorkspace>,
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
            workspace: None,
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

    fn set(
        &mut self,
        setting: Setting,
    ) -> Result<(), SettingRefusal> {
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
                self.workspace = workspace;
            }
        }
        Ok(())
    }
}

// Today's `endpoint_fault`, in the server's `clients/documents.rs`.
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

/// A model server's address, read as `scheme:` then, for http,
/// `//authority/...`.
fn address_fault(address: &str) -> Result<(), SettingRefusal> {
    let not_an_address = || SettingRefusal::NotAnAddress {
        address: address.to_owned(),
    };
    let (scheme, rest) =
        address.split_once(':').ok_or_else(not_an_address)?;
    let is_scheme = scheme
        .starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !is_scheme {
        return Err(not_an_address());
    }
    if !["http", "https"]
        .iter()
        .any(|http| scheme.eq_ignore_ascii_case(http))
    {
        return Err(SettingRefusal::NotHttp {
            address: address.to_owned(),
        });
    }
    let authority = rest
        .trim_start_matches('/')
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    if authority.is_empty() {
        return Err(not_an_address());
    }
    if authority.contains('@') {
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
            "INSERT INTO settings (id, value, version) VALUES (1, \
             ?1, 1) ON CONFLICT (id) DO UPDATE SET value = \
             excluded.value, version = version + 1",
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

impl Act<Client> for EditSettings {
    type Output = Done<SettingRefusal>;
    type Loaded = Settings;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Settings> {
        read(rows).await
    }

    fn decide(
        &self,
        _by: &Client,
        mut settings: Settings,
    ) -> Decision<(), Self::Output> {
        for setting in &self.set {
            if let Err(refusal) = settings.set(setting.clone()) {
                return Decision::Answer(Done::refused(refusal));
            }
        }
        Decision::Change(
            Decided::new(()).change(Change::Keep(settings)),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}
