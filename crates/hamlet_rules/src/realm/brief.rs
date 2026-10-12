//! An agent's brief on the other hamlets, and the tools a mayor deals
//! with them by.

use serde_json::{Value, json};

use super::Realm;
use crate::hamlet::Hamlet;
use crate::record::text;

impl Realm {
    /// The things a mayor waits for `hamlet`'s mayor to decide: roads
    /// and moves the other mayor has agreed to.
    fn awaiting(&self, hamlet: &str) -> Vec<String> {
        let events = self.events();
        let roads = self.roads();
        let everyone = self.everyone_in(&self.places_in(&events).0);
        let home_of = |name: &str| {
            everyone
                .iter()
                .find(|(r, _)| r.name == name)
                .map(|(_, h)| h.clone())
        };
        let mut lines = Vec::new();
        let mut seen = Vec::<(String, String)>::new();
        for e in &events {
            match e["type"].as_str() {
                Some("road_agreed") => {
                    let (a, b) = (text(e, "a"), text(e, "b"));
                    if seen.contains(&(a.clone(), b.clone()))
                        || a != b
                            && roads.iter().any(|r| r.joins(&a, &b))
                    {
                        continue;
                    }
                    let agreed =
                        Realm::road_agreements(&events, &a, &b);
                    let mine = [home_of(&a), home_of(&b)]
                        .contains(&Some(hamlet.to_string()));
                    if mine
                        && !agreed.is_empty()
                        && !agreed.iter().any(|h| h == hamlet)
                    {
                        lines.push(format!(
                            "- the mayor of `{}` agreed to a road \
                             between `{a}` and `{b}`: {} Agree with \
                             agree_road, or leave it.",
                            agreed[0],
                            text(e, "reason")
                        ));
                    }
                    seen.push((a, b));
                }
                Some("move_agreed") => {
                    let (agent, to) =
                        (text(e, "agent"), text(e, "to"));
                    let key = (agent.clone(), to.clone());
                    if seen.contains(&key) {
                        continue;
                    }
                    let agreed =
                        Realm::move_agreements(&events, &agent, &to);
                    let from = home_of(&agent);
                    let mine = from.as_deref() == Some(hamlet)
                        || to == hamlet;
                    if mine
                        && from.as_deref() != Some(to.as_str())
                        && !agreed.is_empty()
                        && !agreed.iter().any(|(h, _)| h == hamlet)
                    {
                        lines.push(format!(
                            "- the mayor of `{}` agreed that \
                             `{agent}` moves to `{to}`, keeping its \
                             roads to [{}]: {} Agree with \
                             agree_move, or leave it.",
                            agreed[0].0,
                            agreed[0].1.join(", "),
                            text(e, "reason")
                        ));
                    }
                    seen.push(key);
                }
                _ => {}
            }
        }
        lines
    }

    /// The realm's part of the system prompt of `name`, an agent of
    /// `hamlet`. Empty while there is one hamlet and no road out of
    /// it.
    pub fn brief(&self, hamlet: &Hamlet, name: &str) -> String {
        let me = self.address(hamlet, name);
        let across = self
            .roads()
            .into_iter()
            .filter(|r| r.has(&me))
            .map(|r| r.other(&me).to_string())
            .collect::<Vec<_>>();
        if !hamlet.is_mayor(name) {
            if across.is_empty() {
                return String::new();
            }
            return format!(
                "Your roads to agents of other hamlets lead to: {}. \
                 Whoever asks pays for the answer.\n",
                across.join(", ")
            );
        }
        let others = self
            .hamlets()
            .into_iter()
            .filter(|p| p.name != hamlet.name())
            .collect::<Vec<_>>();
        if others.is_empty() {
            return String::new();
        }
        let mut p = String::from(
            "\n# Other hamlets\nYou have a road to every other \
             mayor: reach one with `ask_agent` as `mayor@<hamlet>`. \
             Whoever asks pays for the answer. Between two hamlets \
             a thing is done only when both mayors agree: \
             `agree_road` for a road between one of your agents and \
             one of theirs, `agree_move` for an agent to move with \
             its house. `ask_removal` asks the chief to remove a \
             hamlet.\n",
        );
        for place in others {
            let there = Hamlet::at(&self.home, &place.name);
            let agents = there
                .residents()
                .into_iter()
                .map(|r| r.name)
                .collect::<Vec<_>>()
                .join(", ");
            p.push_str(&format!(
                "- {}: mayor `{}`; agents: {}\n",
                place.name,
                there.mayor().name,
                if agents.is_empty() { "(none)" } else { &agents }
            ));
        }
        let awaiting = self.awaiting(hamlet.name());
        if !awaiting.is_empty() {
            p.push_str("Waiting for your decision:\n");
            for line in awaiting {
                p.push_str(&line);
                p.push('\n');
            }
        }
        p
    }

    /// The tools a mayor deals with other hamlets by.
    pub fn definitions() -> Vec<Value> {
        vec![
            json!({"type": "function", "function": {
                "name": "agree_road",
                "description": "Agree to a road between one of your agents and an agent of another \
                                hamlet. The road exists once both mayors have agreed. If you are the \
                                first to agree, the other mayor is asked and its answer returned.",
                "parameters": {"type": "object", "properties": {
                    "a": {"type": "string"}, "b": {"type": "string"},
                    "reason": {"type": "string", "description": "Kept in the record and shown to the other mayor."}},
                    "required": ["a", "b", "reason"]}}}),
            json!({"type": "function", "function": {
                "name": "agree_move",
                "description": "Agree that an agent moves to another hamlet with its house: one of \
                                yours leaving, or another's joining you. It moves once both mayors \
                                have agreed. If you are the first to agree, the other mayor is asked.",
                "parameters": {"type": "object", "properties": {
                    "agent": {"type": "string"},
                    "to": {"type": "string", "description": "The hamlet it would join."},
                    "keep_roads": {"type": "array", "items": {"type": "string"},
                        "description": "Agents of the hamlet it leaves that it should keep a road to. A road stays only if both mayors name it."},
                    "reason": {"type": "string"}},
                    "required": ["agent", "to", "reason"]}}}),
            json!({"type": "function", "function": {
                "name": "ask_removal",
                "description": "Ask for a hamlet to be removed: your own by your decision, or another's, \
                                which takes three mayors. The chief must confirm before anything is removed.",
                "parameters": {"type": "object", "properties": {
                    "hamlet": {"type": "string"},
                    "reason": {"type": "string"}},
                    "required": ["hamlet", "reason"]}}}),
        ]
    }
}
