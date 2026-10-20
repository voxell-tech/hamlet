//! The tools every agent works with, whether or not its hamlet works
//! on a folder (C16): `read`, `write`, `edit`, `bash`, `search` and
//! `find`.

mod bash;
mod diff;
mod environment;
mod files;
mod find;
mod search;
mod truncate;
mod walk;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::harness::link::{
    Diagnostic, DiagnosticSeverity, ToolSpec, ToolSpecReplay,
};
use crate::sandbox::path::{self, text};
use crate::sandbox::{Env, Sandbox, closed_policy, open_policy};
use crate::scrub::scrub_value;

/// The places an agent's tools work in: its house, and the folder its
/// hamlet works on (C20).
#[derive(Clone, Debug)]
pub struct Place {
    /// The agent's house.
    pub house: PathBuf,
    /// The folder its hamlet works on.
    pub folder: Option<PathBuf>,
    /// Hamlet's home, which the tools never read.
    pub home: PathBuf,
    /// The tools added by hand and the library, which they read.
    pub shared: Vec<PathBuf>,
}

/// What a tool call ended with: the text pi's own tool would give,
/// and its remarks apart.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// The output pi's own tool would give.
    pub text: String,
    pub error: bool,
    /// pi's details of the result.
    pub details: Option<Map<String, Value>>,
    /// Remarks on the result, in the order pi gives them.
    pub diagnostics: Vec<Diagnostic>,
}

impl Outcome {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
            details: None,
            diagnostics: Vec::new(),
        }
    }

    fn failed(text: impl Into<String>) -> Self {
        Self {
            error: true,
            ..Self::text(text)
        }
    }

    /// A call that threw, as pi gives it: no text and one error.
    pub(crate) fn thrown(
        code: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            diagnostics: vec![diagnostic(
                DiagnosticSeverity::Error,
                code,
                message,
            )],
            ..Self::failed("")
        }
    }
}

pub(crate) fn diagnostic(
    severity: DiagnosticSeverity,
    code: &str,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        severity,
        code: code.into(),
        message: message.into(),
    }
}

/// The tools a model is offered.
pub fn specs() -> Vec<ToolSpec> {
    let spec =
        |name: &str, description: String, parameters, replay| {
            ToolSpec {
                name: name.into(),
                description,
                parameters: object(parameters),
                replay,
            }
        };
    let string = |description: &str| json!({"type": "string", "description": description});
    let number = |description: &str| json!({"type": "number", "description": description});
    vec![
        spec(
            "read",
            "Read the contents of a file. Supports text files and \
             images (jpg, png, gif, webp). Images are sent as \
             attachments. For text files, output is truncated to \
             2000 lines or 50KB (whichever is hit first). Use \
             offset/limit for large files. When you need the full \
             file, continue with offset until complete."
                .into(),
            json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": string(
                        "Path to the file to read (relative or absolute)"
                    ),
                    "offset": number(
                        "Line number to start reading from (1-indexed)"
                    ),
                    "limit": number("Maximum number of lines to read"),
                },
            }),
            ToolSpecReplay::Safe,
        ),
        spec(
            "write",
            "Write content to a file. Creates the file if it \
             doesn't exist, overwrites if it does. Automatically \
             creates parent directories."
                .into(),
            json!({
                "type": "object",
                "required": ["path", "content"],
                "properties": {
                    "path": string(
                        "Path to the file to write (relative or absolute)"
                    ),
                    "content": string("Content to write to the file"),
                },
            }),
            ToolSpecReplay::Unsafe,
        ),
        spec(
            "edit",
            "Edit a single file using exact text replacement. Every \
             edits[].oldText must match a unique, non-overlapping \
             region of the original file. If two changes affect the \
             same block or nearby lines, merge them into one edit \
             instead of emitting overlapping edits. Do not include \
             large unchanged regions just to connect distant \
             changes."
                .into(),
            json!({
                "type": "object",
                "required": ["path", "edits"],
                "properties": {
                    "path": string(
                        "Path to the file to edit (relative or absolute)"
                    ),
                    "edits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["oldText", "newText"],
                            "properties": {
                                "oldText": string(
                                    "Exact text for one targeted \
                                     replacement. It must be unique in \
                                     the original file and must not \
                                     overlap with any other \
                                     edits[].oldText in the same call."
                                ),
                                "newText": string(
                                    "Replacement text for this targeted \
                                     edit."
                                ),
                            },
                        },
                        "description": "One or more targeted \
                                        replacements. Each edit is \
                                        matched against the original \
                                        file, not incrementally. Do not \
                                        include overlapping or nested \
                                        edits. If two changes touch the \
                                        same block or nearby lines, \
                                        merge them into one edit \
                                        instead.",
                    },
                },
            }),
            ToolSpecReplay::Unsafe,
        ),
        spec(
            "bash",
            bash::description(),
            json!({
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": string("Bash command to execute"),
                    "timeout": number(&format!(
                        "Timeout in seconds, at most {}",
                        bash::WALL_SECS
                    )),
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
            "search",
            search::DESCRIPTION.into(),
            json!({
                "type": "object",
                "required": ["pattern"],
                "properties": {
                    "pattern": string(
                        "A regular expression, such as `fn main` or \
                         `TODO|FIXME`."
                    ),
                    "path": string(
                        "The folder or file to search. Leave out for the \
                         folder commands run in."
                    ),
                    "name": string(
                        "Only files whose name matches, such as `*.rs`."
                    ),
                    "ignore_case": {"type": "boolean"},
                },
            }),
            ToolSpecReplay::Safe,
        ),
        spec(
            "find",
            find::DESCRIPTION.into(),
            json!({
                "type": "object",
                "properties": {
                    "path": string(
                        "The folder. Leave out for the folder commands \
                         run in."
                    ),
                    "name": string(
                        "A file name with `*` and `?`, such as `*.toml` \
                         or `mod.rs`. With a `/` in it, it is matched \
                         against the path under the folder."
                    ),
                },
            }),
            ToolSpecReplay::Safe,
        ),
    ]
}

fn object(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

/// Runs `tool` with `arguments` for an agent at `place`, reaching the
/// network only with `net`, and takes `secrets` out of all it gives.
/// Dropping the future kills a command it runs.
pub async fn run(
    sandbox: &Sandbox,
    place: &Place,
    net: bool,
    secrets: &[String],
    tool: &str,
    arguments: &Map<String, Value>,
) -> Outcome {
    let mut outcome = match tool {
        "bash" => {
            bash::run(sandbox, place, net, secrets, arguments).await
        }
        "read" | "write" | "edit" | "search" | "find" => {
            let (sandbox, place) = (*sandbox, place.clone());
            let (tool, arguments) =
                (tool.to_owned(), arguments.clone());
            tokio::task::spawn_blocking(move || {
                let env = match environment(&sandbox, &place, false) {
                    Ok(env) => env,
                    Err(message) => {
                        return Outcome::thrown(
                            "tool_error",
                            message,
                        );
                    }
                };
                futures::executor::block_on(file_run(
                    &environment::Files::Local(&env),
                    &place,
                    &tool,
                    &arguments,
                ))
            })
            .await
            .unwrap_or_else(|error| {
                Outcome::thrown("tool_error", error.to_string())
            })
        }
        _ => {
            crate::hand::run(
                sandbox,
                &place.home,
                net,
                secrets,
                tool,
                arguments,
            )
            .await
        }
    };
    scrub(secrets, &mut outcome);
    outcome
}

pub(crate) async fn run_remote(
    remote: &crate::workspace::Remote,
    place: &Place,
    secrets: &[String],
    tool: &str,
    arguments: &Map<String, Value>,
) -> Outcome {
    let mut outcome = match tool {
        "bash" => {
            bash::remote(remote, place, secrets, arguments).await
        }
        "read" | "write" | "edit" | "search" | "find" => {
            file_run(
                &environment::Files::Remote(remote),
                place,
                tool,
                arguments,
            )
            .await
        }
        // Hand tools run on the host, which a remote workspace never
        // falls back to.
        _ => Outcome::failed(format!(
            "The tool {tool} was added by hand and does not run in \
             a remote workspace."
        )),
    };
    scrub(secrets, &mut outcome);
    outcome
}

async fn file_run(
    env: &environment::Files<'_>,
    place: &Place,
    tool: &str,
    arguments: &Map<String, Value>,
) -> Outcome {
    match tool {
        "read" => files::read(env, place, arguments).await,
        "write" => files::write(env, place, arguments).await,
        "edit" => files::edit(env, place, arguments).await,
        "search" => search::run(env, arguments).await,
        _ => find::run(env, arguments).await,
    }
}

/// `outcome` without `secrets`, as a hook takes them out of every
/// tool's result (C32).
fn scrub(secrets: &[String], outcome: &mut Outcome) {
    let mut value = json!({
        "text": outcome.text,
        "details": outcome.details,
        "diagnostics": outcome.diagnostics.iter().map(|each| {
            json!({"code": each.code, "message": each.message})
        }).collect::<Vec<_>>(),
    });
    scrub_value(secrets, &mut value);
    let words =
        |value: &Value| value.as_str().unwrap_or("").to_owned();
    outcome.text = words(&value["text"]);
    outcome.details = value["details"].as_object().cloned();
    for (each, scrubbed) in outcome
        .diagnostics
        .iter_mut()
        .zip(value["diagnostics"].as_array().into_iter().flatten())
    {
        each.code = words(&scrubbed["code"]);
        each.message = words(&scrubbed["message"]);
    }
}

/// The environment of an agent's tools, under the sandbox (S12, C18).
/// It works in the folder, or in the house when there is none; writes
/// the house and the folder; reads the shared folders; and never
/// reads the rest of the home.
pub(crate) fn environment(
    sandbox: &Sandbox,
    place: &Place,
    net: bool,
) -> Result<Env, String> {
    let mut policy = match &place.folder {
        None => closed_policy(&place.house, &place.shared),
        Some(folder) => open_policy(
            folder,
            &place.house,
            &place.home,
            &place.shared,
        ),
    };
    policy.net = net;
    let cwd = place.folder.as_ref().unwrap_or(&place.house);
    Env::new(&text(cwd), policy, *sandbox)
}

/// The full path a tool's `path` names (C20). A full path stays as it
/// is. A relative one is the folder's, unless only the house has its
/// first part, as with `NOTES.md`, `skills/` and `inbox/`; with no
/// folder it is the house's.
pub fn footing(path: &str, place: &Place) -> Result<PathBuf, String> {
    Ok(PathBuf::from(footed(path, place)))
}

fn footed(path: &str, place: &Place) -> String {
    let house = text(&place.house);
    if path::is_absolute(path) {
        return path::normalize(path);
    }
    let Some(folder) = &place.folder else {
        return path::join(&[house.as_str(), path]);
    };
    let folder = text(folder);
    let first = path.split(['\\', '/']).next().unwrap_or("");
    let there = |root: &str| {
        std::path::Path::new(&path::join(&[root, first])).exists()
    };
    let own = !there(&folder) && there(&house);
    path::join(&[if own { &house } else { &folder }, path])
}

/// The arguments of a call, refused as pi refuses ones that break the
/// tool's parameters.
struct Arguments<'a> {
    tool: &'a str,
    map: &'a Map<String, Value>,
}

impl<'a> Arguments<'a> {
    fn new(tool: &'a str, map: &'a Map<String, Value>) -> Self {
        Self { tool, map }
    }

    fn invalid(&self, name: &str, wanted: &str) -> Outcome {
        Outcome::thrown(
            "invalid_arguments",
            format!(
                "Validation failed for tool \"{}\":\n  - {name}: \
                 must be {wanted}\n\nReceived arguments:\n{}",
                self.tool,
                serde_json::to_string_pretty(self.map)
                    .unwrap_or_default()
            ),
        )
    }

    fn get(&self, name: &str) -> Option<&'a Value> {
        self.map.get(name).filter(|value| !value.is_null())
    }

    fn string(&self, name: &str) -> Result<&'a str, Outcome> {
        self.get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| self.invalid(name, "string"))
    }

    fn optional_string(
        &self,
        name: &str,
    ) -> Result<Option<&'a str>, Outcome> {
        self.get(name)
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| self.invalid(name, "string"))
            })
            .transpose()
    }

    fn number(&self, name: &str) -> Result<Option<f64>, Outcome> {
        self.get(name)
            .map(|value| {
                value
                    .as_f64()
                    .ok_or_else(|| self.invalid(name, "number"))
            })
            .transpose()
    }

    fn boolean(&self, name: &str) -> Result<Option<bool>, Outcome> {
        self.get(name)
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| self.invalid(name, "boolean"))
            })
            .transpose()
    }
}

/// A number as JavaScript writes it.
pub(crate) fn number_words(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e21 {
        // Plus zero, as JavaScript writes no minus zero.
        format!("{:.0}", value + 0.0)
    } else {
        value.to_string()
    }
}
