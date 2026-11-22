//! What the realm's own words mean, for their tooltips.

/// Everything one chief keeps.
pub const REALM: &str =
    "Everything you keep: the Hand, the city and every hamlet.";

pub const HAND: &str =
    "Your own agent. Ask it anything, or to set things up for you.";

pub const HAMLET: &str =
    "A team of agents that works on one folder of yours.";

pub const LEAD: &str =
    "The hamlet's top agent, the one you talk to first.";

pub const AGENT: &str =
    "An agent working in a hamlet, under its lead or another agent.";

pub const WORKER: &str =
    "A helper started for one task. It ends with its answer.";

pub const PROFILE: &str = "A saved agent with its own memory and \
                           skills, which hamlets can add as agents.";

pub const CITY: &str =
    "Where the profiles live while they are not at work.";

pub const HARBOR: &str =
    "New profiles wait here until you let them into the city.";

pub const LIBRARY: &str = "Skills every agent can use.";

pub const HEARTH: &str = "Your own house in the world.";

pub const NEEDS: &str =
    "Requests and approvals waiting for your response.";

pub const LEFT: &str =
    "Agents and profiles that left. They can be brought back.";

pub const LIMITS: &str = "What the hamlet may spend, and what its \
                          parents must ask you first.";

pub const AGENTS: &str = "Everyone working in the hamlet.";

/// An agent or lead that is no profile of the city.
pub const BLANK: &str = "A fresh agent made for this job alone. \
                         What it learns stays with the job.";

/// A profile as an agent or a lead.
pub const EXISTING_PROFILE: &str = "Brings its memory and skills to \
                                    the job, and can take home what \
                                    it learns.";

/// A hamlet some of whose agents wait for the chief.
pub const MANUAL: &str = "Manual";

/// A hamlet whose agents edit and run commands without waiting.
pub const AUTO: &str = "Auto";

/// What the manual clockwork says.
pub const MANUAL_WORDS: &str =
    "An agent of this hamlet waits for you before it edits a folder \
     or runs a command. Set each agent's permissions from its chat.";

/// What the auto clockwork says.
pub const AUTO_WORDS: &str =
    "Its agents edit the folders and run commands without waiting. \
     The sandbox stays on for all but an agent with full access on \
     this machine; a remote workspace keeps its own.";

/// The levels of an agent's permissions, in T3 Code's words.
pub const SUPERVISED: &str = "Supervised";
pub const SUPERVISED_WORDS: &str =
    "Ask before commands and file changes.";
pub const ACCEPT_EDITS: &str = "Auto-accept edits";
pub const ACCEPT_EDITS_WORDS: &str =
    "Auto-approve edits, ask before other actions.";
pub const AUTO_ACCESS: &str = "Auto";
pub const AUTO_ACCESS_WORDS: &str =
    "Commands and edits run in the sandbox. The network still asks.";
pub const FULL_ACCESS: &str = "Full access";
pub const FULL_ACCESS_WORDS: &str =
    "Allow commands and edits without prompts, with no sandbox. A \
     remote workspace keeps its sandbox and only opens the network.";

pub const PERMISSIONS: &str = "What this agent may do in the \
                               hamlet's folders before it asks you. \
                               A new folder always asks.";

pub const HISTORY: &str =
    "What happened in the hamlet: who came and went, and why.";
