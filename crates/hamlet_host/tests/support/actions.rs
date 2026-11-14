//! The actions of the stand-in Node host's kind `agent`, which the
//! stand-in serves and the tests call.

use hamlet_actor::Action;
use hamlet_actor_error::{ActorError, ActorErrorKind};
use hamlet_host::{Address, Caller};
use serde::{Deserialize, Serialize};

/// What a call gave its caller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seen {
    Output(serde_json::Value),
    Error {
        group: String,
        code: String,
        message: String,
        metadata: Option<serde_json::Value>,
    },
}

impl Seen {
    /// What `outcome` gave its caller.
    pub fn of(outcome: anyhow::Result<serde_json::Value>) -> Self {
        match outcome {
            Ok(output) => Self::Output(output),
            Err(error) => {
                let error = ActorError::extract(&error);
                Self::Error {
                    group: error.group().to_owned(),
                    code: error.code().to_owned(),
                    message: error.message().to_owned(),
                    metadata: error.metadata(),
                }
            }
        }
    }

    /// What a call's `called` output, or its error, gave its caller.
    pub fn of_call(called: anyhow::Result<Vec<u8>>) -> Self {
        Self::of(called.and_then(|output| {
            Ok(ciborium::from_reader(output.as_slice())?)
        }))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Who;

impl Action for Who {
    type Output = Option<Caller>;
    const NAME: &'static str = "who";
}

/// An agent's input.
#[derive(Serialize, Deserialize)]
pub struct Input;

impl Action for Input {
    type Output = Option<Address>;
    const NAME: &'static str = "input";
}

/// What the call of an agent's start gave.
#[derive(Serialize, Deserialize)]
pub struct Started;

impl Action for Started {
    type Output = Option<Seen>;
    const NAME: &'static str = "started";
}

#[derive(Serialize, Deserialize)]
pub struct Relay {
    pub to: Address,
    pub name: String,
    pub args: Vec<u8>,
    pub deadline_ms: u64,
}

impl Action for Relay {
    type Output = Seen;
    const NAME: &'static str = "relay";
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fail {
    Coded {
        group: String,
        code: String,
        message: String,
        metadata: serde_json::Value,
    },
    Uncoded {
        text: String,
    },
}

impl Fail {
    /// The error the action fails with.
    pub fn into_error(self) -> anyhow::Error {
        let (group, code, message, metadata) = match self {
            Self::Coded {
                group,
                code,
                message,
                metadata,
            } => (group, code, message, metadata),
            Self::Uncoded { text } => return anyhow::anyhow!(text),
        };
        match serde_json::value::to_raw_value(&metadata) {
            Ok(meta) => anyhow::Error::new(ActorError {
                kind: ActorErrorKind::Dynamic {
                    group,
                    code,
                    default_message: message.clone(),
                },
                meta: Some(meta),
                message: Some(message),
                actor: None,
            }),
            Err(error) => error.into(),
        }
    }
}

impl Action for Fail {
    type Output = ();
    const NAME: &'static str = "fail";
}

/// An agent's action that answers after `ms`.
#[derive(Serialize, Deserialize)]
pub struct Slow {
    pub ms: u64,
}

impl Action for Slow {
    type Output = ();
    const NAME: &'static str = "slow";
}

/// An agent's action that ends its Node host.
#[derive(Serialize, Deserialize)]
pub struct Exit;

impl Action for Exit {
    type Output = ();
    const NAME: &'static str = "exit";
}
