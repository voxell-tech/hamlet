//! What the realm's own words mean, for their tooltips.

/// Everything one chief keeps.
pub(crate) const REALM: &str =
    "Everything you keep: the Hand, the city and every hamlet.";

pub(crate) const HAND: &str =
    "Your own agent. Ask it anything, or to set things up for you.";

pub(crate) const HAMLET: &str =
    "A team of agents that works on one folder of yours.";

pub(crate) const LEAD: &str =
    "The hamlet's top agent, the one you talk to first.";

pub(crate) const AGENT: &str =
    "An agent working in a hamlet, under its lead or another agent.";

pub(crate) const WORKER: &str =
    "A helper started for one task. It ends with its answer.";

pub(crate) const PROFILE: &str = "A saved agent with its own memory \
                                  and skills, which hamlets can add \
                                  as agents.";

pub(crate) const CITY: &str =
    "Where the profiles live while they are not at work.";

pub(crate) const HARBOR: &str =
    "New profiles wait here until you let them into the city.";

pub(crate) const LIBRARY: &str = "Skills every agent can use.";

pub(crate) const HEARTH: &str = "Your own house in the world.";

pub(crate) const NEEDS: &str =
    "Requests and approvals waiting for your response.";

pub(crate) const LEFT: &str =
    "Agents and profiles that left. They can be brought back.";

pub(crate) const LIMITS: &str = "What the hamlet may spend, and \
                                 what its parents must ask you \
                                 first.";

pub(crate) const AGENTS: &str = "Everyone working in the hamlet.";

/// An agent or lead that is no profile of the city.
pub(crate) const BLANK: &str = "A fresh agent made for this job \
                                alone. What it learns stays with \
                                the job.";

/// A profile as an agent or a lead.
pub(crate) const EXISTING_PROFILE: &str =
    "Brings its memory and skills to the job, and can take home \
     what it learns.";

/// A hamlet some of whose agents wait for the chief.
pub(crate) const MANUAL: &str = "Manual";

/// A hamlet whose agents edit and run commands without waiting.
pub(crate) const AUTO: &str = "Auto";

/// What the manual clockwork says.
pub(crate) const MANUAL_WORDS: &str =
    "An agent of this hamlet waits for you before it edits a folder \
     or runs a command. Set each agent's permissions from its chat.";

/// What the auto clockwork says.
pub(crate) const AUTO_WORDS: &str =
    "Its agents edit the folders and run commands without waiting. \
     The sandbox stays on for all but an agent with full access on \
     this machine; a remote workspace keeps its own.";

/// The levels of an agent's permissions, in T3 Code's words.
pub(crate) const SUPERVISED: &str = "Supervised";
pub(crate) const SUPERVISED_WORDS: &str =
    "Ask before commands and file changes.";
pub(crate) const ACCEPT_EDITS: &str = "Auto-accept edits";
pub(crate) const ACCEPT_EDITS_WORDS: &str =
    "Auto-approve edits, ask before other actions.";
pub(crate) const AUTO_ACCESS: &str = "Auto";
pub(crate) const AUTO_ACCESS_WORDS: &str =
    "Commands and edits run in the sandbox. The network still asks.";
pub(crate) const FULL_ACCESS: &str = "Full access";
pub(crate) const FULL_ACCESS_WORDS: &str =
    "Allow commands and edits without prompts, with no sandbox. A \
     remote workspace keeps its sandbox and only opens the network.";

pub(crate) const PERMISSIONS: &str =
    "What this agent may do in the hamlet's folders before it asks \
     you. A new folder always asks.";

pub(crate) const HISTORY: &str =
    "What happened in the hamlet: who came and went, and why.";
