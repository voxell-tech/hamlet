//! The actors of the contract, as the host addresses them: a kind and
//! a key.

use hamlet_address::{Address, InvalidName};

use crate::workspace::AgentNumber;

/// An actor a [`crate::Message`] is sent to.
pub trait Party {
    /// Its actor kind, as the host registers it.
    const KIND: &'static str;

    fn address(&self) -> Result<Address, InvalidName>;
}

/// The workspace of this machine, `workspace/local`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Workspace;

/// The realm, `realm/local`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Realm;

/// An agent, `agent/<n>`: its number in decimal, with no sign and no
/// leading zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Agent(pub AgentNumber);

/// The key of the workspace, and of the realm.
pub const LOCAL: &str = "local";

impl Party for Workspace {
    const KIND: &'static str = "workspace";

    fn address(&self) -> Result<Address, InvalidName> {
        Address::new(Self::KIND, LOCAL)
    }
}

impl Party for Realm {
    const KIND: &'static str = "realm";

    fn address(&self) -> Result<Address, InvalidName> {
        Address::new(Self::KIND, LOCAL)
    }
}

impl Party for Agent {
    const KIND: &'static str = "agent";

    fn address(&self) -> Result<Address, InvalidName> {
        Address::new(Self::KIND, self.0.0.to_string())
    }
}
