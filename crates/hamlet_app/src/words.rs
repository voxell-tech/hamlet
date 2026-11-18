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

/// A hamlet whose agents ask before each edit and command.
pub(crate) const ASK_FIRST: &str = "Ask First";

/// A hamlet whose agents edit and run commands without asking.
pub(crate) const NO_ASKING: &str = "Don't Ask";

pub(crate) const APPROVALS: &str = "Whether this hamlet's agents \
                                    ask you before they edit its \
                                    folder or run a command.";

pub(crate) const HISTORY: &str =
    "What happened in the hamlet: who came and went, and why.";
