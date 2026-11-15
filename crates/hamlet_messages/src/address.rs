//! The actors of the contract, as the host addresses them: a kind and
//! a key.

use anyhow::Context as _;
use hamlet_actor::{Actor, ActorKeySegment, Ctx};
use hamlet_address::{Address, InvalidName};

use crate::realm::ProfileId;
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

/// A profile's actor, `profile/<n>`: its id in decimal, with no sign
/// and no leading zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile(pub ProfileId);

/// The library, `library/local`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Library;

/// The key of the workspace, the realm and the library.
pub const LOCAL: &str = "local";

/// The number a key of `agent/<n>` or `profile/<n>` holds: the key
/// read back only when it is the number in decimal, with no sign and
/// no leading zero.
pub(crate) fn number_at(key: &str) -> Option<u32> {
    let number = key.parse::<u32>().ok()?;
    (number.to_string() == key).then_some(number)
}

impl Profile {
    /// The profile whose actor `ctx` is, from its own address.
    pub fn own<A: Actor>(ctx: &Ctx<A>) -> anyhow::Result<Self> {
        let key = match ctx.key().as_slice() {
            [ActorKeySegment::String(key)]
                if ctx.name() == Self::KIND =>
            {
                number_at(key)
            }
            _ => None,
        };
        key.map(|id| Self(ProfileId(id))).with_context(|| {
            format!("`{}` is not a profile's actor", ctx.actor_id())
        })
    }
}

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

impl Party for Profile {
    const KIND: &'static str = "profile";

    fn address(&self) -> Result<Address, InvalidName> {
        Address::new(Self::KIND, self.0.0.to_string())
    }
}

impl Party for Library {
    const KIND: &'static str = "library";

    fn address(&self) -> Result<Address, InvalidName> {
        Address::new(Self::KIND, LOCAL)
    }
}
