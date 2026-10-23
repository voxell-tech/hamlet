//! What a plugin may ask the person for. Powers that are never
//! granted, such as keys or running processes, have no name here,
//! so no manifest can ask for them: a key is given on the engine's
//! own card.

use serde::{Deserialize, Serialize};

/// A kind of access a plugin asks for, by its stable name.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    Serialize,
    Deserialize,
)]
pub enum Capability {
    /// Fetching from one host, the ask's target.
    #[serde(rename = "net.fetch")]
    NetFetch,
    /// Reading assets of one type, the ask's target.
    #[serde(rename = "assets.read")]
    AssetRead,
    /// Calling one service of another plugin, the ask's target
    /// (`plugin.id/service`).
    #[serde(rename = "services.call")]
    ServiceCall,
    /// Reading the agents' settings, status and sessions, on
    /// [`LOCAL`](crate::agents::LOCAL).
    #[serde(rename = "agents.read")]
    AgentsRead,
    /// Acting as the chief: sending to agents, stopping them and
    /// taking permissions back.
    #[serde(rename = "agents.chief")]
    AgentsChief,
    /// Changing the agents' settings, models, charter and limits.
    #[serde(rename = "agents.settings")]
    AgentsSettings,
    /// Opening an `https` address, the target
    /// [`WEB`](crate::desktop::WEB), or a file under one folder, the
    /// target.
    #[serde(rename = "desktop.open")]
    DesktopOpen,
    /// Changing the desktop's settings, on
    /// [`LOCAL`](crate::agents::LOCAL).
    #[serde(rename = "desktop.settings")]
    DesktopSettings,
    /// Being handed the path of each file the person drops on one of
    /// the plugin's views, on [`FILES`](crate::desktop::FILES).
    #[serde(rename = "desktop.drop")]
    DesktopDrop,
    /// Building scenes the engine draws, on [`GPU`].
    #[serde(rename = "render.scene")]
    RenderScene,
    /// Drawing with WGSL of the plugin's own: materials and passes
    /// over a camera's image, on [`GPU`].
    #[serde(rename = "render.shader")]
    RenderShader,
    /// Running WGSL compute of the plugin's own over its buffers, on
    /// [`GPU`].
    #[serde(rename = "render.compute")]
    RenderCompute,
    /// Playing sounds through the engine's audio output, on
    /// [`OUTPUT`](crate::audio::OUTPUT).
    #[serde(rename = "audio.play")]
    AudioPlay,
}

/// The target of the render capabilities: the GPU the engine draws
/// with.
pub const GPU: &str = "gpu";

impl Capability {
    /// Its name, as a manifest writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::NetFetch => "net.fetch",
            Self::AssetRead => "assets.read",
            Self::ServiceCall => "services.call",
            Self::AgentsRead => "agents.read",
            Self::AgentsChief => "agents.chief",
            Self::AgentsSettings => "agents.settings",
            Self::DesktopOpen => "desktop.open",
            Self::DesktopSettings => "desktop.settings",
            Self::DesktopDrop => "desktop.drop",
            Self::RenderScene => "render.scene",
            Self::RenderShader => "render.shader",
            Self::RenderCompute => "render.compute",
            Self::AudioPlay => "audio.play",
        }
    }
}

/// One thing a plugin asks for, and why, in its own words.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Ask {
    pub capability: Capability,
    /// What the access is to: a host, a type name, a service.
    pub target: String,
    pub reason: String,
}
