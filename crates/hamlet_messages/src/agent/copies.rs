//! What the realm keeps that reaches an agent's prompt and its
//! model calls, given to the agent as copies with a version.

use std::fmt;

use hamlet_actor::Action;
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize};

use crate::Message;
use crate::address::{Agent, Realm};

/// The parts that changed, each with its version. The agent keeps a
/// part when its version is newer than the one it holds.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
pub struct Copies {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charter: Option<Versioned<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Versioned<ModelChoice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Versioned<AgentSettings>>,
}

impl Action for Copies {
    type Output = ();
    const NAME: &'static str = "copies";
}

impl Message for Copies {
    type To = Agent;
    type From = Realm;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Versioned<T> {
    pub version: CopyVersion,
    pub value: T,
}

/// The version of a part of the copies.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub struct CopyVersion(pub u32);

/// The model the realm resolved for the agent (H29, H29a).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelChoice {
    /// `provider::model`, as the catalogue names it.
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<hamlet_protocol::Effort>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fast: bool,
    /// The catalogue card's price; absent when the card has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
}

/// Dollars per million tokens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Price {
    #[serde(deserialize_with = "dollars")]
    pub input: f64,
    #[serde(deserialize_with = "dollars")]
    pub output: f64,
    #[serde(deserialize_with = "dollars")]
    pub cache_read: f64,
    #[serde(deserialize_with = "dollars")]
    pub cache_write: f64,
}

/// A price read as a float or as an integer: the Node host's encoder
/// writes a whole number such as 3 as an integer.
fn dollars<'de, D: Deserializer<'de>>(
    from: D,
) -> Result<f64, D::Error> {
    struct Dollars;
    impl Visitor<'_> for Dollars {
        type Value = f64;

        fn expecting(&self, out: &mut fmt::Formatter) -> fmt::Result {
            out.write_str("a number")
        }

        fn visit_f64<E>(self, price: f64) -> Result<f64, E> {
            Ok(price)
        }

        fn visit_u64<E>(self, price: u64) -> Result<f64, E> {
            Ok(price as f64)
        }

        fn visit_i64<E>(self, price: i64) -> Result<f64, E> {
            Ok(price as f64)
        }
    }
    from.deserialize_any(Dollars)
}

/// The realm's settings an agent's turns read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSettings {
    pub reflect_every: u32,
    pub allow_net: bool,
}
