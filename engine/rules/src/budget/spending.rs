//! One hamlet's budget on disk: its charges, and what its agents are
//! told of them.

use anyhow::Result;
use hamlet_home::limits::Limits;
use hamlet_model::catalogue;
use hamlet_model::provider::CallRecord;
use serde_json::json;

use super::{
    Amount, Books, Charge, Ledger, Meter, Report, cannot_afford,
    report, road_price, shown, stop, today,
};
use crate::hamlet::{Census, Hamlet, MAYOR};
use crate::models::allowed_words;

/// The name in a prompt for the model of an agent nobody chose one
/// for.
const OWN_MODEL: &str = "the hamlet's own model";

impl Hamlet {
    /// Starts the hamlet's budget again for the period it is in,
    /// at the word of the chief `by`.
    pub(crate) fn reset_budget(&self, by: &str) -> Result<()> {
        Ledger::new(self.root()).reset(today(), by)?;
        self.append(json!({ "type": "budget_reset", "by": by }))
    }

    fn books(&self, limits: &Limits, task: &str) -> Books {
        Books::read(
            &Ledger::new(self.root()),
            limits.budget.period,
            today(),
            task,
        )
    }

    /// Reason nobody in the hamlet may make a model call for `task`,
    /// in words for the one who gave the task. `None` when they may.
    pub fn stopped(&self, task: &str) -> Option<String> {
        let limits = match self.limits() {
            Ok(limits) => limits,
            Err(e) => {
                return Some(format!(
                    "The chief's limits cannot be read, so nothing \
                     may be spent until the chief fixes the file: \
                     {e:#}"
                ));
            }
        };
        stop(&limits, &self.books(&limits, task))
    }

    /// Charges one model call of `agent` to the hamlet.
    pub fn spend(
        &self,
        agent: &str,
        task: &str,
        call: &CallRecord,
        estimate: u64,
    ) -> Result<()> {
        Ledger::new(self.root()).charge(
            today(),
            &Charge {
                agent,
                task,
                what: "model",
                model: &call.model,
                amount: Amount::of(
                    call,
                    estimate,
                    catalogue::price(self.home(), &call.model),
                ),
                estimated: call.input_tokens.is_none()
                    && call.output_tokens.is_none(),
            },
        )
    }

    /// Charges a road to the hamlet, or says why it cannot pay.
    pub(crate) fn pay_for_road(
        &self,
        limits: &Limits,
        task: &str,
    ) -> Result<()> {
        let price = road_price(limits);
        if let Some(why) =
            cannot_afford(limits, &self.books(limits, task), price)
        {
            anyhow::bail!("{why}");
        }
        Ledger::new(self.root()).charge(
            today(),
            &Charge {
                agent: MAYOR,
                task,
                what: "road",
                model: "",
                amount: price,
                estimated: false,
            },
        )
    }

    /// Refuses a message once `task` has sent all it may (H12).
    pub fn may_send(&self, task: &str) -> Result<()> {
        let most = self.limits()?.task.messages;
        let sent = self
            .events()
            .iter()
            .filter(|e| e["type"] == "message" && e["task"] == task)
            .count();
        if sent >= most as usize {
            anyhow::bail!(
                "this task has used all {most} of its messages \
                 between agents; finish with what you have and \
                 report. The chief can raise it under One Task on \
                 the hamlet's Bylaws page"
            );
        }
        Ok(())
    }

    /// The budget's part of the system prompt of `name`, the mayor's
    /// when `mayor`.
    pub fn budget_words(&self, name: &str, mayor: bool) -> String {
        match mayor {
            true => self.budget_prompt(),
            false => self.budget_line(name),
        }
    }

    /// The budget's part of the mayor's system prompt.
    fn budget_prompt(&self) -> String {
        let Ok(limits) = self.limits() else {
            return String::new();
        };
        let census = self.census();
        let runs = self.models(&limits);
        let mut p = format!(
            "Budget: you and every agent spend from the hamlet's \
             one budget, and only the chief can change it. A road \
             costs {}. `set_model` chooses an agent's model. A \
             dearer one spends the budget faster, so give each \
             agent the cheapest that does its work well. You may \
             choose: {}\n{}\n",
            shown(Meter::Tokens, limits.road.tokens),
            allowed_words(&runs),
            self.report_in(&limits, &census, &runs, OWN_MODEL)
                .lines()
        );
        for r in &census.residents {
            let pinned = limits.pinned.contains_key(&r.name);
            let chosen = census.model(&r.name);
            if let Some(model) = chosen.filter(|model| {
                !pinned && !runs.iter().any(|m| m.name() == *model)
            }) {
                p.push_str(&format!(
                    "The hamlet may no longer run `{model}`, so \
                     `{}` runs the hamlet's own model.\n",
                    r.name
                ));
            }
        }
        p
    }

    /// The budget's part of a resident's system prompt.
    fn budget_line(&self, name: &str) -> String {
        self.report(OWN_MODEL)
            .agent(name)
            .map(|a| {
                format!(
                    "Everyone in the hamlet spends from its one \
                     budget. Your spend: {}.\n",
                    a.summary()
                )
            })
            .unwrap_or_default()
    }

    /// Spend against budget and the model of each agent. `own_model`
    /// names the model of an agent nobody chose one for.
    pub fn report(&self, own_model: &str) -> Report {
        match self.limits() {
            Ok(limits) => self.report_in(
                &limits,
                &self.census(),
                &self.models(&limits),
                own_model,
            ),
            Err(e) => Report {
                problem: Some(format!("{e:#}")),
                ..Report::default()
            },
        }
    }

    /// The report from a census and a list of models already read.
    fn report_in(
        &self,
        limits: &Limits,
        census: &Census,
        runs: &[catalogue::Model],
        own_model: &str,
    ) -> Report {
        let agents = std::iter::once(MAYOR)
            .chain(census.residents.iter().map(|r| r.name.as_str()))
            .map(|name| {
                let model =
                    Hamlet::model_in(census, limits, runs, name)
                        .unwrap_or_else(|| own_model.to_string());
                (name.to_string(), model)
            })
            .collect();
        report(limits, &self.books(limits, ""), agents)
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    fn hamlet(tag: &str, limits: &str) -> (Scratch, Hamlet) {
        let dir = Scratch::new(&format!("spending-{tag}"));
        set_limits(&dir, limits);
        let hamlet = Hamlet::new(dir.to_path_buf());
        for name in ["librarian", "scout", "clerk"] {
            hamlet.create(name, "Works.", "test").unwrap();
        }
        (dir, hamlet)
    }

    fn set_limits(home: &std::path::Path, text: &str) {
        std::fs::write(hamlet_home::limits::path(home), text)
            .unwrap();
    }

    #[test]
    fn the_limits_say_how_many_agents_a_hamlet_may_have() {
        let (dir, hamlet) = hamlet("agents", "most_agents = 3\n");
        let full = hamlet.create("fourth", "Works.", "test");
        assert!(
            full.unwrap_err().to_string().contains("full (3 agents)")
        );
        set_limits(&dir, "most_agents = 4\n");
        hamlet.create("fourth", "Works.", "test").unwrap();
    }

    #[test]
    fn a_road_is_charged_to_the_hamlet_and_counted_against_the_limit()
    {
        let text = "most_roads = 2\n[budget]\ncost = \
                    10.0\n[road]\ncost = 4.0\n";
        let (dir, hamlet) = hamlet("roads", text);
        hamlet.build_road("librarian", "scout", "r", "t").unwrap();
        let report = hamlet.report("own");
        assert_eq!(report.hamlet.spent.micros, 4_000_000);
        assert_eq!(
            report.agent(MAYOR).unwrap().spent.micros,
            4_000_000
        );
        hamlet.build_road("librarian", "clerk", "r", "t").unwrap();
        // The hamlet has two dollars left and a road costs four.
        set_limits(&dir, &text.replace("most_roads = 2", ""));
        let poor = hamlet.build_road("scout", "clerk", "r", "t");
        let said = poor.unwrap_err().to_string();
        assert!(said.contains("cannot pay for it today"), "{said}");
        assert_eq!(
            hamlet.report("own").hamlet.spent.micros,
            8_000_000
        );
        // The chief's cap on roads holds whatever the budget.
        set_limits(&dir, "most_roads = 2\n");
        let many = hamlet.build_road("scout", "clerk", "r", "t");
        let said = many.unwrap_err().to_string();
        assert!(said.contains("all the roads the chief allows (2)"));
        assert!(!hamlet.has_road("scout", "clerk"));
    }

    #[test]
    fn a_task_may_send_only_so_many_messages() {
        let (_dir, hamlet) =
            hamlet("messages", "[task]\nmessages = 2\n");
        hamlet.may_send("t").unwrap();
        hamlet.carried(MAYOR, "scout", "t").unwrap();
        hamlet.carried("scout", MAYOR, "t").unwrap();
        let said = hamlet.may_send("t").unwrap_err().to_string();
        assert!(
            said.contains("used all 2 of its messages"),
            "{said}"
        );
        // Another task has its own count.
        hamlet.may_send("u").unwrap();
    }
}
