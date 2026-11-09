//! The tools every agent works with, whether or not its hamlet works
//! on a folder (C16): `read`, `write`, `edit`, a shell, `search` and
//! `find`.

mod diff;
mod environment;
mod files;
mod find;
mod parcel;
mod search;
mod shell;
pub(crate) mod truncate;
mod walk;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;

use hamlet_os::fs::path::{self, text};
use hamlet_protocol::client::{AttachedFile, ImageId};
use hamlet_protocol::runtime::{
    Diagnostic, DiagnosticSeverity, ToolSpec, ToolSpecReplay,
};
use hamlet_sandbox::scrub::scrub_value;
use hamlet_sandbox::{
    Env, FileError, Leftovers, Policy, Sandbox, SetupError, base_env,
    host_env,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub(crate) use self::parcel::{Sent, To};
pub(crate) use self::shell::{Dialect, Shells};
use crate::images::Limits;
use crate::society::refusal::Refusal;

const FILE_TOOLS: [&str; 5] =
    ["read", "write", "edit", "search", "find"];

/// The preview of a file the agent may not read as text.
pub(crate) const UNREADABLE: &str =
    "[a file that could not be read as text]";

/// The places an agent's tools work in: its lodge, or a worker's
/// scratch folder, and the folder its hamlet works on (C20).
#[derive(Clone, Debug)]
pub struct Place {
    /// The agent's lodge, or a worker's scratch folder.
    pub lodge: PathBuf,
    /// The folder its hamlet works on.
    pub folder: Option<PathBuf>,
    /// The realm's folder, which the tools never read.
    pub realm: PathBuf,
    /// What they read and never write: the library, the tools added
    /// by hand, and a worker's starter's lodge.
    pub shared: Vec<PathBuf>,
}

/// What a tool call ended with: the text pi's own tool would give,
/// and its remarks apart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    /// The output pi's own tool would give.
    pub text: String,
    pub error: bool,
    /// pi's details of the result.
    pub details: Option<Map<String, Value>>,
    /// Remarks on the result, in the order pi gives them.
    pub diagnostics: Vec<Diagnostic>,
    /// The images the result holds, kept in the realm's folder of
    /// them (C34).
    #[serde(default)]
    pub images: Vec<ImageId>,
}

/// The images the model of an agent takes, and where the realm
/// keeps them.
#[derive(Clone, Debug)]
pub(crate) struct Sight {
    pub folder: PathBuf,
    /// None when the model takes no images.
    pub limits: Option<Limits>,
}

impl Sight {
    /// A sight of no images.
    pub fn blind() -> Self {
        Self {
            folder: PathBuf::new(),
            limits: None,
        }
    }
}

impl Outcome {
    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
            details: None,
            diagnostics: Vec::new(),
            images: Vec::new(),
        }
    }

    pub(crate) fn failed(text: impl Into<String>) -> Self {
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

/// The names a hand tool may not take.
pub(crate) fn reserved_names() -> impl Iterator<Item = String> {
    specs(&Shells::local())
        .into_iter()
        .chain(crate::web::specs())
        .map(|tool| tool.name)
        .chain(Dialect::ALL.map(|dialect| dialect.tool().to_owned()))
}

/// The tools a model is offered, with `shells` among them.
pub fn specs(shells: &Shells) -> Vec<ToolSpec> {
    let spec =
        |name: &str, description: String, parameters, replay| {
            ToolSpec {
                name: name.into(),
                description,
                parameters: object(parameters),
                replay,
                retain: None,
            }
        };
    let string = |description: &str| json!({"type": "string", "description": description});
    let number = |description: &str| json!({"type": "number", "description": description});
    let mut specs = vec![
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
    ];
    specs.extend(shells.specs());
    specs.extend([
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
    ]);
    specs
}

fn object(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

/// An agent's tools: on this machine under the sandbox, or in a
/// remote workspace, which never falls back to this machine.
pub(crate) enum ToolSession {
    Local {
        place: Place,
        /// Whether commands reach the network.
        net: bool,
        sandbox: Sandbox,
        shells: Shells,
        leftovers: Leftovers,
        sight: Sight,
    },
    Remote {
        place: Place,
        session: crate::workspace::Session,
        shells: Shells,
        sight: Sight,
    },
}

impl ToolSession {
    pub fn local(
        place: Place,
        net: bool,
        sandbox: Sandbox,
        shells: Shells,
        leftovers: Leftovers,
    ) -> Self {
        Self::Local {
            place,
            net,
            sandbox,
            shells,
            leftovers,
            sight: Sight::blind(),
        }
    }

    /// The tools of `session`, whose library is read from `place`.
    pub fn remote(
        place: Place,
        session: crate::workspace::Session,
    ) -> Self {
        Self::Remote {
            place,
            session,
            shells: Shells::remote(),
            sight: Sight::blind(),
        }
    }

    /// The same tools, reading images as `seen` says.
    pub fn seeing(mut self, seen: Sight) -> Self {
        let (Self::Local { sight, .. } | Self::Remote { sight, .. }) =
            &mut self;
        *sight = seen;
        self
    }

    /// The text of the file a `write` to `path` would replace, as its
    /// preview shows it, or `None` where there is none. It is read as
    /// the agent may, so the chief sees nothing the agent could not
    /// read.
    pub async fn preview(
        &self,
        path: &str,
    ) -> Result<Option<String>, FileError> {
        match self {
            Self::Local {
                place,
                sandbox,
                leftovers,
                ..
            } => {
                let Ok(mut env) = environment::Local::open(
                    *sandbox,
                    place,
                    false,
                    leftovers.clone(),
                )
                .await
                else {
                    return Ok(Some(UNREADABLE.into()));
                };
                files::preview(&mut env, path).await
            }
            Self::Remote { session, .. } => {
                let mut env =
                    environment::Remote::new(session.clone());
                files::preview(&mut env, path).await
            }
        }
    }

    /// The files under `paths` as the agent may read them, without
    /// following links, saved into `to` when it is given. A remote
    /// workspace gives at most `MAX_FILE` bytes of a file at once.
    pub(crate) async fn parcel(
        &self,
        paths: &[String],
        to: Option<To<'_>>,
    ) -> anyhow::Result<Result<Sent, Refusal>> {
        match self {
            Self::Local {
                place,
                sandbox,
                leftovers,
                ..
            } => {
                let mut env = match environment::Local::open(
                    *sandbox,
                    place,
                    false,
                    leftovers.clone(),
                )
                .await
                {
                    Ok(env) => env,
                    Err(unopened) => {
                        return Ok(Err(Refusal::Usage {
                            words: unopened.to_string(),
                        }));
                    }
                };
                parcel::run(
                    &mut env,
                    paths,
                    AttachedFile::MOST_FILE,
                    to,
                )
                .await
            }
            Self::Remote { session, .. } => {
                let mut env =
                    environment::Remote::new(session.clone());
                parcel::run(
                    &mut env,
                    paths,
                    hamlet_workspace_wire::MAX_FILE as u64,
                    to,
                )
                .await
            }
        }
    }

    /// Whether an edit of `path` stays in the agent's own lodge,
    /// where the sandbox keeps it. A remote lodge is never taken as
    /// one.
    pub fn is_lodge_edit(&self, path: &str) -> bool {
        let Self::Local { place, .. } = self else {
            return false;
        };
        let lodge = text(&place.lodge);
        let folder = place.folder.as_deref().map(text);
        let there =
            |path: &String| std::path::Path::new(path).exists();
        let own = firsts(path, &lodge, folder.as_deref())
            .is_some_and(|[folder, lodge]| {
                !there(&folder) && there(&lodge)
            });
        let full = footing(path, &lodge, folder.as_deref(), own);
        hamlet_sandbox::is_within(
            std::path::Path::new(&full),
            &place.lodge,
        )
    }

    /// Runs `tool` with `arguments` and takes `secrets` out of all it
    /// gives. Dropping the future kills a command it runs.
    pub async fn run(
        &self,
        secrets: &[String],
        tool: &str,
        arguments: &Map<String, Value>,
    ) -> Outcome {
        let mut outcome = match self {
            Self::Local {
                place,
                net,
                sandbox,
                shells,
                leftovers,
                sight,
            } if is_built_in(shells, tool) => {
                let net = *net && shells.named(tool).is_some();
                match environment::Local::open(
                    *sandbox,
                    place,
                    net,
                    leftovers.clone(),
                )
                .await
                {
                    Ok(mut env) => {
                        reach_run(
                            &mut env, shells, place, sight, secrets,
                            tool, arguments,
                        )
                        .await
                    }
                    Err(unopened) => Outcome::thrown(
                        "tool_error",
                        unopened.to_string(),
                    ),
                }
            }
            Self::Local {
                place,
                net,
                sandbox,
                leftovers,
                ..
            } => {
                crate::work::added::run(
                    sandbox,
                    &place.realm,
                    *net,
                    leftovers,
                    secrets,
                    tool,
                    arguments,
                )
                .await
            }
            Self::Remote {
                place,
                session,
                shells,
                sight,
            } => {
                // The library is read once for all the call's
                // requests.
                let library = place.realm.join("library");
                match session.clone().with_library(library).await {
                    Err(failure) => {
                        Outcome::thrown("tool_error", failure.message)
                    }
                    Ok(session) if is_built_in(shells, tool) => {
                        let mut env =
                            environment::Remote::new(session);
                        reach_run(
                            &mut env, shells, place, sight, secrets,
                            tool, arguments,
                        )
                        .await
                    }
                    // A tool added by hand runs on this machine,
                    // which a remote workspace never falls back to.
                    Ok(_) => Outcome::failed(format!(
                        "The tool {tool} was added by hand and does \
                         not run in a remote workspace."
                    )),
                }
            }
        };
        scrub(secrets, &mut outcome);
        outcome
    }
}

fn is_built_in(shells: &Shells, tool: &str) -> bool {
    shells.named(tool).is_some() || FILE_TOOLS.contains(&tool)
}

/// The built-in tool `tool` on `env`.
async fn reach_run(
    env: &mut impl environment::Reach,
    shells: &Shells,
    place: &Place,
    sight: &Sight,
    secrets: &[String],
    tool: &str,
    arguments: &Map<String, Value>,
) -> Outcome {
    if let Some(shell) = shells.named(tool) {
        return shell::run(shell, env, place, secrets, arguments)
            .await;
    }
    match tool {
        "read" => {
            files::read(env, arguments, shells.dialect(), sight).await
        }
        "write" => files::write(env, arguments).await,
        "edit" => files::edit(env, arguments).await,
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
/// It works in the folder, or in the lodge when there is none; writes
/// the lodge and the folder; reads the shared folders; and never
/// reads the rest of the realm.
pub(crate) fn environment(
    sandbox: &Sandbox,
    place: &Place,
    net: bool,
) -> Result<Env, SetupError> {
    let mut policy = match &place.folder {
        None => {
            let mut env = base_env();
            env.insert("HOME".into(), text(&place.lodge));
            Policy {
                ro: place.shared.clone(),
                rw: vec![place.lodge.clone()],
                env,
                ..Policy::default()
            }
        }
        // The machine to read but the realm, and the chief's own
        // environment, so the project's tools are found.
        Some(folder) => Policy {
            ro: place.shared.clone(),
            rw: vec![folder.clone(), place.lodge.clone()],
            open: true,
            hidden: vec![place.realm.clone()],
            env: host_env(),
            ..Policy::default()
        },
    };
    policy.net = net;
    let cwd = place.folder.as_ref().unwrap_or(&place.lodge);
    Env::new(&text(cwd), policy, *sandbox)
}

/// The full path a tool's `path` names (C20). A full path stays as it
/// is. A relative one is the folder's, unless `own`: only the lodge
/// has its first part, as with `memory/`, `skills/` and `inbox/`.
/// With no folder it is the lodge's.
fn footing(
    path: &str,
    lodge: &str,
    folder: Option<&str>,
    own: bool,
) -> String {
    match folder {
        _ if path::is_absolute(path) => path::normalize(path),
        Some(folder) if !own => path::join(&[folder, path]),
        _ => path::join(&[lodge, path]),
    }
}

/// The folder's and the lodge's path of the first part of a relative
/// `path`, which decide where it is footed, while there is a folder.
fn firsts(
    path: &str,
    lodge: &str,
    folder: Option<&str>,
) -> Option<[String; 2]> {
    let folder = folder.filter(|_| !path::is_absolute(path))?;
    let first = path.split(['\\', '/']).next().unwrap_or("");
    Some([path::join(&[folder, first]), path::join(&[lodge, first])])
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

/// True for the characters pi's tools take as white space.
pub(crate) fn is_pi_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// A number as pi's tools write it.
pub(crate) fn number_words(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e21 {
        // Plus zero, as pi's tools write no minus zero.
        format!("{:.0}", value + 0.0)
    } else {
        value.to_string()
    }
}
