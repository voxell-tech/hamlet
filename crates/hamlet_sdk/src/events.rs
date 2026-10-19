//! The events the host publishes. A plugin lists one under
//! `imports.events` as `host/<topic>` and receives it while it holds
//! the capability the event names. A payload is text, as UTF-8.

use crate::capability::Capability;
/// The sender of the events of this catalogue.
pub use crate::plugin::HOST;

/// An event of the catalogue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostEvent {
    pub topic: &'static str,
    /// What a plugin must hold to receive it.
    pub capability: Capability,
    /// What it is, and what its payload holds, in a few words.
    pub words: &'static str,
}

/// The selection of an agent in the world changed. The payload is
/// the selected agent's name, empty when none is selected.
pub const SELECTION_CHANGED: HostEvent = HostEvent {
    topic: "selection-changed",
    capability: Capability::ReadAgents,
    words: "The selected agent's name, empty when none is",
};

/// An agent came, left, was renamed or moved. The payload is empty:
/// the list is read with `agents`.
pub const AGENTS_CHANGED: HostEvent = HostEvent {
    topic: "agents-changed",
    capability: Capability::ReadAgents,
    words: "An agent came, left, was renamed or moved",
};

/// An agent's turn began. The payload is lines of `key=value`: the
/// `agent`, its `hamlet`, the `turn` and no `tools` yet. It tells of
/// the turn and cannot change or stop it.
pub const BEFORE_TURN: HostEvent = HostEvent {
    topic: "before-turn",
    capability: Capability::AgentHook,
    words: "An agent's turn began: its name, hamlet and the turn's \
            number",
};

/// An agent's turn ended. The payload is as `before-turn` with the
/// names of the tools the turn used, in order, as `tools`. It never
/// holds what was said or what a tool was given or returned.
pub const AFTER_TURN: HostEvent = HostEvent {
    topic: "after-turn",
    capability: Capability::AgentHook,
    words: "An agent's turn ended: its name, hamlet, the turn's \
            number and the names of the tools it used",
};

/// Every event of the catalogue.
pub const ALL: [HostEvent; 4] =
    [SELECTION_CHANGED, AGENTS_CHANGED, BEFORE_TURN, AFTER_TURN];
