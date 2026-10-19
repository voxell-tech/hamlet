//! A plugin that watches the agents' turns: it is told when each
//! begins and ends, with the names of the tools it used, and keeps
//! a count for each agent. It cannot change or stop a turn, and it
//! never hears what was said.

use hamlet_guest::events::{self, Turn};
use hamlet_guest::ui::{View, column, heading, note};
use hamlet_guest::{Incoming, Intent, Level, Plugin, State};
use serde::{Deserialize, Serialize};

/// What the host holds across a reload.
#[derive(Default, Serialize, Deserialize)]
struct Model {
    agents: Vec<Agent>,
}

/// What is known of one agent.
#[derive(Serialize, Deserialize)]
struct Agent {
    name: String,
    turns: u64,
    /// Each tool, with the times it was used.
    tools: Vec<(String, u32)>,
}

impl Model {
    fn note(&mut self, turn: &Turn, ended: bool) {
        let at = self.agents.iter().position(|a| a.name == turn.agent);
        let agent = match at {
            Some(at) => &mut self.agents[at],
            None => {
                self.agents.push(Agent {
                    name: turn.agent.clone(),
                    turns: 0,
                    tools: Vec::new(),
                });
                self.agents.last_mut().expect("just pushed")
            }
        };
        agent.turns = agent.turns.max(turn.number);
        if !ended {
            return;
        }
        for tool in &turn.tools {
            match agent.tools.iter_mut().find(|(name, _)| name == tool) {
                Some((_, times)) => *times += 1,
                None => agent.tools.push((tool.clone(), 1)),
            }
        }
    }
}

struct Diary;

impl Plugin for Diary {
    fn entry() {
        hamlet_guest::panel("diary", "DIARY", "book");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        let mut heard = false;
        for incoming in inbox {
            let Incoming::Event(event) = incoming else {
                continue;
            };
            let Some(turn) = events::turn(&event) else {
                continue;
            };
            heard = true;
            model.note(&turn, event.topic == "after-turn");
        }
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        match heard {
            true => vec![Intent::Redraw("diary".into())],
            false => Vec::new(),
        }
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        let mut rows = vec![heading("DIARY")];
        if model.agents.is_empty() {
            rows.push(note("No turn yet."));
        }
        for agent in &model.agents {
            let tools = agent
                .tools
                .iter()
                .map(|(name, times)| format!("{name} x{times}"))
                .collect::<Vec<_>>()
                .join(", ");
            rows.push(note(&format!(
                "{}: {} turns. {tools}",
                agent.name, agent.turns
            )));
        }
        column(rows).into()
    }
}

hamlet_guest::export!(Diary);
