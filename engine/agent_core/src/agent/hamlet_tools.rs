//! The hamlet's own tools as an agent calls them: what a mayor
//! decides, votes, messages, the library and roads.

use anyhow::{Result, anyhow, bail};
use hamlet_rules::civic::Civic;
use hamlet_rules::hamlet::{Alone, Hamlet, MAYOR, tidy_name};
use hamlet_rules::lifecycle::Who;
use hamlet_rules::realm::{self, Realm};
use hamlet_rules::{budget, standing, vote};
use serde_json::Value;

use super::message::no_road;
use super::{Agent, Event, Listener};
use crate::tools::{Call, Leave, Outcome};

/// The tools only a mayor has.
const MAYORS: [&str; 8] = [
    "create_agent",
    "build_road",
    "set_model",
    "remove_agent",
    "merge_agents",
    "agree_road",
    "agree_move",
    "ask_removal",
];

/// One call of a hamlet's tool, with what every tool reads of it.
struct Asked<'a> {
    hamlet: &'a Hamlet,
    realm: &'a Realm,
    args: Value,
    /// The chief reserved the act and then allowed this call.
    allowed: bool,
}

impl Asked<'_> {
    /// The argument `key` as text. A name is read as the realm
    /// addresses it.
    fn text(&self, key: &str) -> String {
        let said =
            self.args[key].as_str().unwrap_or_default().to_string();
        match key {
            "name" | "to" | "a" | "b" | "agent" | "keep"
            | "absorb" => self.realm.plain(&said),
            _ => said,
        }
    }

    /// `Err` when the mayor may not do `act` in this call.
    fn may(&self, act: Alone) -> Result<()> {
        match self.allowed {
            true => Ok(()),
            false => self.hamlet.mayor_may(act),
        }
    }

    /// The authority the mayor acts on in this call.
    fn by(&self) -> Who<'static> {
        match self.allowed {
            true => Who::Allowed,
            false => Who::Mayor,
        }
    }
}

impl Agent {
    /// Runs one of the hamlet's tools. `None` when `call` is not
    /// one, or this agent lives in no hamlet.
    pub(super) async fn hamlet_call(
        &mut self,
        call: &Call,
        leave: Leave,
        on: &mut Listener<'_>,
    ) -> Option<Outcome> {
        let hamlet = &self.hamlet.as_ref()?.again();
        let realm = &self.realm(hamlet);
        let asked = Asked {
            hamlet,
            realm,
            args: serde_json::from_str::<Value>(&call.args)
                .unwrap_or_default(),
            allowed: leave.reserved,
        };
        let done = self.hamlet_tool(&call.name, &asked, on).await?;
        Some(match done {
            Ok(said) => Outcome::plain(0, said),
            Err(e) => Outcome::plain(1, format!("Error: {e}")),
        })
    }

    /// The answer of the hamlet's tool `name`. `None` when the hamlet
    /// has no such tool for this agent.
    async fn hamlet_tool(
        &mut self,
        name: &str,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Option<Result<String>> {
        let hamlet = asked.hamlet;
        if self.answering {
            return Some(Err(anyhow!(
                "this turn is your answer to the vote; reply in \
                 words"
            )));
        }
        if self.ballot.is_some() {
            return self.cast(name, asked);
        }
        // While the hamlet votes, its mayor decides nothing: the
        // voters could be removed or merged as they are asked.
        let decides = MAYORS.contains(&name) && name != "ask_removal";
        if self.mayor
            && decides
            && let Err(held) = vote::wait(hamlet, &self.rules)
        {
            return Some(Err(held));
        }
        Some(match (name, self.mayor) {
            ("create_agent", true) => self.create_agent(asked, on),
            ("build_road", true) => self.build_road(asked, on),
            ("set_model", true) => set_model(asked),
            ("remove_agent", true) => remove_agent(asked, on),
            ("merge_agents", true) => {
                self.merge(
                    &asked.text("keep"),
                    &asked.text("absorb"),
                    &asked.text("purpose"),
                    &asked.text("reason"),
                    asked.by(),
                    on,
                )
                .await
            }
            ("agree_road", true) => self.agree_road(asked, on).await,
            ("agree_move", true) => self.agree_move(asked, on).await,
            ("ask_removal", true) => self.ask_removal(asked, on),
            (name, false) if MAYORS.contains(&name) => Err(anyhow!(
                "only the mayor can do this, and you are not the \
                 mayor of this hamlet"
            )),
            ("call_vote", _) => match vote::motion_of(&asked.args) {
                Ok(motion) => {
                    let reason = asked.text("reason");
                    self.hold_vote(motion, &reason, None, on).await
                }
                Err(e) => Err(e),
            },
            ("tell_agent", _) => self.tell(asked, on),
            ("ask_librarian", _) => {
                let question = asked.text("question");
                self.inquire(hamlet, &question, on).await
            }
            ("submit_skill", _) => {
                let (skill, reason) =
                    (asked.text("skill"), asked.text("reason"));
                self.submit(hamlet, &skill, &reason, on).await
            }
            ("ask_agent", _) => self.ask(asked, on).await,
            ("request_road", false) => {
                self.request_road(asked, on).await
            }
            _ => return None,
        })
    }

    /// Casts the ballot this turn is for. `None` when the voter is
    /// nobody the hamlet knows.
    fn cast(
        &mut self,
        name: &str,
        asked: &Asked<'_>,
    ) -> Option<Result<String>> {
        let polling = self.ballot.as_mut()?;
        if name != "cast_ballot" {
            return Some(Err(anyhow!(
                "this turn is your ballot; call cast_ballot"
            )));
        }
        let evidence = asked.text("evidence");
        let voter =
            self.resident.as_ref().map(|r| r.name.as_str())?;
        let cast = vote::cast(
            asked.hamlet,
            polling.id,
            voter,
            &asked.text("choice"),
            &asked.text("reason"),
            &evidence,
            vote::grounded(&evidence, &polling.corpus, &self.rules),
        );
        Some(cast.map(|choice| {
            polling.cast = Some(choice);
            "Your ballot is cast.".to_string()
        }))
    }

    fn create_agent(
        &self,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let hamlet = asked.hamlet;
        asked.may(Alone::CreateAgents)?;
        let resident = hamlet.create(
            &tidy_name(&asked.text("name")),
            &asked.text("purpose"),
            &asked.text("reason"),
        )?;
        let mut done = format!(
            "Created `{}`. Give it work with ask_agent.",
            resident.name
        );
        on(Event::AgentCreated {
            name: resident.name.clone(),
            purpose: resident.purpose,
        });
        let character = asked.text("character");
        if !character.trim().is_empty() {
            let given =
                hamlet.house(&resident.name).and_then(|house| {
                    Ok(house.set_character(&character)?)
                });
            if let Err(e) = given {
                done.push_str(&format!(
                    " Its character was not written: {e}."
                ));
            }
        }
        let model = asked.text("model");
        if !model.is_empty()
            && let Err(e) = hamlet.choose_model(
                &resident.name,
                &model,
                "chosen when the agent was created",
            )
        {
            done.push_str(&format!(
                " It runs the hamlet's own model: {e}."
            ));
        }
        let roads = asked.args["roads"].as_array();
        for other in
            roads.into_iter().flatten().filter_map(Value::as_str)
        {
            let built = asked.may(Alone::BuildRoads).and_then(|()| {
                hamlet.build_road(
                    &resident.name,
                    other,
                    "laid when the agent was created",
                    &self.task,
                )
            });
            match built {
                Ok(()) => on(Event::RoadBuilt {
                    a: resident.name.clone(),
                    b: other.to_string(),
                }),
                Err(e) => done.push_str(&format!(
                    " No road to `{other}`: {e}."
                )),
            }
        }
        Ok(done)
    }

    fn build_road(
        &self,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let (a, b) = (asked.text("a"), asked.text("b"));
        asked.may(Alone::BuildRoads)?;
        asked.hamlet.build_road(
            &a,
            &b,
            &asked.text("reason"),
            &self.task,
        )?;
        let done = format!("`{a}` and `{b}` now have a road.");
        on(Event::RoadBuilt { a, b });
        Ok(done)
    }

    /// Agrees to a road between an agent of this hamlet and one of
    /// another, and asks the other mayor when it has not agreed yet.
    async fn agree_road(
        &mut self,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let (hamlet, realm) = (asked.hamlet, asked.realm);
        let (a, b, reason) =
            (asked.text("a"), asked.text("b"), asked.text("reason"));
        let built =
            realm.agree_road(hamlet.name(), &a, &b, &reason)?;
        on(Event::Civic(Civic::RoadAgreed {
            a: a.clone(),
            b: b.clone(),
            built,
        }));
        if built {
            return Ok(format!(
                "Both mayors agree: `{a}` and `{b}` now have a road."
            ));
        }
        // The other hamlet is the one that is not this mayor's.
        let other = [&a, &b]
            .into_iter()
            .filter_map(|n| realm.hamlet_of(n))
            .find(|h| h != hamlet.name())
            .unwrap_or_default();
        let ask = format!(
            "The mayor of `{}` agrees to a road between `{a}` and \
             `{b}`. Its reason: {reason}\nDecide. To agree, call \
             agree_road with the same two names and your reason. To \
             refuse, say why in a sentence.",
            hamlet.name()
        );
        let reply = self.send(&realm::office(&other), &ask, on).await;
        Ok(if realm.has_road(&a, &b) {
            format!(
                "The mayor of `{other}` agreed too: `{a}` and `{b}` \
                 now have a road."
            )
        } else {
            format!(
                "You agreed. The mayor of `{other}` has not: {}",
                reply.unwrap_or_else(|e| e.to_string())
            )
        })
    }

    /// Agrees that an agent moves between this hamlet and another,
    /// and asks the other mayor when it has not agreed yet.
    async fn agree_move(
        &mut self,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let (hamlet, realm) = (asked.hamlet, asked.realm);
        let (agent, to, reason) = (
            asked.text("agent"),
            asked.text("to"),
            asked.text("reason"),
        );
        let keep = asked.args["keep_roads"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let from = realm.hamlet_of(&agent).unwrap_or_default();
        let moved = realm.agree_move(
            hamlet.name(),
            &agent,
            &to,
            &keep,
            &reason,
        )?;
        if let Some(m) = moved {
            on(Event::Civic(Civic::AgentMoved {
                agent: m.agent.clone(),
                from: m.from.clone(),
                to: m.to.clone(),
            }));
            return Ok(format!(
                "`{}` moved from `{}` to `{}` with its house. Roads \
                 kept: [{}]. Roads that lapsed: [{}].",
                m.agent,
                m.from,
                m.to,
                m.kept.join(", "),
                m.lapsed.join(", ")
            ));
        }
        let other = if from == hamlet.name() {
            to.clone()
        } else {
            from.clone()
        };
        let ask = format!(
            "The mayor of `{}` agrees that `{agent}` moves from \
             `{from}` to `{to}` with its house, keeping its roads \
             to [{}]. Its reason: {reason}\nDecide. To agree, call \
             agree_move with the same agent and hamlet, and the \
             roads you agree it keeps. To refuse, say why in a \
             sentence.",
            hamlet.name(),
            keep.join(", ")
        );
        let reply = self.send(&realm::office(&other), &ask, on).await;
        let now = realm.hamlet_of(&agent);
        Ok(if now.as_deref() == Some(to.as_str()) {
            format!(
                "The mayor of `{other}` agreed too: `{agent}` now \
                 lives in `{to}`."
            )
        } else {
            format!(
                "You agreed. The mayor of `{other}` has not: {}",
                reply.unwrap_or_else(|e| e.to_string())
            )
        })
    }

    fn ask_removal(
        &self,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let hamlet = asked.hamlet;
        let target = asked.text("hamlet");
        let removal = asked.realm.ask_removal(
            hamlet.name(),
            &target,
            &asked.text("reason"),
        )?;
        on(Event::Civic(Civic::RemovalAsked {
            hamlet: target.clone(),
            by: hamlet.name().to_string(),
            ready: removal.ready,
        }));
        Ok(if removal.ready {
            format!(
                "Asked. The chief of `{target}` has been asked to \
                 confirm; nothing is removed until then."
            )
        } else {
            format!(
                "Asked. {} of the {} other mayors it takes have \
                 asked so far.",
                removal.others.len(),
                self.rules.removal_mayors
            )
        })
    }

    /// Sends a message this agent does not wait on.
    fn tell(
        &self,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let (hamlet, me) = (asked.hamlet, self.name());
        let message = asked.text("message");
        let to = hamlet.address(&asked.text("name"));
        if message.trim().is_empty() {
            bail!("`message` is empty");
        }
        if let Some(gone) = hamlet.whereabouts(&to) {
            bail!("{gone}");
        }
        if self.awaited {
            bail!(
                "another agent is waiting on your reply, so you \
                 cannot wait for a later one. Use ask_agent."
            );
        }
        if hamlet.resolve(&to).is_none()
            && asked.realm.resolve(&to).is_some()
        {
            bail!(
                "`{to}` lives in another hamlet; use ask_agent to \
                 reach it"
            );
        }
        if !hamlet.has_road(&me, &to) {
            bail!("{}", no_road(&to));
        }
        hamlet.may_send(&self.task)?;
        hamlet.carried(&me, &to, &self.task)?;
        on(Event::Message {
            from: me,
            to: to.clone(),
        });
        on(Event::Tell {
            to: to.clone(),
            text: message,
            task: self.task.clone(),
        });
        Ok(format!(
            "Sent. `{to}` is working on it; its reply will come to \
             you as a new message."
        ))
    }

    /// Sends a message and waits for its reply.
    async fn ask(
        &mut self,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let (to, message) =
            (asked.text("name"), asked.text("message"));
        if message.trim().is_empty() {
            bail!("`message` is empty");
        }
        if let Some(gone) = asked.hamlet.whereabouts(&to) {
            bail!("{gone}");
        }
        self.send(&to, &message, on).await
    }

    /// Asks the mayor for a road to another agent.
    async fn request_road(
        &mut self,
        asked: &Asked<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let (hamlet, me) = (asked.hamlet, self.name());
        let (to, reason) = (asked.text("to"), asked.text("reason"));
        let has = || {
            hamlet.has_road(&me, &to)
                || asked.realm.has_road(&me, &to)
        };
        if has() {
            return Ok(format!("You already have a road to `{to}`."));
        }
        let ask = format!(
            "`{me}` asks for a road to `{to}`. Its reason: \
             {reason}\nDecide. To agree, call build_road with your \
             reason, or agree_road if `{to}` lives in another \
             hamlet. To refuse, say why in a sentence."
        );
        let reply = self.send(MAYOR, &ask, on).await?;
        if has() {
            return Ok(format!(
                "The mayor built the road. You can now ask_agent \
                 `{to}`."
            ));
        }
        // Kept, so that it is told what it was refused and why.
        let _ = standing::refuse_road(hamlet, &me, &to, &reply);
        Ok(format!("The mayor did not build it: {reply}"))
    }

    /// Merges `absorb` into `keep` for the mayor. One model call
    /// writes the merged notes when both agents have some.
    async fn merge(
        &mut self,
        keep: &str,
        absorb: &str,
        purpose: &str,
        reason: &str,
        by: Who<'_>,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let Some(hamlet) = self.hamlet.as_ref().map(Hamlet::again)
        else {
            bail!("not in a hamlet");
        };
        hamlet.can_merge(keep, absorb, purpose, reason, by)?;
        let mut notes = None;
        // The budget's reason for leaving the notes as they are.
        let mut unpaid = None;
        if let Some(request) =
            hamlet.merge_request(keep, absorb, purpose)
        {
            // The mayor decided the merge, so the call is the
            // mayor's: its model and its task.
            let me = self.name();
            match hamlet.stopped(&self.task) {
                Some(why) => unpaid = Some(why),
                None => {
                    let model = hamlet.model_of(&me);
                    let written = self
                        .provider
                        .complete(
                            &request,
                            &[],
                            model.as_deref(),
                            &mut |_| {},
                        )
                        .await;
                    // Without a reply the two notes are joined.
                    if let Ok(done) = written {
                        notes = done.message["content"]
                            .as_str()
                            .map(String::from);
                        self.log.model_call(&done.call)?;
                        let estimate = budget::estimate(
                            &request,
                            &[],
                            &done.message,
                        );
                        hamlet.spend(
                            &me, &self.task, &done.call, estimate,
                        )?;
                        on(Event::ModelCall(done.call));
                    }
                }
            }
        }
        let merged = hamlet.merge(
            keep,
            absorb,
            purpose,
            reason,
            by,
            notes.as_deref(),
        )?;
        on(Event::AgentsMerged {
            kept: merged.kept.name.clone(),
            gone: merged.gone.name.clone(),
        });
        let mut report = merged.report();
        if let Some(why) = unpaid {
            report.push_str(&format!(
                " No model was asked to rewrite them: {why}"
            ));
        }
        Ok(report)
    }
}

fn set_model(asked: &Asked<'_>) -> Result<String> {
    let (name, model) = (asked.text("name"), asked.text("model"));
    asked.hamlet.choose_model(
        &name,
        &model,
        &asked.text("reason"),
    )?;
    Ok(if model.is_empty() {
        format!("`{name}` now runs the hamlet's own model.")
    } else {
        format!("`{name}` now runs `{model}`.")
    })
}

fn remove_agent(
    asked: &Asked<'_>,
    on: &mut Listener<'_>,
) -> Result<String> {
    let name = asked.text("name");
    let gone = asked.hamlet.remove(
        &name,
        &asked.text("reason"),
        asked.by(),
    )?;
    let done = format!(
        "Removed `{name}`. Its house is archived as {} and its \
         roads are gone. A chief can restore it.",
        gone.id
    );
    on(Event::AgentRemoved { name });
    Ok(done)
}
