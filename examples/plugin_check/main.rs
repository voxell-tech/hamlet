//! Puts plugins through a script with no window and prints what their
//! panels showed (docs/authoring.md).
//!
//! `cargo run --example plugin_check --features wasm -- <plugin
//! dir>... [--script steps.toml] [--out dir] [--no-pictures]`
//!
//! Several folders are installed in one home, so that the plugins can
//! use each other. Without a script they run two frames and a picture
//! of each panel is drawn. The pictures go to `--out`,
//! `plugin-check-out` by default, and `--no-pictures` leaves them
//! out. The exit code is 1 when a step failed.

use std::path::PathBuf;
use std::process::ExitCode;

use hamlet_harness::script::{Script, Step};
use hamlet_harness::{Harness, run};
use hamlet_offscreen::word;

fn main() -> ExitCode {
    match check() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(why) => {
            eprintln!("plugin_check: {why}");
            ExitCode::from(2)
        }
    }
}

/// The plugin folders on the command line: the arguments that are
/// neither a flag nor the value of one that takes a value.
fn folders() -> Vec<PathBuf> {
    let mut args = std::env::args().skip(1);
    let mut folders = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--script" | "--out" => {
                args.next();
            }
            flag if flag.starts_with("--") => {}
            _ => folders.push(PathBuf::from(arg)),
        }
    }
    folders
}

/// Runs the plugins, and says whether every step worked.
fn check() -> Result<bool, String> {
    let folders = folders();
    if folders.is_empty() {
        return Err("usage: plugin_check <plugin dir>... [--script \
                    steps.toml] [--out dir] [--no-pictures]"
            .into());
    }
    let out = word("--out").map_or_else(
        || PathBuf::from("plugin-check-out"),
        PathBuf::from,
    );
    let refs =
        folders.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let mut harness = Harness::open_all(&refs)?;
    let mut script = match word("--script") {
        Some(file) => {
            let text = std::fs::read_to_string(&file)
                .map_err(|e| format!("{file}: {e}"))?;
            Script::parse(&text)
                .map_err(|e| format!("{file}: {e}"))?
        }
        None => run::plain(&mut harness),
    };
    if std::env::args().any(|arg| arg == "--no-pictures") {
        script.steps.retain(|step| !step.is_picture());
        for step in &mut script.steps {
            if let Step::Context(choosing) = step {
                choosing.file = None;
            }
        }
    }
    let report = run::run(&mut harness, &script, &out);
    report.lines.iter().for_each(|line| println!("{line}"));
    Ok(report.failed.is_none())
}
