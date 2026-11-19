//! The realm a client is shown.

use std::sync::Arc;

use anyhow::Result;
use hamlet_messages::address::Realm;
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::world::Seen;
use hamlet_protocol::client::{
    self as protocol, AgentId, Confinement, LimitsBudget, Link,
    LinkState, Newcomers, Period, RealmLimits, SettingsView,
    SettingsViewWorkspace,
};

use super::call::Calls;
use super::host::HAND;

/// What the server knows of itself, shown in every `State`.
pub(crate) struct Own {
    pub(crate) chief: String,
    pub(crate) confinement: Confinement,
    /// The realm's folder, as a client shows it.
    pub(crate) realm: String,
    /// When this server started, in milliseconds since the epoch.
    pub(crate) epoch: i64,
}

/// The realm as the door last read it, with the server's own fields.
pub(crate) struct View {
    pub(crate) own: Arc<Own>,
    #[expect(
        dead_code,
        reason = "the commands and the state read it"
    )]
    pub(crate) world: Seen,
}

/// Reads the realm's world now.
pub(crate) async fn read(
    calls: &Calls,
    own: &Arc<Own>,
) -> Result<View> {
    Ok(View {
        own: own.clone(),
        world: calls.call(&Realm, &ReadWorld).await?,
    })
}

/// The realm as a client is shown it: the server's own fields and
/// the Hand, with no agent, hamlet or setting read from the actors.
pub(crate) fn state(view: &View) -> protocol::State {
    let own = &view.own;
    protocol::State {
        agents: Vec::new(),
        chief: own.chief.clone(),
        closed: Vec::new(),
        confinement: own.confinement.clone(),
        deeds: crate::commands::deeds(),
        epoch: own.epoch,
        grants: Vec::new(),
        hamlets: Vec::new(),
        hand: AgentId(u64::from(HAND.0)),
        library: Vec::new(),
        limits: RealmLimits {
            budget: LimitsBudget {
                cost: None,
                minutes: None,
                period: Period::Day,
            },
            roads_between: false,
        },
        providers: Vec::new(),
        realm: own.realm.clone(),
        revision: 0,
        scholars: Vec::new(),
        settings: SettingsView {
            allow_net: false,
            character: String::new(),
            effort: None,
            endpoints: Vec::new(),
            link: Link {
                models: None,
                state: LinkState::Untested,
                words: None,
            },
            model: String::new(),
            newcomers: Newcomers::default(),
            reflect_every: 0,
            unset: true,
            workspace: SettingsViewWorkspace {
                moving: None,
                running: None,
                url: None,
            },
        },
        waits: Vec::new(),
    }
}
