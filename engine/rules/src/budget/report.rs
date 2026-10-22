//! Spend beside budget, for the hamlet and for each of its agents.

use hamlet_home::limits::Limits;

use super::{
    Amount, Books, Standing, format_cost, gone, period_words,
};

/// An agent's line in a [`Report`].
#[derive(Clone, Debug, PartialEq)]
pub struct AgentBudget {
    pub name: String,
    /// The model it runs.
    pub model: String,
    /// Its spend from the hamlet's budget in the period.
    pub spent: Amount,
}

impl AgentBudget {
    /// The cost of its spend, as it is shown.
    pub fn cost(&self) -> String {
        format_cost(self.spent.micros as f64 / 1e6)
    }

    pub fn summary(&self) -> String {
        match self.model.as_str() {
            "" => self.cost(),
            model => format!("{model}, {}", self.cost()),
        }
    }
}

/// Spend against budget for the hamlet and each agent, for the
/// window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// "today" or "this month".
    pub period: String,
    /// The share of the period that has gone by, from 0 to 1.
    pub gone: f32,
    pub hamlet: Standing,
    /// The mayor first.
    pub agents: Vec<AgentBudget>,
    /// Reason the limits could not be read.
    pub problem: Option<String>,
}

impl Report {
    pub fn summary(&self) -> String {
        match &self.problem {
            Some(_) => "limits.toml cannot be read".to_string(),
            None => format!(
                "Hamlet {}: {}",
                self.period,
                self.hamlet.summary()
            ),
        }
    }

    pub fn agent(&self, name: &str) -> Option<&AgentBudget> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// The whole report, a line for the hamlet and one an agent.
    pub fn lines(&self) -> String {
        if let Some(problem) = &self.problem {
            return format!("limits.toml cannot be read: {problem}");
        }
        let mut text = self.summary();
        for a in &self.agents {
            text.push_str(&format!(
                "\n- {}: {}",
                a.name,
                a.summary()
            ));
        }
        text
    }
}

/// Builds the report. `agents` are names with the model each runs.
pub fn report(
    limits: &Limits,
    books: &Books,
    agents: Vec<(String, String)>,
) -> Report {
    Report {
        period: period_words(limits.budget.period).0.to_string(),
        gone: gone(limits.budget.period),
        hamlet: Standing {
            spent: books.hamlet,
            caps: limits.budget.caps(),
        },
        agents: agents
            .into_iter()
            .map(|(name, model)| AgentBudget {
                spent: books.of(&name),
                name,
                model,
            })
            .collect(),
        problem: None,
    }
}
