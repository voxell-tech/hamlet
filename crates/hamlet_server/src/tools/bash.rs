//! Hamlet's `bash`: pi's, with a `network` flag, a time limit a model
//! may only shorten, and hints where the sandbox refused a write
//! (C16, C19). Every credential is taken out of its output before any
//! of it is kept (C32).

use hamlet_workspace_wire::Code;
use serde_json::{Map, Value, json};

use super::environment::{Reach as _, Remote};
use super::truncate::{MAX_BYTES, MAX_LINES, Tail};
use super::{
    Arguments, Outcome, Place, diagnostic, environment, number_words,
};
use crate::harness::link::DiagnosticSeverity;
use crate::sandbox::{ExecError, FileKind, Sandbox};
use crate::scrub::scrub_text;

/// Seconds a command may run, which a model can ask to shorten and
/// never to lengthen.
pub(super) const WALL_SECS: u64 = 120;

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

pub(super) fn description() -> String {
    format!(
        "Execute a bash command in the current working directory. \
         Returns combined stdout and stderr. Output is truncated to \
         last {MAX_LINES} lines or {}KB (whichever is hit first). \
         If truncated, full output is saved to a temp file. \
         Optionally provide a timeout in seconds. It stops after \
         {WALL_SECS} seconds, or sooner with `timeout`.",
        MAX_BYTES / 1024
    )
}

pub(super) async fn run(
    sandbox: &Sandbox,
    place: &Place,
    net: bool,
    secrets: &[String],
    arguments: &Map<String, Value>,
) -> Outcome {
    let args = Arguments::new("bash", arguments);
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
    let env = match environment(sandbox, place, net) {
        Ok(env) => env,
        Err(message) => {
            return Outcome::thrown("tool_error", message);
        }
    };
    let folder = env.file_info(env.cwd());
    if !folder.is_ok_and(|info| info.kind == FileKind::Directory) {
        return Outcome::failed(format!(
            "Error: the folder {} is gone. Tell the chief.",
            env.cwd()
        ));
    }
    let wall =
        timeout.unwrap_or(WALL_SECS as f64).min(WALL_SECS as f64);
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
    if !(wall.is_finite() && wall > 0.0) {
        fail(
            &mut kept,
            "Invalid timeout: must be a finite number of seconds",
        );
    } else {
        let ran = env
            .exec(command, wall, secrets, &mut |text| kept.push(text))
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
        "policy": env.policy_hash(),
        "degraded": false,
    });
    Outcome {
        text: kept.text(),
        error: exit_code != 0,
        details: details.as_object().cloned(),
        diagnostics,
    }
}

pub(super) async fn remote(
    env: &mut Remote,
    place: &Place,
    secrets: &[String],
    arguments: &Map<String, Value>,
) -> Outcome {
    let args = Arguments::new("bash", arguments);
    let (command, timeout) = match (
        args.string("command"),
        args.number("timeout"),
        args.boolean("network"),
    ) {
        (Ok(command), Ok(timeout), Ok(_)) => (command, timeout),
        (Err(invalid), ..)
        | (_, Err(invalid), _)
        | (.., Err(invalid)) => return invalid,
    };
    let wall =
        timeout.unwrap_or(WALL_SECS as f64).min(WALL_SECS as f64);
    if !(wall.is_finite() && wall > 0.0) {
        return Outcome::failed(
            "Error: Invalid timeout: must be a finite number of \
             seconds. Inspect the output before trying again, or \
             tell the chief.",
        );
    }
    let cwd = env.cwd().to_owned();
    match env.file_info(&cwd).await {
        Ok(info) if info.kind == FileKind::Directory => {}
        Err(error) if error.code != Code::NotFound => {
            return Outcome::failed(error.message);
        }
        _ => {
            return Outcome::failed(format!(
                "Error: the folder {} is gone. Tell the chief.",
                env.cwd()
            ));
        }
    }
    let ran = env.exec(command, (wall * 1000.0).ceil() as u64).await;
    let mut kept = Tail::default();
    let mut diagnostics = Vec::new();
    let mut exit = -1;
    match ran {
        Ok(ran) => {
            let output = scrub_text(
                secrets,
                &format!("{}{}", ran.stdout, ran.stderr),
            );
            if output.len() > MAX_BYTES
                || output.lines().count() > MAX_LINES
            {
                let dir = format!(
                    "/house/.tmp/bash-{}",
                    uuid::Uuid::new_v4()
                );
                let path = format!("{dir}/output.txt");
                let spill = env.write_file(&path, &output).await;
                if let Err(error) = spill {
                    return Outcome::thrown(
                        "tool_error",
                        format!(
                            "The command ran but its full output \
                             could not be saved: {}. Inspect its \
                             files before repeating it.",
                            error.message
                        ),
                    );
                }
                diagnostics.push(diagnostic(
                    DiagnosticSeverity::Info,
                    "full_output",
                    format!("Full output: {path}"),
                ));
            }
            kept.push(&output);
            exit = ran.exit_code;
            if exit != 0 {
                diagnostics.push(diagnostic(
                    DiagnosticSeverity::Error,
                    "exit_code",
                    format!("Command exited with code {exit}"),
                ));
                if REFUSED.iter().any(|words| output.contains(words))
                {
                    kept.push(if place.folder.is_none() {
                        OUTSIDE
                    } else {
                        OUTSIDE_PROJECT
                    });
                }
            }
        }
        Err(error) if error.code == Code::Timeout => kept.push(
            &format!("\n[timed out after {}s]", number_words(wall)),
        ),
        Err(error) => kept.push(&format!(
            "Error: {error}. Inspect the output before trying \
             again, or tell the chief."
        )),
    }
    let details = json!({
        "exitCode": exit,
        "policy": env.policy(),
        "degraded": false,
    });
    Outcome {
        text: kept.text(),
        error: exit != 0,
        details: details.as_object().cloned(),
        diagnostics,
    }
}
