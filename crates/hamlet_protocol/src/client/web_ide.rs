//! The web IDE a Web panel shows on a hamlet's folder.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::RequestId;

/// Asks for the address of a web IDE on `hamlet`'s folder, started
/// if it is not running. It is answered with a reply of the kind
/// `address` whose text is the address, or an error reply that says
/// why there is none.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct WebIde {
    pub hamlet: String,
    pub id: RequestId,
}
