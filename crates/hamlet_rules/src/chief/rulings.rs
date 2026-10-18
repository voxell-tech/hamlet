//! The rules of talking back to a chief (H10), the two tools only a
//! chief can answer, and the commands about what waits.

use hamlet_home::limits::Permission;
use serde_json::{Value, json};

use crate::commands::Ruling;

/// The tool an agent argues against its instruction with.
pub const OBJECT: &str = "object";
/// The tool an agent asks a chief questions with.
pub const ASK_CHIEF: &str = "ask_chief";

/// The tool an agent objects to its instruction with.
pub fn object_definition() -> Value {
    json!({"type": "function", "function": {
        "name": OBJECT,
        "description": "Tell the chief why you think the instruction you were just given is a \
                        mistake. The work is held until the chief replies. You can do this once \
                        per instruction, and you may not refuse: if the chief says go ahead, do it.",
        "parameters": {"type": "object", "properties": {
            "reason": {"type": "string", "description": "Why it is a mistake, in a sentence or two."}},
            "required": ["reason"]}}})
}

/// The same tool as the mayor is offered it, about an instruction
/// given to one of its agents.
pub fn review_definition() -> Value {
    json!({"type": "function", "function": {
        "name": OBJECT,
        "description": "Tell the chief why the instruction it gave your agent is a mistake. \
                        The agent's work is held until the chief replies.",
        "parameters": {"type": "object", "properties": {
            "reason": {"type": "string", "description": "Why it is a mistake, in a sentence or two."}},
            "required": ["reason"]}}})
}

/// The tool an agent asks the chief questions with.
// TODO(pi): Pi's RPC extension UI offers only one `select` or `input`
// dialog at a time, with options as bare strings. Under Pi this tool
// becomes an extension that hands the whole `asking::Questions` to
// Hamlet's queue, and not a chain of Pi dialogs.
pub fn ask_chief_definition() -> Value {
    use super::asking::{MOST_OPTIONS, MOST_QUESTIONS};
    json!({"type": "function", "function": {
        "name": ASK_CHIEF,
        "description": "Ask the chief questions you cannot go on without. You wait until the \
                        chief answers, so ask only when no agent and no file can tell you, and \
                        put every question you have in one call. Give a question options when \
                        its likely answers are few; the chief can still answer in words unless \
                        `text_input` is false. The chief sees the questions on a card of their \
                        own, so do not also write them in your reply. The answer is the chief's \
                        reply, each question quoted over its answer: take it as answered, and do \
                        not ask it again. To argue against an instruction, use `object` and not \
                        this.",
        "parameters": {"type": "object", "properties": {
            "message": {"type": "string",
                        "description": "A short line shown over the questions."},
            "questions": {"type": "array", "minItems": 1, "maxItems": MOST_QUESTIONS,
                "items": {"type": "object", "properties": {
                    "question": {"type": "string"},
                    "description": {"type": "string",
                                    "description": "What the chief needs to know to answer."},
                    "options": {"type": "array", "maxItems": MOST_OPTIONS,
                        "items": {"type": "object", "properties": {
                            "title": {"type": "string"},
                            "description": {"type": "string"}},
                            "required": ["title"]}},
                    "text_input": {"type": "boolean",
                                   "description": "The chief may answer in words of their \
                                                   own. Defaults to true."}},
                    "required": ["question"]}}},
            "required": ["questions"]}}})
}

/// The ones who may argue against an instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Party {
    /// The agent that was instructed.
    Agent,
    Mayor,
}

/// The state of the arguing over one instruction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hearing {
    agent: bool,
    mayor: bool,
    /// The chief said to go ahead.
    cleared: bool,
}

impl Hearing {
    /// `Ok` when `who` may object now, and the reason when not.
    pub(crate) fn may_object(
        self,
        who: Party,
    ) -> Result<(), &'static str> {
        let spent = match who {
            Party::Agent => self.agent,
            Party::Mayor => self.mayor,
        };
        if self.cleared {
            Err("the chief has said to go ahead with this \
                 instruction, so nobody may object to it again")
        } else if spent {
            Err("you have already objected to this instruction \
                 once; wait for the chief's reply")
        } else {
            Ok(())
        }
    }

    pub fn object(&mut self, who: Party) -> Result<(), &'static str> {
        self.may_object(who)?;
        match who {
            Party::Agent => self.agent = true,
            Party::Mayor => self.mayor = true,
        }
        Ok(())
    }

    /// The work is held: someone objected and the chief has not
    /// replied.
    pub fn held(self) -> bool {
        (self.agent || self.mayor) && !self.cleared
    }

    pub fn go_ahead(&mut self) {
        self.cleared = true;
    }
}

/// The question to the mayor when a chief instructs one of its
/// agents.
pub fn review_prompt(
    agents: &str,
    notes: &str,
    agent: &str,
    chief: &str,
    instruction: &str,
) -> (String, String) {
    (
        format!(
            "You are the mayor of a hamlet of agents. A chief, a \
             human the hamlet belongs to, has just instructed one \
             of your agents directly. The work has already begun \
             and the chief always wins. If you believe the \
             instruction is a mistake for the hamlet, call \
             `{OBJECT}` with your reason: the work is then held \
             until the chief replies. You may do this once. If it \
             is fine, or you are unsure, reply with the single word \
             `pass`. Most instructions are fine.\n{agents}\n# Your \
             notes\n{notes}"
        ),
        format!("{chief} told `{agent}`: {instruction}"),
    )
}

/// The reason in a model's reply to `review_prompt`, if it objected.
pub fn review_reason(message: &Value) -> Option<String> {
    message["tool_calls"].as_array()?.iter().find_map(|c| {
        if c["function"]["name"] != OBJECT {
            return None;
        }
        let args = serde_json::from_str::<Value>(
            c["function"]["arguments"].as_str()?,
        )
        .ok()?;
        let reason = args["reason"].as_str()?.trim();
        (!reason.is_empty()).then(|| reason.to_string())
    })
}

/// A chief's answer to something that waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Run it this once.
    Allow,
    /// Run it, and let the agent do the like without asking.
    Always,
    /// Allow it for now and ask again at the next launch.
    AskNextTime,
    Deny,
    GoAhead,
    /// Take the instruction back.
    Withdraw,
    /// Answer in words.
    Reply,
    Dismiss,
}

impl Choice {
    pub fn word(self) -> &'static str {
        match self {
            Choice::Allow => "allow",
            Choice::Always => "always",
            Choice::AskNextTime => "ask-next-time",
            Choice::Deny => "deny",
            Choice::GoAhead => "go",
            Choice::Withdraw => "withdraw",
            Choice::Reply => "reply",
            Choice::Dismiss => "dismiss",
        }
    }

    /// The answer a check (`agree`) or a cross gives among
    /// `answers`, when one of them is plain enough for a single
    /// click.
    pub fn quick(answers: &[Choice], agree: bool) -> Option<Choice> {
        let plain: &[Choice] = if agree {
            &[Choice::Allow, Choice::GoAhead]
        } else {
            &[Choice::Deny, Choice::Withdraw]
        };
        answers
            .iter()
            .find(|c| plain.contains(c))
            .or(answers
                .first()
                .filter(|c| agree && **c == Choice::Dismiss))
            .copied()
    }
}

/// Something a chief typed that is not a message to an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Show what waits.
    Queue,
    /// Answer the oldest thing that waits.
    Answer { choice: Choice, text: String },
    /// Stop one agent, or everything when `agent` is `None`.
    Stop { agent: Option<String> },
    Revoke {
        agent: String,
        permission: Permission,
    },
    /// Show what each agent may do without asking.
    Grants,
    /// Reverse the latest thing that can be reversed.
    Undo,
    /// Instruct an agent directly.
    To { agent: String, text: String },
}

/// The command `ruling` with `rest` after its name. `Err` says how
/// it is written when it is written wrongly.
pub fn read(ruling: Ruling, rest: &str) -> Result<Command, String> {
    let answer = |choice: Choice| {
        Ok(Command::Answer {
            choice,
            text: rest.to_string(),
        })
    };
    match ruling {
        Ruling::Queue => Ok(Command::Queue),
        Ruling::Grants => Ok(Command::Grants),
        Ruling::Undo => Ok(Command::Undo),
        Ruling::Allow => answer(Choice::Allow),
        Ruling::Always => answer(Choice::Always),
        Ruling::Deny => answer(Choice::Deny),
        Ruling::Go => answer(Choice::GoAhead),
        Ruling::Withdraw => answer(Choice::Withdraw),
        Ruling::Dismiss => answer(Choice::Dismiss),
        Ruling::Reply if rest.is_empty() => {
            Err("say what to reply: /reply <text>".into())
        }
        Ruling::Reply => answer(Choice::Reply),
        Ruling::Stop => Ok(Command::Stop {
            agent: (!rest.is_empty() && rest != "all")
                .then(|| rest.to_string()),
        }),
        Ruling::Revoke => rest
            .split_once(char::is_whitespace)
            .and_then(|(agent, permission)| {
                Some(Command::Revoke {
                    agent: agent.to_string(),
                    permission: Permission::named(permission.trim())?,
                })
            })
            .ok_or_else(|| {
                "usage: /revoke <agent> <network|unconfined>".into()
            }),
        Ruling::To => match rest.split_once(char::is_whitespace) {
            Some((agent, text)) if !text.trim().is_empty() => {
                Ok(Command::To {
                    agent: agent.to_string(),
                    text: text.trim().to_string(),
                })
            }
            _ => Err("usage: /to <agent> <instruction>".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{self, Handler};

    #[test]
    fn each_may_object_once_and_nobody_after_a_go_ahead() {
        let mut hearing = Hearing::default();
        assert!(!hearing.held());
        hearing.object(Party::Agent).unwrap();
        assert!(hearing.held());
        assert!(hearing.object(Party::Agent).is_err());
        // The mayor has not spoken yet.
        hearing.object(Party::Mayor).unwrap();
        assert!(hearing.object(Party::Mayor).is_err());
        hearing.go_ahead();
        assert!(!hearing.held());
        assert!(hearing.may_object(Party::Agent).is_err());
        assert!(hearing.may_object(Party::Mayor).is_err());

        // A go-ahead closes the instruction to the one that had
        // not objected, too.
        let mut hearing = Hearing::default();
        hearing.object(Party::Agent).unwrap();
        hearing.go_ahead();
        assert!(hearing.object(Party::Mayor).is_err());
        assert!(!hearing.held());
    }

    #[test]
    fn a_review_objects_only_through_the_tool() {
        let passed = json!({"role": "assistant", "content": "I object strongly."});
        assert_eq!(review_reason(&passed), None);
        let objected = json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c1",
            "function": {"name": OBJECT, "arguments": "{\"reason\":\" It deletes the list. \"}"}}]});
        assert_eq!(
            review_reason(&objected),
            Some("It deletes the list.".into())
        );
        let empty = json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c1",
            "function": {"name": OBJECT, "arguments": "{\"reason\":\"\"}"}}]});
        assert_eq!(review_reason(&empty), None);
    }

    #[test]
    fn a_check_and_a_cross_give_only_plain_answers() {
        let approval = [Choice::Allow, Choice::Always, Choice::Deny];
        assert_eq!(
            Choice::quick(&approval, true),
            Some(Choice::Allow)
        );
        assert_eq!(
            Choice::quick(&approval, false),
            Some(Choice::Deny)
        );
        let objection = [Choice::GoAhead, Choice::Withdraw];
        assert_eq!(
            Choice::quick(&objection, false),
            Some(Choice::Withdraw)
        );
        // A question needs words, so neither mark answers it.
        let question = [Choice::Reply, Choice::Dismiss];
        assert_eq!(Choice::quick(&question, true), None);
        assert_eq!(Choice::quick(&question, false), None);
        let fault = [Choice::Dismiss];
        assert_eq!(
            Choice::quick(&fault, true),
            Some(Choice::Dismiss)
        );
        assert_eq!(Choice::quick(&fault, false), None);
    }

    /// The command `line` is, when it is one about what waits.
    fn command(line: &str) -> Option<Result<Command, String>> {
        let typed = commands::read(line)?;
        let Handler::Oversight(ruling) = typed.command.by else {
            return None;
        };
        Some(read(ruling, typed.rest))
    }

    #[test]
    fn a_chief_types_commands() {
        assert_eq!(command("hello"), None);
        assert_eq!(command("/new"), None);
        assert_eq!(command("/queue"), Some(Ok(Command::Queue)));
        assert_eq!(command("/undo"), Some(Ok(Command::Undo)));
        assert_eq!(
            command("/deny not on this machine"),
            Some(Ok(Command::Answer {
                choice: Choice::Deny,
                text: "not on this machine".into()
            }))
        );
        assert_eq!(
            command("/stop"),
            Some(Ok(Command::Stop { agent: None }))
        );
        assert_eq!(
            command("/stop scout"),
            Some(Ok(Command::Stop {
                agent: Some("scout".into())
            }))
        );
        assert_eq!(
            command("/revoke scout network"),
            Some(Ok(Command::Revoke {
                agent: "scout".into(),
                permission: Permission::Network
            }))
        );
        assert!(matches!(command("/revoke scout"), Some(Err(_))));
        assert!(matches!(command("/reply"), Some(Err(_))));
        assert_eq!(
            command("/to scout find the paper"),
            Some(Ok(Command::To {
                agent: "scout".into(),
                text: "find the paper".into()
            }))
        );
    }
}
