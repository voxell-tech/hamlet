//! The shell tools, one per shell language. Each is pi's `bash` with
//! a `network` flag, a time limit a model may only shorten, and hints
//! where the sandbox refused a write (C16, C19). Every credential is
//! taken out of its output before any of it is kept (C32).

#[cfg(any(windows, test))]
mod find;

use std::time::Duration;

use hamlet_protocol::runtime::{
    DiagnosticSeverity, ToolSpec, ToolSpecReplay, ToolSpecRetain,
};
use hamlet_workspace_wire::Code;
use serde_json::{Map, Value, json};

use super::environment::Reach;
use super::truncate::{MAX_BYTES, MAX_LINES, Tail};
use super::{Arguments, Outcome, Place, diagnostic, number_words};
use crate::sandbox::{ExecError, FileKind};
use crate::scrub::scrub_text;

/// Seconds a command may run, which a model can ask to shorten and
/// never to lengthen.
const WALL_SECS: u64 = 120;

/// Words a sandbox refuses a write with: Seatbelt's, then
/// bubblewrap's.
const REFUSED: [&str; 3] = [
    "Operation not permitted",
    "Read-only file system",
    "Permission denied",
];

/// Said after a command the sandbox stopped, while the hamlet works
/// on no folder.
const OUTSIDE: &str = "\n[The sandbox lets a command write only \
                       inside the workspace, which is its working \
                       directory. Use a relative path there, not \
                       /tmp or another folder.]";

/// Said after a command the sandbox stopped, while the hamlet works
/// on a folder.
const OUTSIDE_PROJECT: &str = "\n[The sandbox lets a command write \
                               only in the project folder and in \
                               your own workspace, and keeps \
                               Hamlet's own folder closed to it.]";

/// The language a shell tool takes its commands in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dialect {
    Bash,
    PowerShell,
}

impl Dialect {
    pub(crate) const ALL: [Self; 2] = [Self::Bash, Self::PowerShell];

    /// The name of the tool a model calls.
    pub(crate) fn tool(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
        }
    }

    pub(crate) fn of_tool(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|dialect| dialect.tool() == name)
    }

    /// How the language is named at the start of a sentence.
    fn title(self) -> &'static str {
        match self {
            Self::Bash => "Bash",
            Self::PowerShell => "PowerShell",
        }
    }

    /// Words that reach the network in this dialect, besides the
    /// ones every dialect shares, in lower case.
    pub(crate) fn network_words(self) -> &'static [&'static str] {
        match self {
            Self::Bash => &["rsync", "nc", "brew install"],
            Self::PowerShell => &[
                "invoke-webrequest",
                "iwr",
                "invoke-restmethod",
                "irm",
                "start-bitstransfer",
                "winget install",
                "install-module",
            ],
        }
    }

    /// The command a model is told to run to read line `line` of
    /// `path` from byte `from`, counted from one.
    pub(crate) fn line_hint(
        self,
        path: &str,
        line: f64,
        from: usize,
    ) -> String {
        match self {
            Self::Bash => format!(
                "Use bash: sed -n '{}p' {path} | tail -c +{from}",
                number_words(line)
            ),
            Self::PowerShell => format!(
                "Use PowerShell: $b = \
                 [Text.Encoding]::UTF8.GetBytes(@(Get-Content \
                 -Encoding UTF8 -LiteralPath '{path}')[{}]); \
                 [Text.Encoding]::UTF8.GetString($b, {skip}, \
                 $b.Length - {skip})",
                number_words(line - 1.0),
                skip = from - 1
            ),
        }
    }
}

/// A shell a workspace offers.
#[derive(Clone, Debug)]
pub(crate) struct Shell {
    dialect: Dialect,
    program: String,
}

impl Shell {
    pub(crate) fn spec(&self) -> ToolSpec {
        let tool = self.dialect.tool();
        let parameters = json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": {
                    "type": "string",
                    "description": format!(
                        "{} command to execute",
                        self.dialect.title()
                    ),
                },
                "timeout": {
                    "type": "number",
                    "description": format!(
                        "Timeout in seconds, at most {WALL_SECS}"
                    ),
                },
                "network": {
                    "type": "boolean",
                    "description":
                        "True when the command needs the network.",
                },
            },
        });
        ToolSpec {
            name: tool.into(),
            description: format!(
                "Execute a {tool} command in the current working \
                 directory. Returns combined stdout and stderr. \
                 Output is truncated to last {MAX_LINES} lines or \
                 {}KB (whichever is hit first). If truncated, full \
                 output is saved to a temp file. Optionally provide \
                 a timeout in seconds. It stops after {WALL_SECS} \
                 seconds, or sooner with `timeout`.",
                MAX_BYTES / 1024
            ),
            parameters: match parameters {
                Value::Object(map) => map.into_iter().collect(),
                _ => Default::default(),
            },
            replay: ToolSpecReplay::Unsafe,
            retain: Some(ToolSpecRetain::Tail),
        }
    }

    /// The argv that runs `script`.
    fn invocation(&self, script: &str) -> Vec<String> {
        vec![self.program.clone(), "-c".into(), script.into()]
    }
}

/// The shells a workspace offers.
#[derive(Clone, Debug)]
pub(crate) struct Shells(Vec<Shell>);

impl Shells {
    /// This machine's: bash, found on the sandbox's `PATH`.
    #[cfg(not(windows))]
    pub(crate) fn local() -> Self {
        Self(vec![Shell {
            dialect: Dialect::Bash,
            program: "bash".into(),
        }])
    }

    /// This machine's: Git Bash, where it is installed.
    #[cfg(windows)]
    pub(crate) fn local() -> Self {
        Self(
            find::git_bash(&find::Machine)
                .map(|program| Shell {
                    dialect: Dialect::Bash,
                    program,
                })
                .into_iter()
                .collect(),
        )
    }

    /// A remote workspace's: bash at its fixed path.
    pub(crate) fn remote() -> Self {
        Self(vec![Shell {
            dialect: Dialect::Bash,
            program: "/bin/bash".into(),
        }])
    }

    pub(crate) fn specs(&self) -> impl Iterator<Item = ToolSpec> {
        self.0.iter().map(Shell::spec)
    }

    /// The offered shell whose tool is `name`.
    pub(super) fn named(&self, name: &str) -> Option<&Shell> {
        let dialect = Dialect::of_tool(name)?;
        self.0.iter().find(|shell| shell.dialect == dialect)
    }

    /// The dialect of the first shell offered.
    pub(super) fn dialect(&self) -> Option<Dialect> {
        self.0.first().map(|shell| shell.dialect)
    }
}

/// Whether a call of `tool` may not run again once it began.
pub(crate) fn runs_once(tool: &str) -> bool {
    Dialect::of_tool(tool).is_some()
        || super::specs(&Shells::local()).iter().any(|spec| {
            spec.name == tool && spec.replay == ToolSpecReplay::Unsafe
        })
}

/// pi's `timeout` in seconds, at most [`WALL_SECS`]; none when it is
/// not a finite number above zero.
fn seconds(timeout: Option<f64>) -> Option<f64> {
    let wall =
        timeout.unwrap_or(WALL_SECS as f64).min(WALL_SECS as f64);
    (wall.is_finite() && wall > 0.0).then_some(wall)
}

pub(super) async fn run(
    shell: &Shell,
    env: &mut impl Reach,
    place: &Place,
    secrets: &[String],
    arguments: &Map<String, Value>,
) -> Outcome {
    let args = Arguments::new(shell.dialect.tool(), arguments);
    let checked = (
        args.string("command"),
        args.number("timeout"),
        args.boolean("network"),
    );
    let (command, timeout) = match checked {
        (Ok(command), Ok(timeout), Ok(_)) => (command, timeout),
        (Err(invalid), ..)
        | (_, Err(invalid), _)
        | (.., Err(invalid)) => {
            return invalid;
        }
    };
    let cwd = env.cwd().to_owned();
    match env.file_info(&cwd).await {
        Ok(info) if info.kind == FileKind::Directory => {}
        Err(error) if error.code != Code::NotFound => {
            return Outcome::failed(error.message);
        }
        _ => {
            return Outcome::failed(format!(
                "Error: the folder {cwd} is gone. Tell the chief."
            ));
        }
    }
    let wall = seconds(timeout);
    let mut kept = Tail::default();
    let mut diagnostics = Vec::new();
    let mut exit_code = -1;
    let mut ended = false;
    let fail = |kept: &mut Tail, reason: &str| {
        kept.push(&scrub_text(
            secrets,
            &format!(
                "Error: {reason}. Inspect the output before trying \
                 again, or tell the chief."
            ),
        ));
    };
    let full_output =
        |spill: &Option<String>, diagnostics: &mut Vec<_>| {
            if let Some(path) = spill {
                diagnostics.push(diagnostic(
                    DiagnosticSeverity::Info,
                    "full_output",
                    format!("Full output: {path}"),
                ));
            }
        };
    if let Some(wall) = wall {
        let ran = env
            .exec(
                &shell.invocation(command),
                Duration::from_secs_f64(wall),
                secrets,
                &mut |text| kept.push(text),
            )
            .await;
        match ran {
            Ok(ran) => {
                full_output(&ran.spill, &mut diagnostics);
                exit_code = ran.exit_code;
                ended = true;
                if exit_code != 0 {
                    diagnostics.push(diagnostic(
                        DiagnosticSeverity::Error,
                        "exit_code",
                        format!(
                            "Command exited with code {exit_code}"
                        ),
                    ));
                }
            }
            Err(ExecError::Timeout { spill }) => {
                full_output(&spill, &mut diagnostics);
                kept.push(&format!(
                    "\n[timed out after {}s]",
                    number_words(wall)
                ));
            }
            Err(ExecError::Spawn(reason)) => fail(&mut kept, &reason),
            Err(ExecError::Spill(message)) => {
                return Outcome {
                    text: kept.text(),
                    ..Outcome::thrown("tool_error", message)
                };
            }
        }
    } else {
        fail(
            &mut kept,
            "Invalid timeout: must be a finite number of seconds",
        );
    }
    if ended
        && exit_code != 0
        && REFUSED.iter().any(|words| kept.text().contains(words))
    {
        kept.push(if place.folder.is_none() {
            OUTSIDE
        } else {
            OUTSIDE_PROJECT
        });
    }
    let details = json!({
        "exitCode": exit_code,
        "policy": env.policy(),
        "degraded": env.degraded(),
    });
    Outcome {
        text: kept.text(),
        error: exit_code != 0,
        details: details.as_object().cloned(),
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    /// The source up to its inline test module.
    fn code_of(text: &str) -> String {
        let mut lines = text.lines().peekable();
        let mut code = String::new();
        while let Some(line) = lines.next() {
            let line = line.trim();
            let tests = line.starts_with("#[cfg(")
                && line.contains("test")
                && lines.peek().is_some_and(|next| {
                    let next = next.trim();
                    next.starts_with("mod ") && next.ends_with('{')
                });
            if tests {
                break;
            }
            code.push_str(line);
            code.push('\n');
        }
        code
    }

    fn sources(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs")
            {
                found.push(path);
            }
        }
    }

    #[test]
    fn shell_names_are_spelled_once() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&src, &mut files);
        let spelled = Dialect::ALL
            .map(|dialect| format!("\"{}\"", dialect.tool()));
        let mut strays = Vec::new();
        for file in files {
            let relative = file.strip_prefix(&src).unwrap();
            if relative == Path::new("tools/shell.rs")
                || relative.file_name().unwrap() == "tests.rs"
            {
                continue;
            }
            let code = code_of(&fs::read_to_string(&file).unwrap());
            for name in &spelled {
                if code.contains(name.as_str()) {
                    strays.push(format!(
                        "{} spells {name}",
                        relative.display()
                    ));
                }
            }
        }
        assert!(
            strays.is_empty(),
            "a shell's name is spelled in tools/shell.rs only, and \
             elsewhere comes from `Dialect`: {strays:?}"
        );
    }
}
