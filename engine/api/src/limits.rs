//! The limits one plugin is held to: every value as the engine uses
//! it, and a set of them each named or not, as a manifest asks for
//! them and the person sets them.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A number one limit holds.
trait Number: Copy {
    /// Whether it is a whole number.
    const WHOLE: bool;

    fn value(self) -> LimitValue;

    /// `value` as this number, the nearest it can hold.
    fn of(value: LimitValue) -> Self;
}

macro_rules! whole_number {
    ($($ty:ty),*) => {$(
        impl Number for $ty {
            const WHOLE: bool = true;

            fn value(self) -> LimitValue {
                LimitValue::Whole(self as u64)
            }

            fn of(value: LimitValue) -> Self {
                match value {
                    LimitValue::Whole(n) => {
                        <$ty>::try_from(n).unwrap_or(<$ty>::MAX)
                    }
                    LimitValue::Real(x) => x as $ty,
                }
            }
        }
    )*};
}

whole_number!(u32, u64, usize);

impl Number for f32 {
    const WHOLE: bool = false;

    fn value(self) -> LimitValue {
        LimitValue::Real(self)
    }

    fn of(value: LimitValue) -> Self {
        match value {
            LimitValue::Whole(n) => n as f32,
            LimitValue::Real(x) => x,
        }
    }
}

/// The value of one limit.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum LimitValue {
    Whole(u64),
    Real(f32),
}

impl LimitValue {
    /// Whether it is above zero, and finite.
    pub fn positive(self) -> bool {
        match self {
            Self::Whole(n) => n > 0,
            Self::Real(x) => x.is_finite() && x > 0.0,
        }
    }
}

/// A whole number is written in groups of three digits joined by `_`,
/// as a manifest may write it.
impl fmt::Display for LimitValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Whole(n) => {
                let digits = n.to_string();
                for (at, digit) in digits.chars().enumerate() {
                    if at > 0 && (digits.len() - at) % 3 == 0 {
                        f.write_str("_")?;
                    }
                    write!(f, "{digit}")?;
                }
                Ok(())
            }
            Self::Real(x) => write!(f, "{x}"),
        }
    }
}

macro_rules! limits {
    ($($(#[$doc:meta])* $field:ident: $ty:ty => $variant:ident,)*) => {
        /// Every limit one plugin is held to.
        #[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct PluginLimits {
            $($(#[$doc])* pub $field: $ty,)*
        }

        /// Limits each given or not, by the names of
        /// [`PluginLimits`]'s fields.
        #[derive(
            Clone, PartialEq, Default, Debug, Serialize, Deserialize,
        )]
        #[serde(deny_unknown_fields)]
        pub struct Limits {
            // No field is skipped when it is none, as postcard is
            // read by position; toml leaves a none out itself.
            $(
                #[serde(default)]
                pub $field: Option<$ty>,
            )*
        }

        /// One of the limits.
        #[derive(
            Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize,
            Deserialize,
        )]
        pub enum Limit {
            $($variant,)*
        }

        impl Limit {
            /// Every limit, in the order of [`PluginLimits`].
            pub const ALL: &[Limit] = &[$(Limit::$variant,)*];

            /// Its name in a manifest's `[limits]`.
            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($field),)*
                }
            }

            /// Whether its value is a whole number.
            pub fn whole(self) -> bool {
                match self {
                    $(Self::$variant => <$ty as Number>::WHOLE,)*
                }
            }
        }

        impl PluginLimits {
            pub fn get(&self, limit: Limit) -> LimitValue {
                match limit {
                    $(Limit::$variant => self.$field.value(),)*
                }
            }

            /// These limits with each one `over` gives in its place.
            pub fn with(&self, over: &Limits) -> Self {
                Self {
                    $($field: over.$field.unwrap_or(self.$field),)*
                }
            }
        }

        impl Limits {
            pub fn get(&self, limit: Limit) -> Option<LimitValue> {
                match limit {
                    $(Limit::$variant => {
                        self.$field.map(Number::value)
                    })*
                }
            }

            /// Gives `limit` the `value`, or takes it away for `None`.
            pub fn set(
                &mut self,
                limit: Limit,
                value: Option<LimitValue>,
            ) {
                match limit {
                    $(Limit::$variant => {
                        self.$field = value.map(<$ty as Number>::of);
                    })*
                }
            }
        }
    };
}

limits! {
    /// Bytes of memory, all of the plugin's memories summed.
    memory: u64 => Memory,
    /// Fuel for one call into the plugin.
    fuel_per_call: u64 => FuelPerCall,
    /// Milliseconds one call may take.
    call_time: f32 => CallTime,
    /// Milliseconds of calls in one second.
    time_per_second: f32 => TimePerSecond,
    /// Fuel of calls in one second.
    fuel_per_second: u64 => FuelPerSecond,
    /// Bytes copied into or out of the plugin by one call, checked
    /// before any copy.
    bytes_per_call: u64 => BytesPerCall,
    calls_per_second: u32 => CallsPerSecond,
    events_per_second: u32 => EventsPerSecond,
    /// Requests to the engine's services one call may make; past
    /// them a request is not taken.
    requests_per_call: usize => RequestsPerCall,
    /// Resources of the system interface (WASI) it holds at once,
    /// such as streams and timers; past them one is refused.
    wasi_resources: usize => WasiResources,
    /// Assets made, changed or deleted in one second.
    asset_writes_per_second: u32 => AssetWritesPerSecond,
    /// Calls of other plugins' services in one second.
    service_calls_per_second: u32 => ServiceCallsPerSecond,
    /// Fuel the services it called spent serving it in one second,
    /// charged to it.
    service_fuel_per_second: u64 => ServiceFuelPerSecond,
    /// Milliseconds the services it called took in one second,
    /// charged to it.
    service_time_per_second: f32 => ServiceTimePerSecond,
    /// Events it sends in one second; the rest are refused.
    events_sent_per_second: u32 => EventsSentPerSecond,
    fetches_per_minute: u32 => FetchesPerMinute,
    /// Fetches under way at once.
    fetches_waiting: usize => FetchesWaiting,
    /// Bytes of the answer to one fetch.
    fetch_bytes: u64 => FetchBytes,
    /// Seconds one fetch may take, its lookup with it.
    fetch_time: f32 => FetchTime,
    panels: usize => Panels,
    /// Everything it adds: panels, commands, menu entries,
    /// shortcuts, settings pages and suggested workspaces.
    contributions: usize => Contributions,
    /// Bytes of one asset. A read asset larger than `bytes_per_call`
    /// cannot be handed over.
    asset_bytes: u64 => AssetBytes,
    /// Assets it may own.
    assets: usize => Assets,
    log_lines_per_second: u32 => LogLinesPerSecond,
    /// Bytes of one line of the log; a longer line is cut.
    log_line_bytes: usize => LogLineBytes,
    petitions_per_minute: u32 => PetitionsPerMinute,
    /// Nodes of retained panels made, set or put in order in one
    /// call; past them the call fails.
    node_changes_per_call: usize => NodeChangesPerCall,
    /// The same, in one second.
    node_changes_per_second: u32 => NodeChangesPerSecond,
    /// Nodes of retained panels it holds at once; past them the call
    /// fails.
    nodes: usize => Nodes,
    /// How deep a node of a retained panel may sit under its root;
    /// a call that makes one deeper fails.
    node_depth: usize => NodeDepth,
    /// Entities of its scenes held at once, each copy of instances
    /// counted; past them a make fails.
    scene_entities: usize => SceneEntities,
    /// Vertices of its meshes held at once.
    scene_vertices: usize => SceneVertices,
    scene_materials: usize => SceneMaterials,
    scene_lights: usize => SceneLights,
    scene_cameras: usize => SceneCameras,
    /// Changes to its scenes in one call; past them the call fails.
    scene_changes_per_call: usize => SceneChangesPerCall,
    /// The same, in one second; past them the plugin waits.
    scene_changes_per_second: u32 => SceneChangesPerSecond,
    /// Bytes of GPU memory its meshes, textures and buffers take, as
    /// the engine counts them; past them a make fails.
    gpu_memory: u64 => GpuMemory,
    /// Shaders, buffers, textures, compute and passes it holds at
    /// once.
    gpu_objects: usize => GpuObjects,
    /// Bytes of one shader's source.
    shader_bytes: usize => ShaderBytes,
    /// Workgroups its compute runs in one frame; a dispatch past
    /// them is refused, and runs past them wait for the next
    /// frame.
    workgroups_per_frame: u32 => WorkgroupsPerFrame,
    /// Milliseconds of GPU time its compute and passes take in one
    /// second, where the GPU can time them; past them they wait for
    /// the next second.
    gpu_time_per_second: f32 => GpuTimePerSecond,
}

impl Limit {
    /// `text` as a value of this limit, when it is a positive number
    /// of its kind. `_` between digits is passed over.
    pub fn parse(self, text: &str) -> Option<LimitValue> {
        let text = text.trim().replace('_', "");
        let value = if self.whole() {
            LimitValue::Whole(text.parse::<u64>().ok()?)
        } else {
            LimitValue::Real(text.parse::<f32>().ok()?)
        };
        value.positive().then_some(value)
    }
}

impl PluginLimits {
    /// The first limit that is zero or below, or not finite.
    pub fn check(&self) -> Result<(), Limit> {
        match Limit::ALL
            .iter()
            .find(|limit| !self.get(**limit).positive())
        {
            Some(limit) => Err(*limit),
            None => Ok(()),
        }
    }
}

impl Limits {
    /// The first limit given that is zero or below.
    pub fn check(&self) -> Result<(), Limit> {
        match Limit::ALL.iter().find(|limit| {
            self.get(**limit).is_some_and(|value| !value.positive())
        }) {
            Some(limit) => Err(*limit),
            None => Ok(()),
        }
    }

    pub fn is_empty(&self) -> bool {
        Limit::ALL.iter().all(|limit| self.get(*limit).is_none())
    }
}
