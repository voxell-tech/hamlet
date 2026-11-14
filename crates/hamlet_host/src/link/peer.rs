//! Who each host of the link speaks for. The answering side vets a
//! request with [`Side::vet`]; the asking side's `Port::may_ask` uses
//! the same role half, [`Role::has_a_program`].

use std::collections::BTreeSet;

use super::Broken;
use crate::Caller;

/// Which host of the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// The host the program opened. It alone has a program and
    /// clients.
    Rust,
    /// Its child.
    Node,
}

impl Role {
    /// Whether a host of this role makes operations with no caller or
    /// with a client's: only the Rust host has a program and clients.
    pub(crate) fn has_a_program(self) -> bool {
        self == Role::Rust
    }
}

/// The other host of the link: which of the two it is, and the kinds
/// it runs (`ChildOptions.kinds` in the Rust host, which the hello
/// must state; the welcome's in the Node host). Its port holds the
/// one copy.
#[derive(Clone, Debug)]
pub(crate) struct Side {
    pub(crate) role: Role,
    pub(crate) kinds: BTreeSet<String>,
}

impl Side {
    pub(crate) fn new(
        role: Role,
        kinds: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            role,
            kinds: kinds.into_iter().collect(),
        }
    }

    /// Whether this host makes operations for `caller`: an actor of
    /// one of its kinds, and for the Rust host also its program
    /// (none) and its program's clients.
    pub(crate) fn speaks_for(&self, caller: Option<&Caller>) -> bool {
        match caller {
            Some(Caller::Actor(from)) => {
                self.kinds.contains(from.kind())
            }
            Some(Caller::Client) | None => self.role.has_a_program(),
        }
    }

    /// The stamp `from` of a request this host wrote, checked. The
    /// only way to a [`Vetted`].
    pub(crate) fn vet(
        &self,
        from: Option<Caller>,
    ) -> Result<Vetted, Broken> {
        if self.speaks_for(from.as_ref()) {
            Ok(Vetted(from))
        } else {
            Err(Broken::Stamp(from))
        }
    }
}

/// A caller the other host asked for and speaks for. Its field is
/// private to this file, so one exists only through [`Side::vet`],
/// and `Stamp::from_peer` takes nothing else.
pub(crate) struct Vetted(Option<Caller>);

impl Vetted {
    pub(crate) fn into_caller(self) -> Option<Caller> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Address;

    fn actor(kind: &str) -> Caller {
        Caller::Actor(Address::new(kind, "x").unwrap())
    }

    /// The table of the sketch's point 4: each host speaks for the
    /// actors of its own kinds, and only the Rust host for none and a
    /// client.
    #[test]
    fn a_side_speaks_for_its_own_actors_and_only_the_rust_host_for_none_and_a_client()
     {
        let rust = Side::new(
            Role::Rust,
            ["realm".to_owned(), "workspace".to_owned()],
        );
        let node = Side::new(Role::Node, ["agent".to_owned()]);
        let stamps = [
            Some(actor("realm")),
            Some(actor("workspace")),
            Some(actor("agent")),
            Some(actor("profile")),
            Some(Caller::Client),
            None,
        ];
        let spoken = |side: &Side| {
            stamps
                .iter()
                .map(|stamp| side.speaks_for(stamp.as_ref()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            spoken(&rust),
            [true, true, false, false, true, true]
        );
        assert_eq!(
            spoken(&node),
            [false, false, true, false, false, false]
        );
        assert!(matches!(
            node.vet(Some(actor("realm"))),
            Err(Broken::Stamp(Some(Caller::Actor(_))))
        ));
        assert_eq!(
            node.vet(Some(actor("agent"))).unwrap().into_caller(),
            Some(actor("agent"))
        );
    }
}
