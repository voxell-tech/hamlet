//! Services: functions a plugin exports for others to call through
//! the engine. The argument and the result are bytes of types named
//! by both sides, and a call goes through only when the names agree.

use serde::{Deserialize, Serialize};

use crate::asset::TypeName;
use crate::id::ServiceId;

/// A service the plugin serves.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceExport {
    /// Its name within the plugin.
    pub name: String,
    pub argument: TypeName,
    pub result: TypeName,
}

/// A service of another plugin that the plugin calls.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceImport {
    pub service: ServiceId,
    pub argument: TypeName,
    pub result: TypeName,
}

/// The services a plugin serves and those it calls, as its
/// manifest's `[services]` names them.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Services {
    #[serde(default)]
    pub exports: Vec<ServiceExport>,
    #[serde(default)]
    pub imports: Vec<ServiceImport>,
}
