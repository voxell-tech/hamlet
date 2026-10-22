//! Making room in a conversation that has outgrown its model: the
//! model's summary of it takes its place.

use anyhow::Result;
use hamlet_model::catalogue::Catalogue;
use hamlet_model::provider::CallRecord;
use hamlet_rules::budget;
use serde_json::{Value, json};

use crate::agent::{Agent, Event, Listener};

/// The share of a model's window a conversation may fill before it is
/// summarised.
const FULL: f64 = 0.8;

/// The share of a model's window the conversation takes where its
/// summary is asked for.
const SHOWN: f64 = 0.5;

/// The most of one message shown where the summary is asked for, in
/// characters.
const MESSAGE_SHOWN: usize = 4000;

/// The characters a token is taken to be, as `budget::estimate` has
/// it.
const TOKEN_CHARS: usize = 4;

/// The tokens under which a reply that was cut off ran out of window.
/// A longer one reached the length its server allows a reply.
const NO_ROOM: u64 = 1024;

const SUMMARISER: &str = "You write the summary of a conversation \
                          between a user and an agent that works \
                          with tools, so that the agent can carry \
                          on from your summary alone.";

const ASK: &str =
    "Summarise the conversation above. Give, in this order: what \
     the user asked for, with the latest request in its own words; \
     what has been done, with the files, commands and results that \
     matter; what was learned, and what failed and why; what is \
     left to do, and the next step. Be exact about names, paths and \
     numbers. Write only the summary.";

const OPENING: &str = "This conversation grew longer than the model \
                       can hold, so everything before this point \
                       was replaced by the summary below. Carry on \
                       from it.\n\n";

const WAITING: &str = "The message to answer now:\n";

const NOTICE: &str = "The conversation grew longer than the model \
                      can hold, so the earlier part of it was \
                      summarised.";

/// The start and the end of `text`, where it is longer than
/// [`MESSAGE_SHOWN`].
fn clipped(text: &str) -> String {
    let count = text.chars().count();
    if count <= MESSAGE_SHOWN {
        return text.to_string();
    }
    let half = MESSAGE_SHOWN / 2;
    let start = text.chars().take(half).collect::<String>();
    let end = text.chars().skip(count - half).collect::<String>();
    format!(
        "{start}\n[{} characters left out]\n{end}",
        count - MESSAGE_SHOWN
    )
}

/// The conversation `messages` as plain text. One longer than `room`
/// characters loses its start.
fn told(messages: &[Value], room: usize) -> String {
    let mut text = String::new();
    for message in messages {
        let role = message["role"].as_str().unwrap_or_default();
        let said = message["content"].as_str().unwrap_or_default();
        if !said.trim().is_empty() {
            text.push_str(&format!(
                "[{role}]\n{}\n\n",
                clipped(said)
            ));
        }
        for call in
            message["tool_calls"].as_array().into_iter().flatten()
        {
            let function = &call["function"];
            text.push_str(&format!(
                "[{role} calls {}]\n{}\n\n",
                function["name"].as_str().unwrap_or_default(),
                clipped(
                    function["arguments"]
                        .as_str()
                        .unwrap_or_default()
                )
            ));
        }
    }
    let over = text.chars().count().saturating_sub(room);
    if over > 0 {
        text = format!(
            "[The start of the conversation is left out.]\n{}",
            text.chars().skip(over).collect::<String>()
        );
    }
    text
}

impl Agent {
    /// The tokens the model `model` holds at once, where that is
    /// known. `None` names the model nobody chose.
    pub(super) fn window(&self, model: Option<&str>) -> Option<u64> {
        let name = model.unwrap_or(&self.model);
        if let Some((seen, window)) = &self.seen
            && seen == name
        {
            return Some(*window);
        }
        let hamlet = self.hamlet.as_ref()?;
        Catalogue::load(hamlet.home()).get(name)?.context
    }

    /// True when the conversation is to be summarised before it is
    /// sent with `system` and `defs` to a model that holds `window`
    /// tokens.
    pub(super) fn crowded(
        &self,
        system: &Value,
        defs: &[Value],
        window: u64,
    ) -> bool {
        let size = |values: &[Value]| {
            budget::estimate(values, &[], &Value::Null)
        };
        let said = size(&self.messages);
        let sent =
            said + size(defs) + size(std::slice::from_ref(system));
        // Where the prompt itself is what fills the window, a
        // summary of the little that was said makes no room.
        said >= window / 4
            && (self.tight || sent as f64 >= FULL * window as f64)
    }

    /// Notes how much of its window the model's last call filled.
    pub(super) fn measure(
        &mut self,
        call: &CallRecord,
        window: Option<u64>,
    ) {
        let used = call.input_tokens.unwrap_or(0)
            + call.output_tokens.unwrap_or(0);
        self.tight = window.is_some_and(|window| {
            used as f64 >= FULL * window as f64
        });
    }

    /// The window a reply that said nothing ran out of, where that
    /// is why it was cut off. The model `model` is then known to
    /// hold that much.
    pub(super) fn out_of_room(
        &mut self,
        call: &CallRecord,
        model: Option<&str>,
        sent: u64,
    ) -> Option<u64> {
        let cut = call.finish_reason.as_deref() == Some("length")
            && call.output_tokens.unwrap_or(0) < NO_ROOM;
        if !cut {
            return None;
        }
        let window = call.input_tokens.unwrap_or(sent);
        let name = model.unwrap_or(&self.model).to_string();
        self.seen = Some((name, window));
        Some(window)
    }

    /// Puts the model's summary of the conversation in its place,
    /// for a model that holds `window` tokens. A message that has no
    /// answer yet is kept as it is. `payer` is charged.
    pub(super) async fn compact(
        &mut self,
        model: Option<&str>,
        payer: &str,
        window: u64,
        on: &mut Listener<'_>,
    ) -> Result<()> {
        let waiting = self
            .messages
            .last()
            .filter(|last| last["role"] == "user")
            .and_then(|last| last["content"].as_str())
            .map(String::from);
        let past =
            self.messages.len() - usize::from(waiting.is_some());
        if past == 0 {
            return Err(self.fail(
                "the message is longer than the model can hold",
            ));
        }
        let room = (window as f64 * SHOWN) as usize * TOKEN_CHARS;
        let shown = told(&self.messages[..past], room);
        let request = [
            json!({"role": "system", "content": SUMMARISER}),
            json!({"role": "user", "content": format!("{shown}{ASK}")}),
        ];
        let done = match self
            .provider
            .complete(&request, &[], model, &mut |_| {})
            .await
        {
            Ok(done) => done,
            Err(e) => {
                return Err(
                    self.fail(&format!("provider error: {e}"))
                );
            }
        };
        self.log.model_call(&done.call)?;
        if let Some(hamlet) = &self.hamlet {
            let estimate =
                budget::estimate(&request, &[], &done.message);
            hamlet.spend(payer, &self.task, &done.call, estimate)?;
        }
        on(Event::ModelCall(done.call));
        let summary = done.message["content"]
            .as_str()
            .unwrap_or_default()
            .trim();
        if summary.is_empty() {
            return Err(self.fail(
                "the conversation is longer than the model can \
                 hold, and the model could not summarise it. Start \
                 a new session, or choose a model that holds more",
            ));
        }
        let mut stands = format!("{OPENING}{summary}");
        if let Some(waiting) = waiting {
            stands.push_str(&format!("\n\n{WAITING}{waiting}"));
        }
        self.log.compacted(&stands)?;
        self.messages =
            vec![json!({"role": "user", "content": stands})];
        self.tight = false;
        self.note(NOTICE, on)
    }
}
