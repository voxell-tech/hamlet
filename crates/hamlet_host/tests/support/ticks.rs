//! The event the stand-in's agent broadcasts, and the actions that
//! drive it and end what follows it.

use hamlet_actor::{Action, Event};
use hamlet_host::Caller;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::actions::Fail;

/// The event no subscriber may follow.
pub const SECRET: &str = "secret";

/// The event the agent takes 300 ms to let a subscriber follow.
pub const SLOW: &str = "slow";

/// The agent's refusal of a subscriber to [`SECRET`].
pub fn forbidden() -> Fail {
    Fail::Coded {
        group: "auth".to_owned(),
        code: "forbidden".to_owned(),
        message: "No one follows the agent's secret.".to_owned(),
        metadata: json!({ "event": SECRET }),
    }
}

/// One tick of the agent: its count after the tick, and padding that
/// sizes the event.
#[derive(Serialize, Deserialize)]
pub struct Ticked(pub u64, pub String);

impl Event for Ticked {
    const NAME: &'static str = "ticked";
}

/// Adds one to the agent's stored count, broadcasts [`Ticked`] with
/// the count and `pad` bytes once that commits, and answers the
/// count.
#[derive(Serialize, Deserialize)]
pub struct Tick {
    pub pad: usize,
}

impl Action for Tick {
    type Output = u64;
    const NAME: &'static str = "tick";
}

/// The agent's stored count.
#[derive(Serialize, Deserialize)]
pub struct Count;

impl Action for Count {
    type Output = u64;
    const NAME: &'static str = "count";
}

#[derive(Serialize, Deserialize)]
pub struct Sleep;

impl Action for Sleep {
    type Output = ();
    const NAME: &'static str = "sleep";
}

#[derive(Serialize, Deserialize)]
pub struct Destroy;

impl Action for Destroy {
    type Output = ();
    const NAME: &'static str = "destroy";
}

/// The agent stops with an error, as its code reports a crash.
#[derive(Serialize, Deserialize)]
pub struct Crash;

impl Action for Crash {
    type Output = ();
    const NAME: &'static str = "crash";
}

/// The agent closes each of its connections but the call's own.
#[derive(Serialize, Deserialize)]
pub struct Close;

impl Action for Close {
    type Output = ();
    const NAME: &'static str = "close";
}

/// The agent's next sleep hook waits for ever.
#[derive(Serialize, Deserialize)]
pub struct Hang;

impl Action for Hang {
    type Output = ();
    const NAME: &'static str = "hang";
}

/// The stamp of each of the agent's connections but the call's own.
#[derive(Serialize, Deserialize)]
pub struct Watchers;

impl Action for Watchers {
    type Output = Vec<Option<Caller>>;
    const NAME: &'static str = "watchers";
}
