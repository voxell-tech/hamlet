//! Puts a plugin through a script with no window and prints what its
//! panels showed (docs/authoring.md).
//!
//! `cargo run --example plugin_check --features wasm -- <plugin dir>
//! [--script steps.toml] [--out dir] [--no-pictures]`
//!
//! Without a script the plugin runs two frames and a picture of each
//! panel is drawn. The pictures go to `--out`, `plugin-check-out` by
//! default, and `--no-pictures` leaves them out. The exit code is 1
//! when a step failed.

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

/// Runs the plugin, and says whether every step worked.
fn check() -> Result<bool, String> {
    let folder = std::env::args()
        .nth(1)
        .filter(|arg| !arg.starts_with("--"))
        .map(PathBuf::from)
        .ok_or(
            "usage: plugin_check <plugin dir> [--script steps.toml] \
             [--out dir]",
        )?;
    let out = word("--out").map_or_else(
        || PathBuf::from("plugin-check-out"),
        PathBuf::from,
    );
    let mut harness = Harness::open(&folder)?;
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
        script.steps.retain(|step| !matches!(step, Step::Render(_)));
    }
    let report = run::run(&mut harness, &script, &out);
    report.lines.iter().for_each(|line| println!("{line}"));
    Ok(report.failed.is_none())
}
