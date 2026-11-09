//! The tools agents use the web with. Each acts on the agent's own
//! page in a Web tab of the chief's window: the server asks the
//! window the chief used last that draws Web panels, and waits for
//! its answer (ADR 0015).

use std::collections::BTreeMap;
use std::time::Duration;

use hamlet_protocol::client::{
    self as protocol, RequestId, ServerMessage, WebAct, WebAnswer,
    WebAsk, is_on_this_machine, web_address,
};
use hamlet_protocol::runtime::{ToolSpec, ToolSpecReplay};
use hamlet_sandbox::scrub::scrub_text;
use hamlet_store::ids::AgentId;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::calls::spec;
use crate::core::Core;
use crate::work::tools::Outcome;
use crate::work::tools::truncate::{
    MAX_BYTES, character_end, format_size,
};

/// How long a window may take to answer, past the act's own wait.
const ANSWER_WITHIN: Duration = Duration::from_secs(30);

/// What an agent is told when no window can draw its page.
const NO_WINDOW: &str = "No window with Web panels is open, so \
                         there is nowhere to load a page. Ask the \
                         chief to open one.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Open,
    Read,
    Console,
}

impl Tool {
    const ALL: [Self; 3] = [Self::Open, Self::Read, Self::Console];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Open => "web_open",
            Self::Read => "web_read",
            Self::Console => "web_console",
        }
    }

    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }
}

pub(crate) fn specs() -> Vec<ToolSpec> {
    vec![
        spec(
        Tool::Open.name(),
        "Load a page in your own Web tab, which the chief can watch, \
         and wait for it to load. Use it to see a site you serve, \
         such as http://localhost:5173, or a file you made. A site \
         on another machine needs the network. Your tab keeps its \
         own session, apart from the chief's, and closes when you \
         end.",
        json!({
            "type": "object",
            "required": ["address"],
            "properties": {
                "address": {
                    "type": "string",
                    "description":
                        "The page's address. localhost:5173 and \
                         example.com are read as http and https.",
                },
            },
        }),
        ToolSpecReplay::Safe,
        ),
        spec(
            Tool::Read.name(),
            "Read the page in your Web tab as the browser built it, \
             after its scripts ran: its title, address and text, then \
             its links, buttons and fields, each with a number that \
             web_click and web_type take. The numbers last until the \
             next read. With html, its HTML instead.",
            json!({
                "type": "object",
                "properties": {
                    "html": {
                        "type": "boolean",
                        "description":
                            "Give the page's HTML rather than its text \
                             and elements. Defaults to false.",
                    },
                },
            }),
            ToolSpecReplay::Safe,
        ),
        spec(
            Tool::Console.name(),
            "Show what the page in your Web tab wrote to its console, \
             the errors that went uncaught in it and the loads that \
             failed, since you last looked: the first place to see \
             whether a site you built works.",
            json!({"type": "object", "properties": {}}),
            ToolSpecReplay::Safe,
        ),
    ]
}

/// Whether a call of `tool` with `arguments` reaches beyond this
/// machine.
pub(crate) fn reaches_network(tool: Tool, arguments: &Value) -> bool {
    match tool {
        Tool::Open => arguments
            .get("address")
            .and_then(Value::as_str)
            .and_then(web_address)
            .is_some_and(|address| !is_on_this_machine(&address)),
        Tool::Read | Tool::Console => false,
    }
}

/// The windows that draw Web panels, and the asks they owe answers
/// to.
#[derive(Default)]
pub(crate) struct Windows {
    /// Each window's turn in the front, the last the highest.
    fronts: BTreeMap<u64, u64>,
    turn: u64,
    asked: BTreeMap<RequestId, Asked>,
}

struct Asked {
    client: u64,
    answer: oneshot::Sender<WebAnswer>,
}

impl Windows {
    /// `client` draws Web panels and came to the front.
    pub(crate) fn fronted(&mut self, client: u64) {
        self.turn += 1;
        self.fronts.insert(client, self.turn);
    }

    /// The window the chief used last of those that draw Web panels.
    fn front(&self) -> Option<u64> {
        self.fronts
            .iter()
            .max_by_key(|(_, turn)| **turn)
            .map(|(client, _)| *client)
    }

    /// Hands `answer` to the call waiting for it, if `client` was
    /// asked.
    pub(crate) fn answered(
        &mut self,
        client: u64,
        answer: WebAnswer,
    ) {
        if self
            .asked
            .get(&answer.to)
            .is_some_and(|asked| asked.client == client)
            && let Some(asked) = self.asked.remove(&answer.to)
        {
            let _ = asked.answer.send(answer);
        }
    }

    /// `client` left: its asks fail.
    pub(crate) fn gone(&mut self, client: u64) {
        self.fronts.remove(&client);
        self.asked.retain(|_, asked| asked.client != client);
    }
}

/// `text` cut to the most a tool shows, at a character.
fn cut(text: &str) -> String {
    if text.len() <= MAX_BYTES {
        return text.to_owned();
    }
    let end = character_end(text.as_bytes(), MAX_BYTES);
    format!(
        "{}\n\n[cut at {} of {}]",
        &text[..end],
        format_size(end),
        format_size(text.len())
    )
}

/// The act a call of `tool` with `arguments` asks for, or the words
/// of one that cannot be asked.
fn act_of(
    tool: Tool,
    arguments: &BTreeMap<String, Value>,
) -> Result<WebAct, String> {
    match tool {
        Tool::Open => {
            let typed = arguments
                .get("address")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let address = web_address(typed)
                .filter(|address| {
                    address.len() <= protocol::WEB_ADDRESS_MOST
                })
                .ok_or_else(|| {
                    format!("`{typed}` is not an address to load.")
                })?;
            Ok(WebAct::Open { address })
        }
        Tool::Console => Ok(WebAct::Console {}),
        Tool::Read => Ok(WebAct::Read {
            html: arguments
                .get("html")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
    }
}

/// Asks the window in front to carry out `tool` with `arguments` on
/// `agent`'s page, its page let onto other machines with `net`; what
/// is left waits for the answer.
pub(crate) fn call(
    core: &mut Core,
    agent: AgentId,
    tool: Tool,
    arguments: &BTreeMap<String, Value>,
    net: bool,
) -> anyhow::Result<impl Future<Output = Outcome> + Send + 'static> {
    let secrets = core.credentials.secrets();
    let asked = match act_of(tool, arguments) {
        Err(words) => Err(words),
        Ok(act) => match core.clients.web.front() {
            None => Err(NO_WINDOW.to_owned()),
            Some(client) => {
                let id = RequestId::new();
                let (answer, waiting) = oneshot::channel();
                core.clients
                    .web
                    .asked
                    .insert(id.clone(), Asked { client, answer });
                let ask = WebAsk {
                    id,
                    agent: protocol::AgentId(u64::try_from(agent.0)?),
                    net,
                    act,
                };
                core.clients.send(
                    client,
                    &core.credentials,
                    ServerMessage::Web(ask),
                )?;
                Ok(waiting)
            }
        },
    };
    Ok(async move {
        let outcome = match asked {
            Err(words) => Outcome::failed(words),
            Ok(waiting) => {
                match tokio::time::timeout(ANSWER_WITHIN, waiting)
                    .await
                {
                    Ok(Ok(answer)) if answer.failed => {
                        Outcome::failed(answer.text)
                    }
                    Ok(Ok(answer)) => {
                        Outcome::text(cut(&answer.text))
                    }
                    Ok(Err(_)) => Outcome::failed(
                        "The window closed before it answered.",
                    ),
                    Err(_) => Outcome::failed(format!(
                        "The window did not answer within {} s.",
                        ANSWER_WITHIN.as_secs()
                    )),
                }
            }
        };
        let mut outcome = outcome;
        outcome.text = scrub_text(&secrets, &outcome.text);
        outcome
    })
}
