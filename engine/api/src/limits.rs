//! The limits a person may hold one plugin to, each off until they
//! switch it on: as the person sets them, and as the engine uses
//! them.

use serde::{Deserialize, Serialize};

/// A limit the person may switch on for a plugin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitKind {
    Memory,
    CallTime,
    CpuTime,
    GpuMemory,
    GpuTime,
    SceneEntities,
    AudioMemory,
}

/// What the value of a limit counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitUnit {
    Bytes,
    Milliseconds,
    Count,
}

impl LimitKind {
    /// Every limit, in the order a plugin's page shows them.
    pub const ALL: [Self; 7] = [
        Self::Memory,
        Self::CallTime,
        Self::CpuTime,
        Self::GpuMemory,
        Self::GpuTime,
        Self::SceneEntities,
        Self::AudioMemory,
    ];

    /// Its name as the person reads it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Memory => "Memory",
            Self::CallTime => "Time of one call",
            Self::CpuTime => "CPU time a second",
            Self::GpuMemory => "GPU memory",
            Self::GpuTime => "GPU time a second",
            Self::SceneEntities => "Scene entities",
            Self::AudioMemory => "Audio memory",
        }
    }

    pub fn unit(self) -> LimitUnit {
        match self {
            Self::Memory | Self::GpuMemory | Self::AudioMemory => {
                LimitUnit::Bytes
            }
            Self::CallTime | Self::CpuTime | Self::GpuTime => {
                LimitUnit::Milliseconds
            }
            Self::SceneEntities => LimitUnit::Count,
        }
    }
}

/// One limit as the person set it: whether it is on, and its value,
/// kept while it is off.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    pub on: bool,
    pub value: u64,
}

/// The limits the person set for one plugin, by the names of
/// [`PluginLimits`]'s fields; one never set is `None`.
#[derive(
    Clone, PartialEq, Eq, Default, Debug, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Limit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_time: Option<Limit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_time_per_second: Option<Limit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_memory: Option<Limit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_time_per_second: Option<Limit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_entities: Option<Limit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_memory: Option<Limit>,
}

impl Limits {
    pub fn get(&self, kind: LimitKind) -> Option<Limit> {
        match kind {
            LimitKind::Memory => self.memory,
            LimitKind::CallTime => self.call_time,
            LimitKind::CpuTime => self.cpu_time_per_second,
            LimitKind::GpuMemory => self.gpu_memory,
            LimitKind::GpuTime => self.gpu_time_per_second,
            LimitKind::SceneEntities => self.scene_entities,
            LimitKind::AudioMemory => self.audio_memory,
        }
    }

    pub fn set(&mut self, kind: LimitKind, limit: Option<Limit>) {
        let slot = match kind {
            LimitKind::Memory => &mut self.memory,
            LimitKind::CallTime => &mut self.call_time,
            LimitKind::CpuTime => &mut self.cpu_time_per_second,
            LimitKind::GpuMemory => &mut self.gpu_memory,
            LimitKind::GpuTime => &mut self.gpu_time_per_second,
            LimitKind::SceneEntities => &mut self.scene_entities,
            LimitKind::AudioMemory => &mut self.audio_memory,
        };
        *slot = limit;
    }

    /// The first limit whose value is zero.
    pub fn check(&self) -> Result<(), LimitKind> {
        match LimitKind::ALL.into_iter().find(|&kind| {
            self.get(kind).is_some_and(|limit| limit.value == 0)
        }) {
            Some(kind) => Err(kind),
            None => Ok(()),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The value of each limit that is on.
    pub fn in_force(&self) -> PluginLimits {
        let value = |limit: Option<Limit>| {
            limit.filter(|limit| limit.on).map(|limit| limit.value)
        };
        PluginLimits {
            memory: value(self.memory),
            call_time: value(self.call_time),
            cpu_time_per_second: value(self.cpu_time_per_second),
            gpu_memory: value(self.gpu_memory),
            gpu_time_per_second: value(self.gpu_time_per_second),
            scene_entities: value(self.scene_entities),
            audio_memory: value(self.audio_memory),
        }
    }
}

/// The limits one plugin is held to: the value of each the person
/// switched on, `None` for each that is off.
#[derive(
    Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct PluginLimits {
    /// Bytes of memory, all of the plugin's memories summed.
    #[serde(default)]
    pub memory: Option<u64>,
    /// Milliseconds one call may take.
    #[serde(default)]
    pub call_time: Option<u64>,
    /// Milliseconds its calls may take in one second; past them it
    /// waits for the next.
    #[serde(default)]
    pub cpu_time_per_second: Option<u64>,
    /// Bytes of its meshes, textures and buffers, as the engine
    /// counts them.
    #[serde(default)]
    pub gpu_memory: Option<u64>,
    /// Milliseconds of GPU time its compute and passes take in one
    /// second, where the GPU can time them.
    #[serde(default)]
    pub gpu_time_per_second: Option<u64>,
    /// Entities of its scenes held at once, each copy of instances
    /// counted.
    #[serde(default)]
    pub scene_entities: Option<u64>,
    /// Bytes of the samples of the clips it holds.
    #[serde(default)]
    pub audio_memory: Option<u64>,
}

impl PluginLimits {
    pub fn get(&self, kind: LimitKind) -> Option<u64> {
        match kind {
            LimitKind::Memory => self.memory,
            LimitKind::CallTime => self.call_time,
            LimitKind::CpuTime => self.cpu_time_per_second,
            LimitKind::GpuMemory => self.gpu_memory,
            LimitKind::GpuTime => self.gpu_time_per_second,
            LimitKind::SceneEntities => self.scene_entities,
            LimitKind::AudioMemory => self.audio_memory,
        }
    }
}
