//! The words a mayor and a resident are told of their hamlet, and the
//! tools they are offered.

use serde_json::{Value, json};

use super::{Hamlet, Resident};
use crate::vote;

/// Number of the latest direct instructions the mayor is shown.
const MAYOR_HEARS: usize = 5;

/// The most of an instruction the mayor is shown, in characters.
const OPENING: usize = 160;

const VOTES: &str =
    "\nAny agent may call a vote with `call_vote`, to replace the \
     mayor or to found a new hamlet under a new mayor. Every agent \
     then votes from its own house, it passes with two thirds of \
     them, and the chief is told and may overturn it. Call one only \
     on grounds from your own work, not on another agent's say-so.\n";

impl Hamlet {
    /// The latest direct instructions, oldest first: who gave it, to
    /// which agent, and its opening words.
    fn instructions(&self) -> Vec<String> {
        let events = self.events();
        let mut lines = events
            .iter()
            .rev()
            .filter(|e| e["type"] == "instructed")
            .take(MAYOR_HEARS)
            .filter_map(|e| {
                let text = e["text"].as_str()?;
                let opening =
                    text.chars().take(OPENING).collect::<String>();
                Some(format!(
                    "- {} told {}: {}",
                    e["user"].as_str()?,
                    e["name"].as_str()?,
                    opening
                ))
            })
            .collect::<Vec<_>>();
        lines.reverse();
        lines
    }

    /// The tools of whoever is mayor.
    pub fn definitions() -> Vec<Value> {
        vec![
            json!({"type": "function", "function": {
                "name": "create_agent",
                "description": "Create a new agent in your hamlet, with its own house: its own \
                                notes, skills and memory. It starts knowing nothing but its purpose.",
                "parameters": {"type": "object", "properties": {
                    "name": {"type": "string", "description": "Lowercase letters and hyphens, such as `librarian`."},
                    "purpose": {"type": "string", "description": "What this agent is for, in a sentence or two. It is told this every turn."},
                    "reason": {"type": "string", "description": "Why the hamlet needs it. Kept in the record."},
                    "roads": {"type": "array", "items": {"type": "string"},
                        "description": "Agents it will plainly work with, to lay roads to now. Each costs budget. It always has a road to you."},
                    "model": {"type": "string", "description": "The model it runs, from those you may choose. Leave out for the hamlet's own."},
                    "character": {"type": "string", "description": "How it behaves and speaks, told to it every turn. Leave out for none. Only the chief can change it later."}},
                    "required": ["name", "purpose", "reason"]}}}),
            ask_definition(
                "Give one of your agents a message and wait for its \
                 reply. It works in its own house and remembers \
                 past messages from you.",
            ),
            tell_definition(),
            crate::library::ask_definition(),
            crate::library::submit_definition(),
            json!({"type": "function", "function": {
                "name": "build_road",
                "description": "Lay a road between two of your agents so they can talk to each other \
                                without going through you. It costs budget; build one where two agents \
                                will keep working together.",
                "parameters": {"type": "object", "properties": {
                    "a": {"type": "string"}, "b": {"type": "string"},
                    "reason": {"type": "string", "description": "Kept in the record."}},
                    "required": ["a", "b", "reason"]}}}),
            json!({"type": "function", "function": {
                "name": "set_model",
                "description": "Choose the model one of your agents runs, from those the chief allows.",
                "parameters": {"type": "object", "properties": {
                    "name": {"type": "string"},
                    "model": {"type": "string", "description": "Empty for the hamlet's own model."},
                    "reason": {"type": "string", "description": "Kept in the record."}},
                    "required": ["name", "model", "reason"]}}}),
            json!({"type": "function", "function": {
                "name": "remove_agent",
                "description": "Remove one of your agents, at once. Its house is archived, never deleted, \
                                and its roads go with it; only a chief can bring it back. Refused while \
                                the agent has a task from a chief.",
                "parameters": {"type": "object", "properties": {
                    "name": {"type": "string"},
                    "reason": {"type": "string", "description": "Kept in the record, and told to agents that try to reach it."}},
                    "required": ["name", "reason"]}}}),
            json!({"type": "function", "function": {
                "name": "merge_agents",
                "description": "Merge two of your agents into one, at once. `keep` stays, under its own \
                                name, with the notes, skills, files and roads of both; `absorb` is archived. \
                                Costs one model call to rewrite the notes. Refused while either has a task \
                                from a chief.",
                "parameters": {"type": "object", "properties": {
                    "keep": {"type": "string", "description": "The agent that stays. Pick the one others already talk to most."},
                    "absorb": {"type": "string", "description": "The agent merged into it."},
                    "purpose": {"type": "string", "description": "What the merged agent is for: the work of both, in a sentence or two."},
                    "reason": {"type": "string", "description": "Kept in the record."}},
                    "required": ["keep", "absorb", "purpose", "reason"]}}}),
            vote::call_definition(),
        ]
    }

    /// The tools of an agent that is not the mayor.
    pub fn resident_definitions() -> Vec<Value> {
        vec![
            ask_definition(
                "Send a message along a road and wait for the \
                 reply. `name` is `mayor` or an agent you have a \
                 road to.",
            ),
            tell_definition(),
            crate::library::ask_definition(),
            crate::library::submit_definition(),
            json!({"type": "function", "function": {
                "name": "request_road",
                "description": "Ask the mayor for a road to another agent, so you can talk to it \
                                directly from now on. The mayor decides. For a single question, \
                                ask the mayor to pass it on instead.",
                "parameters": {"type": "object", "properties": {
                    "to": {"type": "string"},
                    "reason": {"type": "string"}},
                    "required": ["to", "reason"]}}}),
            vote::call_definition(),
        ]
    }

    /// The mayor's part of the system prompt, with `budget` as its
    /// words on the budget.
    pub fn prompt(&self, budget: &str) -> String {
        let mut p = String::from(
            "\n# Your hamlet\nYou are the mayor of this hamlet. \
             `create_agent` adds an agent with its own house (its \
             own notes, skills and memory, separate from yours), \
             `ask_agent` gives one work and returns its reply, and \
             `build_road` lets two agents talk to each other \
             directly; an agent without a road to another can only \
             reach it through you. Give `create_agent` a \
             `character` to say how the new agent behaves. Create \
             an agent when a kind of work will come back and \
             deserves its own memory; do small one-off things \
             yourself. Before creating one, check the list below \
             for an agent that already fits. `merge_agents` makes \
             one agent of two whose work overlaps, and \
             `remove_agent` retires one nobody needs; both keep \
             everything in the archive, and neither may be used on \
             an agent that has a task from a chief.\n",
        );
        let census = self.census();
        if census.mayor.id != 0 {
            p.push_str(&format!(
                "You are `{}`, and you were made mayor in place of \
                 another. Your work before was: {} The former \
                 mayors are ordinary agents now and are in the \
                 list.\n",
                census.mayor.name, census.mayor.purpose
            ));
        }
        p.push_str(
            "Your commands can read each agent's workspace, shown \
             beside it, and cannot write there. Read a file an \
             agent made from it, and ask the agent for any \
             change.\nAgents:\n",
        );
        let residents = census.residents.clone();
        if residents.is_empty() {
            p.push_str("(none yet)\n");
        }
        let workspaces = self.workspaces();
        for r in residents {
            p.push_str(&format!("- {}: {}", r.name, r.purpose));
            if let Some((_, workspace)) =
                workspaces.iter().find(|(name, _)| *name == r.name)
            {
                p.push_str(&format!(" ({})", workspace.display()));
            }
            p.push('\n');
        }
        p.push_str(
            &census.farewells(None, self.lifecycle.gone_shown),
        );
        p.push_str(budget);
        let instructions = self.instructions();
        if !instructions.is_empty() {
            p.push_str(
                "A chief can instruct your agents directly, without \
                 asking you. You may tell the chief if you think \
                 one of these was a mistake, but you may not undo \
                 it or act against the agent for following it. The \
                 latest:\n",
            );
            for line in instructions {
                p.push_str(&line);
                p.push('\n');
            }
        }
        p.push_str(VOTES);
        p
    }

    /// A resident's part of the system prompt, with `budget` as its
    /// words on the budget.
    pub fn identity(
        &self,
        resident: &Resident,
        budget: &str,
    ) -> String {
        let census = self.census();
        let roads = census
            .roads
            .iter()
            .cloned()
            .filter_map(|r| {
                let (a, b) = r.ends;
                if a == resident.name {
                    Some(b)
                } else if b == resident.name {
                    Some(a)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut others = census
            .residents
            .iter()
            .filter(|r| r.name != resident.name)
            .map(|r| format!("- {}: {}\n", r.name, r.purpose))
            .collect::<String>();
        if others.is_empty() {
            others.push_str("(none)\n");
        }
        others.push_str(
            &census
                .farewells(Some(resident), self.lifecycle.gone_shown),
        );
        format!(
            "\n# Your place\nYou are `{}`, an agent in a hamlet. \
             Your purpose: {}\nMessages come from the mayor or from \
             a chief, a human the hamlet belongs to. The chief \
             cannot see your workspace and the mayor can only read \
             it: put what they need in your reply, and name a file \
             you made by its full path.\nA chief's instruction \
             outranks the mayor's. If you think a chief's \
             instruction is a mistake, say why once and wait for \
             the reply before doing anything; if the chief then \
             says to go ahead, do it and do not object again. You \
             may not refuse.\nYou can talk only along your roads, \
             with `ask_agent`. Your roads lead to: {roads}.\n{}The \
             other agents of the hamlet:\n{}{VOTES}{}",
            resident.name,
            resident.purpose,
            budget,
            others,
            crate::standing::note(self, &resident.name)
        )
    }
}

fn tell_definition() -> Value {
    json!({"type": "function", "function": {
        "name": "tell_agent",
        "description": "Send a message along a road without waiting. The other agent works \
                        on it while you carry on, and its reply comes to you later as a new \
                        message. Use it to set several agents working at once; use ask_agent \
                        when you need the answer before you can go on.",
        "parameters": {"type": "object", "properties": {
            "name": {"type": "string"},
            "message": {"type": "string", "description": "Everything it needs to know; it cannot see your conversation."}},
            "required": ["name", "message"]}}})
}

fn ask_definition(description: &str) -> Value {
    json!({"type": "function", "function": {
        "name": "ask_agent",
        "description": description,
        "parameters": {"type": "object", "properties": {
            "name": {"type": "string"},
            "message": {"type": "string", "description": "Everything it needs to know; it cannot see your conversation."}},
            "required": ["name", "message"]}}})
}
