//! The tools agents open, type into, read and close terminals with.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hamlet_os::process::pty::Size;
use hamlet_protocol::client::{AgentRef, TerminalView};
use hamlet_protocol::runtime::{ToolSpec, ToolSpecReplay};
use serde_json::{Value, json};

use super::{Screen, lock, shown};
use crate::record;
use crate::service::Service;
use crate::tools::Outcome;

/// Seconds a write or a read may wait for the output to settle.
const MOST_WAIT: f64 = 30.0;

/// How long output must stay still to count as settled.
const STILL: Duration = Duration::from_millis(400);

/// The most of a screen an agent is shown, in bytes.
const MOST_SHOWN: usize = 40_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Open,
    Write,
    Read,
    Close,
}

impl Tool {
    const ALL: [Self; 4] =
        [Self::Open, Self::Write, Self::Read, Self::Close];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Open => "terminal_open",
            Self::Write => "terminal_write",
            Self::Read => "terminal_read",
            Self::Close => "terminal_close",
        }
    }

    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }
}

/// The keys `terminal_write` names, and the bytes each sends.
const KEYS: [(&str, &str); 10] = [
    ("enter", "\r"),
    ("ctrl-c", "\x03"),
    ("ctrl-d", "\x04"),
    ("ctrl-z", "\x1a"),
    ("escape", "\x1b"),
    ("tab", "\t"),
    ("up", "\x1b[A"),
    ("down", "\x1b[B"),
    ("right", "\x1b[C"),
    ("left", "\x1b[D"),
];

pub(crate) fn specs() -> Vec<ToolSpec> {
    let spec =
        |tool: Tool, description: &str, parameters: Value, replay| {
            ToolSpec {
                name: tool.name().into(),
                description: description.into(),
                parameters: match parameters {
                    Value::Object(map) => map.into_iter().collect(),
                    _ => BTreeMap::new(),
                },
                replay,
                retain: None,
            }
        };
    let terminal = json!({
        "type": "string",
        "description":
            "The terminal's id as terminal_open gave it, or its name.",
    });
    let wait = json!({
        "type": "number",
        "description": format!(
            "Seconds to wait for the output to settle, at most \
             {MOST_WAIT}. Defaults to 2."
        ),
    });
    vec![
        spec(
            Tool::Open,
            "Open an interactive bash terminal in the folder your \
             commands run in, under the same sandbox. It keeps \
             running between calls, so use it for servers, \
             watchers, REPLs and programs that ask for input; use \
             bash for one-off commands. The chief sees it in the \
             Terminal panel and may type into it too. Returns its \
             id and screen.",
            json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "A short name the chief sees.",
                    },
                    "network": {
                        "type": "boolean",
                        "description":
                            "True when what you will run needs the \
                             network.",
                    },
                },
            }),
            ToolSpecReplay::Unsafe,
        ),
        spec(
            Tool::Write,
            "Type into a terminal, then show its screen once the \
             output settles. `text` is typed as is, and Enter \
             pressed after it unless `enter` is false. `key` \
             presses one key after the text.",
            json!({
                "type": "object",
                "required": ["terminal"],
                "properties": {
                    "terminal": terminal,
                    "text": {"type": "string"},
                    "enter": {
                        "type": "boolean",
                        "description":
                            "Press Enter after the text. Defaults to \
                             true when there is text.",
                    },
                    "key": {
                        "type": "string",
                        "enum": KEYS.map(|(name, _)| name),
                    },
                    "wait": wait,
                    "network": {
                        "type": "boolean",
                        "description":
                            "True when the command needs the network.",
                    },
                },
            }),
            ToolSpecReplay::Unsafe,
        ),
        spec(
            Tool::Read,
            "Show a terminal's screen, with lines above it from its \
             scrollback.",
            json!({
                "type": "object",
                "required": ["terminal"],
                "properties": {
                    "terminal": terminal,
                    "scrollback": {
                        "type": "number",
                        "description":
                            "Lines above the screen to show too, at \
                             most 2000.",
                    },
                    "wait": wait,
                },
            }),
            ToolSpecReplay::Safe,
        ),
        spec(
            Tool::Close,
            "Close a terminal and end what runs in it.",
            json!({
                "type": "object",
                "required": ["terminal"],
                "properties": {"terminal": terminal},
            }),
            ToolSpecReplay::Unsafe,
        ),
    ]
}

/// The bytes a `terminal_write` with `arguments` types.
fn typed(
    arguments: &BTreeMap<String, Value>,
) -> Result<Vec<u8>, String> {
    let text = arguments
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let enter = arguments
        .get("enter")
        .and_then(Value::as_bool)
        .unwrap_or(!text.is_empty());
    let mut bytes =
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes();
    if enter {
        bytes.push(b'\r');
    }
    if let Some(key) = arguments.get("key").and_then(Value::as_str) {
        let Some((_, sent)) =
            KEYS.iter().find(|(name, _)| *name == key)
        else {
            return Err(format!("There is no key named `{key}`."));
        };
        bytes.extend_from_slice(sent.as_bytes());
    }
    if bytes.is_empty() {
        return Err("Give text to type or a key to press.".into());
    }
    Ok(bytes)
}

/// What a `terminal_write` types, as a chief asked about it reads it.
pub(crate) fn command(arguments: &Value) -> String {
    let text = arguments
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match arguments.get("key").and_then(Value::as_str) {
        Some(key) if text.is_empty() => format!("[{key}]"),
        Some(key) => format!("{text} [{key}]"),
        None => text.into(),
    }
}

fn seconds(arguments: &BTreeMap<String, Value>) -> Duration {
    let wait = arguments
        .get("wait")
        .and_then(Value::as_f64)
        .filter(|wait| wait.is_finite())
        .unwrap_or(2.0);
    Duration::from_secs_f64(wait.clamp(0.0, MOST_WAIT))
}

/// Carries out `tool` with `arguments` for `agent`, reaching the
/// network with `net`; what is left waits for the output.
pub(crate) fn call(
    service: &mut Service,
    agent: &record::AgentRef,
    tool: Tool,
    arguments: &BTreeMap<String, Value>,
    net: bool,
) -> anyhow::Result<impl Future<Output = Outcome> + Send + 'static> {
    let secrets = service.credentials.secrets();
    let failed = |text: String| Work::Done(Outcome::failed(text));
    let me = AgentRef {
        hamlet: agent.hamlet.clone(),
        number: agent.number,
    };
    let terminals = &service.terminals;
    let terminal = arguments
        .get("terminal")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let found = terminals.of_hamlet(terminal, &agent.hamlet);
    let work = match (tool, found) {
        (Tool::Open, _) => {
            let name = arguments
                .get("name")
                .and_then(Value::as_str)
                .map(String::from);
            let size = Size {
                rows: 30,
                cols: 120,
            };
            let hamlet = Some(agent.hamlet.as_str());
            match super::open(
                service,
                hamlet,
                Some(&me),
                name,
                size,
                net,
            )? {
                Ok(id) => Work::Settle {
                    found: service
                        .terminals
                        .of_hamlet(&id, &agent.hamlet),
                    wait: Duration::from_secs(2),
                    above: 0,
                    since: 0,
                },
                Err(refusal) => failed(refusal.to_string()),
            }
        }
        (_, None) => failed(format!(
            "There is no terminal `{terminal}` in your hamlet."
        )),
        (Tool::Write, Some(found)) => match typed(arguments) {
            Err(words) => failed(words),
            Ok(bytes) => {
                let since = lock(&found.1).seq();
                if terminals.type_in(&found.0.id, bytes) {
                    Work::Settle {
                        found: Some(found),
                        wait: seconds(arguments),
                        above: 0,
                        since,
                    }
                } else {
                    failed(format!(
                        "The terminal `{terminal}` takes no input: \
                         its shell ended, or it shows runs."
                    ))
                }
            }
        },
        (Tool::Read, Some(found)) => {
            let above = arguments
                .get("scrollback")
                .and_then(Value::as_f64)
                .map_or(0, |lines| {
                    lines.clamp(0.0, super::SCROLLBACK as f64)
                        as usize
                });
            let since = lock(&found.1).seq();
            let wait = if arguments.contains_key("wait") {
                seconds(arguments)
            } else {
                Duration::ZERO
            };
            Work::Settle {
                found: Some(found),
                wait,
                above,
                since,
            }
        }
        (Tool::Close, Some((view, _))) => {
            service.terminals.close(&view.id);
            Work::Done(Outcome::text(format!(
                "Closed the terminal `{terminal}`."
            )))
        }
    };
    Ok(async move {
        let mut outcome = work.run().await;
        outcome.text =
            crate::scrub::scrub_text(&secrets, &outcome.text);
        outcome
    })
}

enum Work {
    Done(Outcome),
    /// Waits up to `wait` for the output after print `since` to
    /// settle, then shows the screen.
    Settle {
        found: Option<(TerminalView, Arc<Mutex<Screen>>)>,
        wait: Duration,
        above: usize,
        since: u64,
    },
}

impl Work {
    async fn run(self) -> Outcome {
        let (view, screen, wait, above, since) = match self {
            Self::Done(outcome) => return outcome,
            Self::Settle { found: None, .. } => {
                return Outcome::failed("The terminal closed.");
            }
            Self::Settle {
                found: Some((view, screen)),
                wait,
                above,
                since,
            } => (view, screen, wait, above, since),
        };
        let now = tokio::time::Instant::now;
        let deadline = now() + wait;
        let mut last = since;
        let mut moved = now();
        while now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let (seq, ended) = {
                let screen = lock(&screen);
                (screen.seq(), screen.ended().is_some())
            };
            if seq != last {
                last = seq;
                moved = now();
            } else if ended
                || (seq != since && moved.elapsed() >= STILL)
            {
                break;
            }
        }
        let text = shown(&view, &screen, above);
        if text.len() <= MOST_SHOWN {
            return Outcome::text(text);
        }
        let mut cut = text.len() - MOST_SHOWN;
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        Outcome::text(format!(
            "[earlier lines cut]\n{}",
            &text[cut..]
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed_of(value: Value) -> Result<Vec<u8>, String> {
        let Value::Object(map) = value else {
            unreachable!("the arguments are an object");
        };
        typed(&map.into_iter().collect())
    }

    #[test]
    fn a_write_types_its_lines_with_enter_and_its_key_after_them() {
        assert_eq!(
            typed_of(json!({"text": "ls\nls -a"})).unwrap(),
            b"ls\rls -a\r"
        );
        assert_eq!(
            typed_of(json!({"text": "q", "enter": false})).unwrap(),
            b"q"
        );
        assert_eq!(
            typed_of(json!({"key": "ctrl-c"})).unwrap(),
            b"\x03"
        );
        assert_eq!(
            typed_of(json!({"text": "y", "key": "tab"})).unwrap(),
            b"y\r\t"
        );
        assert!(typed_of(json!({})).is_err());
        assert!(typed_of(json!({"key": "f13"})).is_err());
    }
}
