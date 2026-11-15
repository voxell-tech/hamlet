//! The realm's settings: the `settings` table and `EditSettings`.

use anyhow::{Result, bail};
use hamlet_messages::realm::Done;
use hamlet_messages::realm::documents::{
    EditSettings, Endpoint, Newcomers, RemoteWorkspace,
    SettingRefusal,
};
use hamlet_messages::sender::Client;
use hamlet_protocol::Effort;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decision, Unbuilt};
use super::calls::Made;
use super::change::{Affected, Rows, Writer};

/// The realm's settings, as a chief last set them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Settings {
    /// `provider::model`; absent for the catalogue's first.
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub reflect_every: u32,
    pub allow_net: bool,
    pub newcomers: Newcomers,
    pub character: Option<String>,
    pub endpoints: Vec<Endpoint>,
    pub workspace: Option<RemoteWorkspace>,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {}

pub(super) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {}
}

/// The settings as the rows hold them: a chief's, else the realm's
/// first.
pub(super) async fn read(_rows: &Rows<'_>) -> Result<Settings> {
    Ok(Settings {
        model: None,
        effort: None,
        reflect_every: 10,
        allow_net: false,
        newcomers: Newcomers::Chief,
        character: None,
        endpoints: Vec::new(),
        workspace: None,
    })
}

impl Act<Client> for EditSettings {
    type Output = Done<SettingRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U10")
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}
