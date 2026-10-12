//! The chief's limits (H22): `limits.toml` in the home, which only a
//! human edits and every check reads again.

mod edit;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
pub use edit::{set, write_template};
use serde::Deserialize;

/// Caps on the three meters of spend. `None` leaves a meter uncapped.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Caps {
    pub tokens: Option<u64>,
    /// In the provider's account currency.
    pub cost: Option<f64>,
    /// Model time.
    pub minutes: Option<u64>,
}

/// Length of a budget's period.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    #[default]
    Day,
    Month,
}

/// The hamlet's budget for a period.
// A meter left out of a written section has no cap, so the fields
// default one by one and the section's own default is used only when
// the whole section is missing.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    #[serde(default)]
    pub period: Period,
    /// Read and not used: a budget is money.
    #[serde(default)]
    pub tokens: Option<u64>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub minutes: Option<u64>,
}

impl Budget {
    pub fn caps(&self) -> Caps {
        Caps {
            tokens: None,
            cost: self.cost,
            minutes: self.minutes,
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            period: Period::Day,
            tokens: None,
            cost: Some(5.0),
            minutes: None,
        }
    }
}

fn messages() -> u32 {
    Task::default().messages
}

/// The caps on one task (H12).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Messages between agents, replies to `tell_agent` included.
    #[serde(default = "messages")]
    pub messages: u32,
    /// Read and not used.
    #[serde(default)]
    pub tokens: Option<u64>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub minutes: Option<u64>,
}

impl Task {
    pub fn caps(&self) -> Caps {
        Caps {
            tokens: None,
            cost: self.cost,
            minutes: self.minutes,
        }
    }
}

impl Default for Task {
    fn default() -> Self {
        Task {
            messages: 12,
            tokens: None,
            cost: Some(1.0),
            minutes: None,
        }
    }
}

/// The mayor's freedoms: the things it may do without asking the
/// chief.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Mayor {
    pub create_agents: bool,
    pub build_roads: bool,
    pub remove_agents: bool,
    pub merge_agents: bool,
    /// Days of the mayor's decisions its agents are told of.
    pub told_days: u64,
}

impl Default for Mayor {
    fn default() -> Self {
        Mayor {
            create_agents: true,
            build_roads: true,
            remove_agents: true,
            merge_agents: true,
            told_days: 7,
        }
    }
}

/// The rules of a hamlet's votes (H13).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Votes {
    /// The share of a hamlet's agents a vote passes with, as a
    /// fraction.
    pub pass: (u32, u32),
    /// The fewest agents besides the mayor a vote can be held with.
    pub least_voters: usize,
    /// Hide the ballots cast so far from each voter.
    pub sealed: bool,
    /// Count a yes only when its evidence was found in the voter's
    /// house.
    pub grounded_only: bool,
    /// Minutes a voter has to cast its ballot.
    pub ballot_minutes: u64,
    /// A vote still open after this long has lapsed.
    pub open_minutes: u64,
    /// Wait of an agent whose vote failed before it calls another of
    /// the same kind.
    pub retry_minutes: u64,
}

impl Default for Votes {
    fn default() -> Self {
        Votes {
            pass: (2, 3),
            least_voters: 3,
            sealed: true,
            grounded_only: false,
            ballot_minutes: 5,
            open_minutes: 60,
            retry_minutes: 60,
        }
    }
}

/// The rules for the hamlets of a home (H14, H19).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Hamlets {
    /// The most hamlets the home may hold.
    pub most: usize,
    /// True when agents of different hamlets may have roads at all.
    pub roads_between: bool,
    /// Mayors of other hamlets it takes to ask for a hamlet's
    /// removal.
    pub removal_mayors: usize,
}

impl Default for Hamlets {
    fn default() -> Self {
        Hamlets {
            most: 6,
            roads_between: true,
            removal_mayors: 3,
        }
    }
}

/// The rules for agents leaving the hamlet (H5, H7).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Leaving {
    /// Minutes an agent stays protected from the mayor after it
    /// answers a chief's task.
    pub protected_minutes: u64,
    /// The most characters of notes a model may write for a merged
    /// agent.
    pub merged_notes: usize,
}

impl Default for Leaving {
    fn default() -> Self {
        Leaving {
            protected_minutes: 0,
            merged_notes: 6000,
        }
    }
}

/// The occasions on which the mayor is asked about an instruction a
/// chief gives one of its agents (H10).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Review {
    Always,
    Never,
    /// Only for a model served from another machine.
    #[default]
    Remote,
}

/// The most agents may ask of the chief (H10, H11).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Asking {
    pub mayor_review: Review,
    /// Refusals in one turn after which an agent's further requests
    /// are refused without asking.
    pub refusals: u32,
    /// Questions one turn may put to the chief.
    pub questions: u32,
}

impl Default for Asking {
    fn default() -> Self {
        Asking {
            mayor_review: Review::Remote,
            refusals: 3,
            questions: 3,
        }
    }
}

/// The agents that may reach past the sandbox without asking, by
/// name (H11, H22).
#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    pub network: Vec<String>,
    pub unconfined: Vec<String>,
}

/// Something an agent may do without asking each time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Network,
    Unconfined,
}

impl Permission {
    pub const ALL: [Self; 2] = [Self::Network, Self::Unconfined];

    pub fn name(self) -> &'static str {
        match self {
            Permission::Network => "network",
            Permission::Unconfined => "unconfined",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        match name {
            "network" => Some(Permission::Network),
            "unconfined" => Some(Permission::Unconfined),
            _ => None,
        }
    }

    /// The names the chief's limits give it to.
    pub fn standing(self, limits: &Permissions) -> &[String] {
        match self {
            Permission::Network => &limits.network,
            Permission::Unconfined => &limits.unconfined,
        }
    }
}

/// The price of one road, on each meter.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Road {
    #[serde(default)]
    pub tokens: u64,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub minutes: u64,
    /// Days without use after which a road is shown grown over.
    #[serde(default = "Road::a_week")]
    pub grows_over_days: u64,
}

impl Road {
    fn a_week() -> u64 {
        7
    }
}

impl Default for Road {
    fn default() -> Self {
        Road {
            tokens: 0,
            cost: 0.01,
            minutes: 0,
            grows_over_days: Road::a_week(),
        }
    }
}

/// Everything `limits.toml` sets.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Models the mayor may choose, besides the hamlet's own. The
    /// home's catalogue has to allow each as well.
    pub models: Vec<String>,
    /// The model an agent of this hamlet runs when nobody chose one
    /// for it. Left out, it is the home's default.
    pub model: Option<String>,
    pub most_agents: usize,
    /// Roads the mayor has built.
    pub most_roads: usize,
    pub budget: Budget,
    pub task: Task,
    pub road: Road,
    pub mayor: Mayor,
    pub votes: Votes,
    pub hamlets: Hamlets,
    pub leaving: Leaving,
    pub asking: Asking,
    pub permissions: Permissions,
    /// The chief's choice of model for an agent, by name.
    pub pinned: BTreeMap<String, String>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            models: vec![],
            model: None,
            most_agents: 12,
            most_roads: 24,
            budget: Budget::default(),
            task: Task::default(),
            road: Road::default(),
            mayor: Mayor::default(),
            votes: Votes::default(),
            hamlets: Hamlets::default(),
            leaving: Leaving::default(),
            asking: Asking::default(),
            permissions: Permissions::default(),
            pinned: BTreeMap::new(),
        }
    }
}

/// The path of the limits of `home`.
pub fn path(home: &Path) -> PathBuf {
    home.join("limits.toml")
}

impl Limits {
    /// The limits of the hamlet at `home`. A missing file is the
    /// defaults; one that does not parse is an error.
    pub fn load(home: &Path) -> Result<Limits> {
        let path = path(home);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Limits::default());
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("reading {}", path.display())
                });
            }
        };
        Limits::read(&text, &path)
    }

    /// The limits `text` holds, as the file at `path` would.
    pub(crate) fn read(text: &str, path: &Path) -> Result<Limits> {
        let limits = toml::from_str::<Limits>(text)
            .with_context(|| format!("parsing {}", path.display()))?;
        let costs = [limits.budget.cost, limits.task.cost]
            .into_iter()
            .flatten()
            .chain([limits.road.cost]);
        for cost in costs {
            if !cost.is_finite() || cost < 0.0 {
                bail!(
                    "{}: a cost is a number of zero or more",
                    path.display()
                );
            }
        }
        let (yes, of) = limits.votes.pass;
        if yes == 0 || of == 0 || yes > of {
            bail!(
                "{}: `pass` under [votes] is a fraction of one or \
                 less, such as [2, 3]",
                path.display()
            );
        }
        Ok(limits)
    }

    /// True when the mayor may choose `model` for an agent.
    pub fn allows(&self, model: &str) -> bool {
        self.models.iter().any(|m| m == model)
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_section_left_out_keeps_its_defaults() {
        let home = Scratch::new("limits-partial");
        std::fs::write(
            path(&home),
            "models = [\"small\"]\n[budget]\nperiod = \
             \"month\"\nminutes = 90\n[task]\nmessages = \
             3\n[pinned]\nscout = \"big\"\n",
        )
        .unwrap();
        let limits = Limits::load(&home).unwrap();
        assert!(limits.allows("small") && !limits.allows("big"));
        assert_eq!(limits.budget.period, Period::Month);
        // A meter left out of a written section has no cap.
        assert_eq!(limits.budget.minutes, Some(90));
        assert_eq!(limits.budget.tokens, None);
        assert_eq!(limits.task.messages, 3);
        assert_eq!(limits.task.tokens, None);
        assert_eq!(limits.road, Road::default());
        assert_eq!(limits.most_agents, 12);
        assert_eq!(limits.pinned["scout"], "big");
    }

    #[test]
    fn a_file_with_a_mistake_is_refused() {
        let home = Scratch::new("limits-mistake");
        for bad in [
            "most_agent = 3\n",
            "[budget]\ntokens = \"lots\"\n",
            "[budget]\ncost = -1.0\n",
            "[budget\n",
            // A vote cannot need more agents than there are.
            "[votes]\npass = [4, 3]\n",
        ] {
            std::fs::write(path(&home), bad).unwrap();
            assert!(Limits::load(&home).is_err(), "{bad}");
        }
    }
}
