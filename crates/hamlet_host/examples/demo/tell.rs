//! The program's `tell` through the stub realm, and its answer.

use std::time::Duration;

use hamlet_actor::Action;
use hamlet_host::Address;
use hamlet_messages::bytes::Base64;

/// Asks the realm to call the action `name` of the actor at `to` with
/// `args` as itself, and to answer what it answers. Only the program
/// may send it.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TellAny {
    pub to: Address,
    pub name: String,
    pub args: Base64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Told {
    pub answer: Base64,
    pub took: Duration,
}

impl Action for TellAny {
    type Output = Told;
    const NAME: &'static str = "tell";
}
